//! Writing a project's history: records appended, flushed, then the head replaced whole, which
//! is the commit point. See "Crash safety" in `docs/rewind.md`. Runs on a background thread in
//! the program; nothing here touches a document.

use crate::frame::{self, Head, RecordHead, LOG_START};
use crate::store::{log_name, Store, HEAD_FILE, LOCK_FILE};
use crate::{Budgets, RewindError};
use pikslar_format::atomic::SaveStep;
use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};

/// Payloads smaller than this are stored as they are.
const DEFLATE_FROM: usize = 256;

/// A moment during a write at which a test can stop it, as a crash would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Locked,
    Copied,
    LogStarted,
    /// Before the record with this index is written.
    Record(usize),
    Flushed,
    Head(SaveStep),
    Tidied,
    /// The rewritten log is written and flushed, before its head.
    Compacted,
    CompactHead(SaveStep),
    Evicting,
}

/// Called at each step. An error stops the write there.
pub type Hook<'a> = &'a mut dyn FnMut(Step) -> std::io::Result<()>;

/// How the steps written join the ones kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// After the kept step at `pos`, which ends at `end` in the log.
    After { pos: u64, end: u64 },
    /// A new chain whose oldest state is `base`.
    Fresh { base: u64 },
}

/// One step to write: its position, kind and payload, not yet deflated.
#[derive(Clone, Debug)]
pub struct Encoded {
    pub pos: u64,
    pub kind: u8,
    pub payload: Vec<u8>,
}

/// Everything a write needs, made on the interface thread.
pub struct Persist {
    pub id: u128,
    /// The head this session read or last wrote for `id`, or none if it knows of none. The
    /// write is refused if the store holds anything else.
    pub expect: Option<Head>,
    /// Save As: the head of the project whose log the new id starts from.
    pub copy_from: Option<Head>,
    pub link: Link,
    /// Oldest first, positions one after another from the link.
    pub records: Vec<Encoded>,
    /// The print of the saved file, which the history now ends at.
    pub print: u128,
    pub budgets: Budgets,
}

/// What a write left on disk.
#[derive(Debug)]
pub struct Written {
    pub head: Head,
    /// Where each step written ends, by position.
    pub ends: Vec<(u64, u64)>,
    /// The log was rewritten: `ends` is every step kept, and nothing known before holds.
    pub rewritten: bool,
    /// How many other projects were removed to keep the store within its budget.
    pub evicted: usize,
}

/// Writes the steps and the head, then tidies, compacts and evicts.
pub fn persist(store: &Store, job: Persist, hook: Hook<'_>) -> Result<Written, RewindError> {
    hook(Step::Locked)?;
    let _lock = store.lock(job.id)?;
    let found = match store.read_head(job.id) {
        Ok(head) => head,
        Err(RewindError::Damaged(_)) => return Err(RewindError::Changed),
        Err(e) => return Err(e),
    };
    let expected = match (&job.expect, &found) {
        (None, None) => true,
        (Some(e), Some(f)) => e.same_chain(f),
        _ => false,
    };
    if !expected {
        return Err(RewindError::Changed);
    }
    let (gen, base) = open_chain(store, &job, found.as_ref(), hook)?;
    let (tip, tip_end, ends) = append(store, &job, gen, hook)?;
    let head = Head {
        id: job.id,
        gen,
        base,
        tip,
        tip_end,
        print: job.print,
        used: crate::now_secs(),
    };
    write_head(store, &head, &mut |s| hook(Step::Head(s)))?;
    let written = Written {
        head,
        ends,
        rewritten: false,
        evicted: 0,
    };
    Ok(after_commit(store, written, job.budgets, hook))
}

/// What follows the commit point only tidies: whatever of it fails, the history just written
/// stands, so the write is reported as done with what is on disk.
fn after_commit(store: &Store, mut written: Written, budgets: Budgets, hook: Hook<'_>) -> Written {
    let head = written.head;
    if hook(Step::Tidied).is_ok() {
        tidy(store, &head);
    }
    let log = fs::metadata(store.log_path(head.id, head.gen)).map_or(0, |m| m.len());
    if log > budgets.project {
        match crate::compact::compact(store, &head, budgets.keep, hook) {
            Ok((head, ends)) => {
                (written.head, written.ends, written.rewritten) = (head, ends, true)
            }
            // The rewritten log may have been committed all the same: only its head is known.
            Err(_) => match store.read_head(head.id) {
                Ok(Some(now)) if now.gen != head.gen => {
                    (written.head, written.ends, written.rewritten) = (now, Vec::new(), true)
                }
                _ => {}
            },
        }
    }
    if hook(Step::Evicting).is_ok() {
        written.evicted = evict(store, budgets.store, head.id);
    }
    written
}

/// Makes the log the steps go in ready: copied for Save As, new for a fresh chain, or the one
/// the head names. Returns its gen and the chain's oldest state.
fn open_chain(
    store: &Store,
    job: &Persist,
    found: Option<&Head>,
    hook: Hook<'_>,
) -> Result<(u64, u64), RewindError> {
    match (job.link, &job.copy_from) {
        (Link::After { .. }, Some(from)) => {
            let (source, copy) = (
                store.log_path(from.id, from.gen),
                store.log_path(job.id, from.gen),
            );
            copy_kept(&source, &copy, from.tip_end, job.budgets.project)?;
            hook(Step::Copied)?;
            Ok((from.gen, from.base))
        }
        (Link::After { pos, .. }, None) => match found {
            Some(head) if pos >= head.base && pos <= head.tip => Ok((head.gen, head.base)),
            _ => Err(RewindError::Changed),
        },
        (Link::Fresh { base }, _) => {
            let gen = found.map_or(1, |h| h.gen.wrapping_add(1));
            let path = store.log_path(job.id, gen);
            new_log(&path, gen)?;
            hook(Step::LogStarted)?;
            Ok((gen, base))
        }
    }
}

/// Copies the first `len` bytes of a log, all a head ending there names, refused past `most`,
/// a project's budget. Bytes after the tip are never read, so they are not copied.
fn copy_kept(
    source: &std::path::Path,
    copy: &std::path::Path,
    len: u64,
    most: u64,
) -> Result<(), RewindError> {
    let too_large = RewindError::Damaged("the history to copy is larger than a project keeps");
    if len > most {
        return Err(too_large);
    }
    let mut from = File::open(source)?;
    if from.metadata()?.len() < len {
        return Err(RewindError::Damaged(
            "the Rewind log is shorter than its head says",
        ));
    }
    let mut to = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(copy)?;
    std::io::copy(&mut std::io::Read::take(&mut from, len), &mut to)?;
    Ok(())
}

/// A new log holding only its start, flushed. Whatever had its name is no log a head names.
pub(crate) fn new_log(path: &std::path::Path, gen: u64) -> Result<File, RewindError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&frame::log_start(gen))?;
    file.sync_all()?;
    Ok(file)
}

/// The new tip, where it ends, and where each step written ends.
type Appended = (u64, u64, Vec<(u64, u64)>);

/// Appends the steps past the end of the log and flushes them. Returns the new tip, where it
/// ends, and where each step ends.
fn append(store: &Store, job: &Persist, gen: u64, hook: Hook<'_>) -> Result<Appended, RewindError> {
    let (mut tip, mut last_end) = match job.link {
        Link::After { pos, end } => (pos, end),
        Link::Fresh { base } => (base, LOG_START),
    };
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(store.log_path(job.id, gen))?;
    let mut at = file.seek(SeekFrom::End(0))?;
    let mut ends = Vec::with_capacity(job.records.len() + 1);
    // The step written on keeps its place, so a session can read below it.
    if let Link::After { pos, end } = job.link {
        ends.push((pos, end));
    }
    for (n, record) in job.records.iter().enumerate() {
        hook(Step::Record(n))?;
        if record.pos != tip + 1 {
            return Err(RewindError::Damaged("the steps to write are out of order"));
        }
        let bytes = framed(at, record, last_end)?;
        file.write_all(&bytes)?;
        at += bytes.len() as u64;
        (tip, last_end) = (record.pos, at);
        ends.push((tip, last_end));
    }
    hook(Step::Flushed)?;
    file.sync_all()?;
    Ok((tip, last_end, ends))
}

/// One step framed for `start`, deflated when that makes it smaller.
fn framed(start: u64, record: &Encoded, prev_end: u64) -> Result<Vec<u8>, RewindError> {
    let decoded = u32::try_from(record.payload.len())
        .ok()
        .filter(|&n| n <= frame::MOST_DECODED)
        .ok_or(RewindError::Damaged("a step is too large to keep"))?;
    let packed = (record.payload.len() > DEFLATE_FROM)
        .then(|| pikslar_format::deflate::deflate(&record.payload))
        .filter(|packed| packed.len() < record.payload.len())
        // Past what a reader lets a payload grow by, it is kept as it is.
        .filter(|packed| packed.len() as u64 * frame::MOST_RATIO >= record.payload.len() as u64);
    let (flags, stored) = match &packed {
        Some(packed) => (frame::DEFLATED, &packed[..]),
        None => (0, &record.payload[..]),
    };
    if stored.len() as u64 + frame::HEADER + frame::TRAILER > frame::MOST_RECORD {
        return Err(RewindError::Damaged("a step is too large to keep"));
    }
    let head = RecordHead {
        kind: record.kind,
        flags,
        pos: record.pos,
        prev_end,
        decoded,
    };
    Ok(frame::encode_record(start, head, stored))
}

/// Replaces the head whole: written beside it, flushed, then renamed over it.
pub(crate) fn write_head(
    store: &Store,
    head: &Head,
    hook: &mut dyn FnMut(SaveStep) -> std::io::Result<()>,
) -> Result<(), RewindError> {
    let path = store.project_dir(head.id).join(HEAD_FILE);
    pikslar_format::atomic::write_atomic_checked(&path, &head.encode(), &mut |step, _| hook(step))?;
    Ok(())
}

/// Removes what interrupted writes left: every file but the head, the lock and the log the
/// head names. Best effort; whatever stays is removed next time.
pub(crate) fn tidy(store: &Store, head: &Head) {
    let dir = store.project_dir(head.id);
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let log = log_name(head.gen);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let keep = [HEAD_FILE, LOCK_FILE, log.as_str()].contains(&&*name.to_string_lossy());
        if !keep {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Removes whole projects, least recently used first, until the store is within `budget`.
/// The project being written is kept, and so is one another Pikslar holds.
pub fn evict(store: &Store, budget: u64, keep: u128) -> usize {
    store.sweep();
    // Only projects count, so removing them is always what brings the total down.
    let mut projects = store.projects();
    let mut total: u64 = projects.iter().map(|p| p.bytes).sum();
    projects.sort_by_key(|p| (p.used, p.id));
    let mut evicted = 0;
    for project in projects {
        if total <= budget {
            break;
        }
        if project.id != keep && store.forget(project.id).is_ok() {
            total = total.saturating_sub(project.bytes);
            evicted += 1;
        }
    }
    evicted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_that_deflates_past_what_a_reader_takes_is_kept_as_it_is() {
        let record = |payload: Vec<u8>| Encoded {
            pos: 1,
            kind: 1,
            payload,
        };
        let flags = |bytes: &[u8]| u16::from_le_bytes([bytes[6], bytes[7]]);
        // However well it packs, a reader takes what is written.
        let flat = framed(LOG_START, &record(vec![0; 1 << 20]), LOG_START).expect("framed");
        let stored = (flat.len() - 48) as u64;
        assert!(flags(&flat) == 0 || stored * frame::MOST_RATIO >= 1 << 20);
        let mixed: Vec<u8> = (0..4096u32).map(|n| (n * 7 % 251) as u8).collect();
        let some = framed(LOG_START, &record(mixed), LOG_START).expect("framed");
        assert_eq!(flags(&some), frame::DEFLATED);
    }
}
