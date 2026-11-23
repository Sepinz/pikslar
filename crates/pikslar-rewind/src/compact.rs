//! Rewriting a log that passed its project's budget: the newest steps that fit are copied into
//! a new log, the head is replaced to name it, and only then is the old log removed. A crash at
//! any moment leaves the old head and the old log, or the new ones.

use crate::frame::{self, Head, LOG_START};
use crate::write::{new_log, tidy, write_head, Hook, Step};
use crate::{RewindError, Store};
use std::fs::File;
use std::io::Write;

/// A kept step: its position and where it ends in the old log.
type Place = (u64, u64);

/// Keeps the newest steps that fit in `keep` bytes. Steps below damage are dropped with it.
/// Returns the new head and where each kept step ends.
pub fn compact(
    store: &Store,
    head: &Head,
    keep: u64,
    hook: Hook<'_>,
) -> Result<(Head, Vec<(u64, u64)>), RewindError> {
    let mut old = File::open(store.log_path(head.id, head.gen))?;
    let len = old.metadata()?.len();
    let kept = newest_that_fit(&mut old, len, head, keep);
    let gen = head.gen.wrapping_add(1);
    let mut new = new_log(&store.log_path(head.id, gen), gen)?;
    let mut at = LOG_START;
    let mut ends = Vec::with_capacity(kept.len());
    for &(pos, end) in kept.iter().rev() {
        let found = frame::read_record(&mut old, len, end, pos)?;
        let record_head = frame::RecordHead {
            prev_end: at,
            ..found.head
        };
        let bytes = frame::encode_record(at, record_head, &found.stored);
        new.write_all(&bytes)?;
        at += bytes.len() as u64;
        ends.push((pos, at));
    }
    new.sync_all()?;
    drop(new);
    hook(Step::Compacted)?;
    let base = kept.last().map_or(head.tip, |&(pos, _)| pos - 1);
    let compacted = Head {
        gen,
        base,
        tip_end: at,
        ..*head
    };
    write_head(store, &compacted, &mut |s| hook(Step::CompactHead(s)))?;
    drop(old);
    tidy(store, &compacted);
    Ok((compacted, ends))
}

/// The newest steps, newest first, whose records add up to at most `keep` bytes, as far down
/// as the log reads soundly.
fn newest_that_fit(old: &mut File, len: u64, head: &Head, keep: u64) -> Vec<Place> {
    let (mut pos, mut end, mut total) = (head.tip, head.tip_end, 0);
    let mut kept = Vec::new();
    while pos > head.base {
        let Ok(found) = frame::read_record(old, len, end, pos) else {
            break;
        };
        total += end - found.start;
        if total > keep {
            break;
        }
        kept.push((pos, end));
        (pos, end) = (pos - 1, found.head.prev_end);
    }
    kept
}
