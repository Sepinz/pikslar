//! A private copy of unsaved work, so a crash or a Windows shutdown cannot lose it.
//!
//! Each running Pikslar owns one stem in the recovery folder and holds `<stem>.lock` open
//! without sharing. Windows releases that lock however the process ends, so a copy whose
//! lock can be opened was left behind by a session that did not exit normally.

use crate::diag::{note, Event};
use crate::files::same_file;
use crate::pending::{wait_until, QUIT_WAIT, UI_WAIT};
use pikslar_core::{Document, FloatingSnapshot};
use pikslar_format::{piks, write_atomic};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, UNIX_EPOCH};

const NO_SHARING: u32 = 0;

/// When a file was last written and how long it was, to notice someone saving over it.
pub type FileStamp = (u64, u64);

pub(crate) fn file_stamp(path: &Path) -> Option<FileStamp> {
    let meta = fs::metadata(path).ok()?;
    let at = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((at.as_nanos() as u64, meta.len()))
}

/// Where this session's copy goes, for writing it without the [`Recovery`] at hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CopyFiles {
    project: PathBuf,
    sidecar: PathBuf,
    stamp: Option<FileStamp>,
}

impl CopyFiles {
    /// Writes the copy of `doc` and its sidecar, as a write by the session would.
    pub(crate) fn write(
        &self,
        doc: &Document,
        floating: Option<&FloatingSnapshot>,
        origin: Origin<'_>,
    ) -> io::Result<()> {
        let origin = origin_text(origin, self.stamp);
        write_copy(&self.project, &self.sidecar, doc, floating, &origin)
    }
}

/// Unsaved work found from an earlier session.
#[derive(Debug, PartialEq, Eq)]
pub struct Orphan {
    pub project: PathBuf,
    pub origin: Option<PathBuf>,
    /// How the origin looked when the session that left this copy last read or wrote it.
    pub stamp: Option<FileStamp>,
    /// A file holding what that session could not keep, which must never be saved over. It is
    /// the origin unless the work was since saved under another name.
    pub protected: Option<PathBuf>,
}

/// Which file a session's work belongs to, as its recovery copy records it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Origin<'a> {
    pub path: Option<&'a Path>,
    /// A file saving must not replace, because it holds what this version could not read.
    pub protected: Option<&'a Path>,
}

impl<'a> Origin<'a> {
    #[cfg(test)]
    pub const NONE: Origin<'static> = Origin {
        path: None,
        protected: None,
    };

    #[cfg(test)]
    pub fn file(path: &'a Path) -> Self {
        Origin {
            path: Some(path),
            protected: None,
        }
    }

    /// A session's file and the file it must not replace. With no file of its own, the work is
    /// named for the protected one, so a restore offers it by name.
    pub fn of(path: &'a Option<PathBuf>, protected: &'a Option<PathBuf>) -> Self {
        Origin {
            path: path.as_deref().or(protected.as_deref()),
            protected: protected.as_deref(),
        }
    }
}

impl Orphan {
    /// False if saving over the origin would destroy something this work never saw.
    pub fn may_save_over_origin(&self) -> bool {
        let origin_protected = self.origin.as_deref().zip(self.protected.as_deref());
        if origin_protected.is_some_and(|(origin, protected)| same_file(origin, protected)) {
            return false;
        }
        match self.origin.as_deref().map(file_stamp) {
            Some(Some(now)) => self.stamp == Some(now),
            _ => true,
        }
    }

    pub fn display_name(&self) -> String {
        self.origin
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(
                || "Untitled".to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
    }
}

struct Active {
    dir: PathBuf,
    stem: String,
    _lock: File,
}

impl Active {
    fn file(&self, extension: &str) -> PathBuf {
        self.dir.join(format!("{}.{extension}", self.stem))
    }
}

/// Where this session's copy lives. Disabled when the folder cannot be used.
#[derive(Default)]
pub struct Recovery {
    active: Option<Active>,
    writer: Option<JoinHandle<io::Result<()>>>,
    /// Leave the copy behind on exit, because Windows is ending the session.
    keep: bool,
    origin_stamp: Option<FileStamp>,
    /// A write refused while the last one runs has been noted in the diagnostics: once per
    /// write, so a stuck one cannot fill the log.
    skip_noted: bool,
    /// The work was saved while a write was under way: the copy goes once that ends.
    clear_pending: bool,
}

/// How a recovery write went, in the diagnostics.
fn note_write(kind: &'static str, started: std::time::Instant, written: &io::Result<()>) {
    match written {
        Ok(()) => note(Event::new(kind, "ok").took(started)),
        Err(e) => note(Event::new(kind, "fail").took(started).io(e)),
    }
}

/// In full, so a later session started from another folder finds the same file.
fn full(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// The path on one line and its stamp on the next, which may be empty, then the file that must
/// not be saved over, if any. Older copies have only the first one or two lines.
/// The sidecar's first three lines: the origin, its stamp and the protected file, each empty
/// when there is none. [`paired_text`] adds the fourth.
fn origin_text(origin: Origin<'_>, stamp: Option<FileStamp>) -> String {
    let Some(path) = origin.path else {
        return "\n\n".to_string();
    };
    let stamp = stamp.map_or_else(String::new, |(at, len)| format!("{at} {len}"));
    let protected = origin.protected.map(full).unwrap_or_default();
    format!("{}\n{stamp}\n{protected}", full(path))
}

/// The sidecar for a copy whose bytes are `copy`: the origin lines and, last, the print of
/// those bytes. The two files are written one after the other, so a process stopped between
/// them leaves a sidecar whose print is not the copy's, and its origin is not used.
fn paired_text(origin: &str, copy: &[u8]) -> String {
    format!("{origin}\n{:032x}", pikslar_rewind::hash128(copy))
}

/// What a sidecar says.
#[derive(Debug, Default, PartialEq, Eq)]
struct Sidecar {
    origin: Option<PathBuf>,
    stamp: Option<FileStamp>,
    protected: Option<PathBuf>,
    /// The print of the copy it was written with; none from versions before B1.
    print: Option<u128>,
}

fn parse_sidecar(text: &str) -> Sidecar {
    let (origin, stamp, protected) = parse_origin(text);
    let print = text
        .lines()
        .nth(3)
        .and_then(|line| u128::from_str_radix(line, 16).ok());
    Sidecar {
        origin,
        stamp,
        protected,
        print,
    }
}

fn parse_origin(text: &str) -> (Option<PathBuf>, Option<FileStamp>, Option<PathBuf>) {
    let mut lines = text.lines();
    let origin = lines.next().filter(|p| !p.is_empty()).map(PathBuf::from);
    let stamp = lines.next().and_then(|line| {
        let (at, len) = line.split_once(' ')?;
        Some((at.parse().ok()?, len.parse().ok()?))
    });
    let protected = match lines.next().filter(|p| !p.is_empty() && origin.is_some()) {
        // One development build marked a protected origin with this word alone.
        Some("protected") => origin.clone(),
        other => other.map(PathBuf::from),
    };
    (origin, stamp, protected)
}

/// The origin goes first, so a copy never appears without knowing where it came from.
fn write_copy(
    project: &Path,
    sidecar: &Path,
    doc: &Document,
    floating: Option<&FloatingSnapshot>,
    origin: &str,
) -> io::Result<()> {
    let bytes = piks::try_save_recovery(doc, floating).map_err(io::Error::other)?;
    let paired = paired_text(origin, &bytes);
    write_atomic(sidecar, paired.as_bytes()).map_err(io::Error::other)?;
    write_atomic(project, &bytes).map_err(io::Error::other)
}

/// Beside a copy whose work a normal exit dealt with while its write was still under way: the
/// copy is never offered, and the next launch clears it.
const CLEARED: &str = "cleared";

/// Everything a session may leave beside `project`. Best effort: a leftover is harmless.
fn remove_session_files(project: &Path) {
    for extension in [
        "piks",
        "origin",
        "lock",
        CLEARED,
        "piks.pikslar-tmp",
        "origin.pikslar-tmp",
        // Left by versions before the temporary name changed.
        "piks.tmp",
        "origin.tmp",
    ] {
        let _ = fs::remove_file(project.with_extension(extension));
    }
}

/// True if no running session holds this lock file.
fn is_abandoned(lock: &Path) -> bool {
    OpenOptions::new()
        .write(true)
        .share_mode(NO_SHARING)
        .open(lock)
        .is_ok()
}

impl Recovery {
    pub fn disabled() -> Self {
        Recovery::default()
    }

    /// The usual per-user location, or `disabled` when Windows does not provide one.
    pub fn default_dir() -> Option<PathBuf> {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(PathBuf::from(base).join("Pikslar").join("recovery"))
    }

    #[cfg(test)]
    pub fn in_dir(dir: &Path) -> Self {
        Self::try_in_dir(dir).unwrap_or_default()
    }

    pub fn try_in_dir(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let stem = format!("{}-{unique}", std::process::id());
        let lock = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(NO_SHARING)
            .open(dir.join(format!("{stem}.lock")))?;
        Ok(Recovery {
            active: Some(Active {
                dir: dir.to_path_buf(),
                stem,
                _lock: lock,
            }),
            writer: None,
            keep: false,
            origin_stamp: None,
            skip_noted: false,
            clear_pending: false,
        })
    }

    /// Remembers how the session's file looks now, as just opened or saved.
    pub fn note_origin(&mut self, origin: Option<&Path>) {
        self.origin_stamp = origin.and_then(file_stamp);
    }

    pub fn is_enabled(&self) -> bool {
        self.active.is_some()
    }

    /// Writes on another thread, unless the previous write is still running.
    ///
    /// Documents share their tiles, so the copy passed in costs almost nothing and the
    /// interface never waits for the disk. Returns false if the write could not start.
    pub fn write_in_background(
        &mut self,
        doc: Document,
        floating: Option<FloatingSnapshot>,
        origin: Origin<'_>,
    ) -> bool {
        if self.writer.as_ref().is_some_and(|w| !w.is_finished()) {
            if !std::mem::replace(&mut self.skip_noted, true) {
                note(Event::new("recovery-write", "skip-busy"));
            }
            return false;
        }
        let Some(active) = &self.active else {
            return false;
        };
        let (project, sidecar) = (active.file("piks"), active.file("origin"));
        let origin = origin_text(origin, self.origin_stamp);
        self.skip_noted = false;
        // Windows ending the session waits for this write (`session_end.rs`).
        let pending = pikslar_platform::write_pending(pikslar_platform::WriteKind::Recovery);
        self.writer = Some(std::thread::spawn(move || {
            let _pending = pending;
            let started = std::time::Instant::now();
            let written = write_copy(&project, &sidecar, &doc, floating.as_ref(), &origin);
            note_write("recovery-write", started, &written);
            written
        }));
        true
    }

    /// Reports a failed background write once, after it has finished.
    pub fn take_error(&mut self) -> Option<io::Error> {
        if !self.writer.as_ref().is_some_and(JoinHandle::is_finished) {
            return None;
        }
        let joined = self.writer.take()?.join();
        if std::mem::take(&mut self.clear_pending) {
            // The work it copied was saved meanwhile: the copy goes, whatever became of it.
            self.remove_copy();
            note(Event::new("recovery-clear", "done-late"));
            return None;
        }
        match joined {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e),
            Err(_) => Some(io::Error::other("the recovery writer stopped unexpectedly")),
        }
    }

    #[cfg(test)]
    pub(crate) fn wait_for_writer(&mut self) {
        if let Some(writer) = self.writer.take() {
            // The result is superseded by whatever the caller does next.
            let _ = writer.join();
        }
    }

    /// Waits up to `within` for the write under way. True once none is; its result is
    /// superseded by whatever the caller does next.
    fn writer_done_within(&mut self, within: Duration) -> bool {
        let Some(writer) = &self.writer else {
            return true;
        };
        if !wait_until(within, || writer.is_finished()) {
            return false;
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        true
    }

    /// Writes now and waits, for when the process is about to end.
    #[cfg(test)]
    pub fn write_now(
        &mut self,
        doc: &Document,
        floating: Option<&FloatingSnapshot>,
        origin: Origin<'_>,
    ) -> io::Result<()> {
        self.write_now_within(doc, floating, origin, QUIT_WAIT)
    }

    /// As [`Recovery::write_now`], waiting at most `within` for a write already under way,
    /// which writes the same files: past that nothing is written and the error says why.
    pub fn write_now_within(
        &mut self,
        doc: &Document,
        floating: Option<&FloatingSnapshot>,
        origin: Origin<'_>,
        within: Duration,
    ) -> io::Result<()> {
        if !self.writer_done_within(within) {
            return Err(io::Error::other(
                "the backup before this one is still being written",
            ));
        }
        self.clear_pending = false;
        let Some(active) = &self.active else {
            return Err(io::Error::other("recovery is unavailable"));
        };
        write_copy(
            &active.file("piks"),
            &active.file("origin"),
            doc,
            floating,
            &origin_text(origin, self.origin_stamp),
        )
    }

    /// Chooses whether the copy survives this process ending.
    pub fn set_keep(&mut self, keep: bool) {
        self.keep = keep;
    }

    /// Removes this session's copy, once the work is safely saved. With a write still under
    /// way after a moment, the copy goes when it ends ([`Recovery::take_error`]): false then.
    pub fn clear(&mut self) -> bool {
        if !self.writer_done_within(UI_WAIT) {
            if !std::mem::replace(&mut self.clear_pending, true) {
                note(Event::new("recovery-clear", "pending"));
            }
            return false;
        }
        self.clear_pending = false;
        self.remove_copy();
        true
    }

    /// Where the copy goes, while recovery is in use.
    pub(crate) fn copy_files(&self) -> Option<CopyFiles> {
        let active = self.active.as_ref()?;
        Some(CopyFiles {
            project: active.file("piks"),
            sidecar: active.file("origin"),
            stamp: self.origin_stamp,
        })
    }

    /// Whether the copy waits for the write under way to end before it goes.
    pub fn clear_waiting(&self) -> bool {
        self.clear_pending
    }

    fn remove_copy(&self) {
        if let Some(active) = &self.active {
            for extension in ["piks", "origin"] {
                // A copy that was never written is already in the wanted state.
                let _ = fs::remove_file(active.file(extension));
            }
        }
    }

    /// Finds the newest copy left behind by a session that is no longer running.
    pub fn find_orphan(&self) -> Option<Orphan> {
        let active = self.active.as_ref()?;
        let mine = format!("{}.piks", active.stem);
        let newest = fs::read_dir(&active.dir)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|e| e == "piks")
                    && path.file_name().is_some_and(|n| n != mine.as_str())
                    && is_abandoned(&path.with_extension("lock"))
                    && !path.with_extension(CLEARED).exists()
            })
            .max_by_key(|path| fs::metadata(path).and_then(|m| m.modified()).ok())?;
        let text = fs::read_to_string(newest.with_extension("origin")).unwrap_or_default();
        let side = parse_sidecar(&text);
        let paired = side.print.is_none_or(|print| {
            fs::read(&newest).is_ok_and(|copy| pikslar_rewind::hash128(&copy) == print)
        });
        if !paired {
            note(Event::new("recovery-pair", "mismatch").file(&newest));
        }
        // An origin from another copy is never used: the work could be saved over a file it
        // does not belong to. A protected file stays protected, whichever copy named it.
        Some(Orphan {
            project: newest,
            origin: side.origin.filter(|_| paired),
            stamp: side.stamp.filter(|_| paired),
            protected: side.protected,
        })
    }

    /// Clears what crashed sessions left that holds no work: locks and unfinished writes.
    pub fn sweep(&self) {
        let Some(active) = &self.active else {
            return;
        };
        let Ok(entries) = fs::read_dir(&active.dir) else {
            return;
        };
        for lock in entries.flatten().map(|entry| entry.path()) {
            let is_lock = lock.extension().is_some_and(|e| e == "lock");
            let holds_no_work =
                !lock.with_extension("piks").exists() || lock.with_extension(CLEARED).exists();
            if is_lock && holds_no_work && is_abandoned(&lock) {
                remove_session_files(&lock.with_extension("piks"));
            }
        }
    }

    /// Deletes a copy the user chose not to keep.
    pub fn discard(orphan: &Orphan) {
        remove_session_files(&orphan.project);
    }

    /// Moves a copy that would not open out of the way without destroying it, in case it
    /// can still be rescued. Returns where it now is.
    pub fn set_aside(orphan: &Orphan) -> PathBuf {
        let kept = orphan.project.with_extension("piks.bad");
        if fs::rename(&orphan.project, &kept).is_err() {
            // It stays where it is and is offered again next launch.
            return orphan.project.clone();
        }
        for extension in ["origin", "lock"] {
            // A leftover sidecar is harmless: only `.piks` files are offered.
            let _ = fs::remove_file(orphan.project.with_extension(extension));
        }
        kept
    }

    /// Makes a restored copy this session's own, so it stays protected if we crash too.
    pub fn adopt(&mut self, orphan: &Orphan) -> io::Result<()> {
        let Some(active) = &self.active else {
            return Err(io::Error::other("recovery is unavailable"));
        };
        // The stamp carries over, so a second crash still knows the file moved on.
        let origin = Origin {
            path: orphan.origin.as_deref(),
            protected: orphan.protected.as_deref(),
        };
        let origin = origin_text(origin, orphan.stamp);
        let paired = paired_text(&origin, &fs::read(&orphan.project)?);
        write_atomic(&active.file("origin"), paired.as_bytes()).map_err(io::Error::other)?;
        fs::rename(&orphan.project, active.file("piks"))?;
        Recovery::discard(orphan);
        Ok(())
    }
}

#[cfg(test)]
impl Recovery {
    /// Makes every later write fail, the way a full disk would.
    pub fn block_writes(&self) {
        if let Some(active) = &self.active {
            fs::create_dir_all(active.file("piks.pikslar-tmp")).expect("blocker");
        }
    }

    /// A write that does not end until `gate` says so, or 10 s pass: a stuck disk.
    pub fn hold_writer(&mut self, gate: std::sync::mpsc::Receiver<()>) {
        self.writer = Some(std::thread::spawn(move || {
            let _ = gate.recv_timeout(Duration::from_secs(10));
            Ok(())
        }));
    }

    /// As [`Recovery::hold_writer`], counted as a recovery write under way, the way a real
    /// one is: the session end waits for it. The count is process-wide, so tests using this
    /// hold `test_app::SESSION_LOCK`.
    pub fn hold_counted_writer(&mut self, gate: std::sync::mpsc::Receiver<()>) {
        let pending = pikslar_platform::write_pending(pikslar_platform::WriteKind::Recovery);
        self.writer = Some(std::thread::spawn(move || {
            let _pending = pending;
            let _ = gate.recv_timeout(Duration::from_secs(10));
            Ok(())
        }));
    }

    /// Lets go of everything, the way the operating system does when a process dies.
    pub fn simulate_crash(&mut self) {
        self.wait_for_writer();
        self.active = None;
    }
}

impl Recovery {
    /// Marks the copy as dealt with, for an exit that cannot wait for its write. True if the
    /// mark is in place.
    fn mark_cleared(&self) -> bool {
        let Some(active) = &self.active else {
            return false;
        };
        let marked = fs::write(active.file(CLEARED), b"").is_ok();
        let how = if marked { "left-writing" } else { "unmarked" };
        note(Event::new("recovery-clear", how));
        marked
    }
}

impl Drop for Recovery {
    /// A normal exit has already dealt with the user's work.
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        if !self.writer_done_within(QUIT_WAIT) && self.mark_cleared() {
            // The write may still leave a copy after this: the mark keeps it from being
            // offered, and the lock file stays so the next launch's sweep clears them all.
            self.remove_copy();
            self.active = None;
            return;
        }
        self.remove_copy();
        if let Some(active) = self.active.take() {
            let lock = active.file("lock");
            drop(active);
            // A leftover lock with no copy beside it is ignored by `find_orphan`.
            let _ = fs::remove_file(lock);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::ColorMode;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pikslar-recovery-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn doc() -> Document {
        Document::new(8, 8, ColorMode::Palette).expect("valid")
    }

    fn abandon(mut session: Recovery) {
        session.wait_for_writer();
        session.active = None;
    }

    #[test]
    fn a_running_session_is_never_treated_as_abandoned() {
        let dir = temp_dir("live");
        let mut first = Recovery::in_dir(&dir);
        first.write_now(&doc(), None, Origin::NONE).expect("write");
        assert_eq!(Recovery::in_dir(&dir).find_orphan(), None);
        drop(first);
        fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn a_document_with_tilesets_gets_a_recovery_copy() {
        let dir = temp_dir("tilesets");
        let mut tiled = doc();
        tiled
            .add_tileset("Walls", ColorMode::Palette, (8, 8))
            .expect("room");
        let mut session = Recovery::in_dir(&dir);
        session
            .write_now(&tiled, None, Origin::NONE)
            .expect("tilesets are written");
        drop(session);
        fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn a_crashed_session_leaves_work_that_can_be_restored() {
        let dir = temp_dir("crash");
        let mut first = Recovery::in_dir(&dir);
        let origin = PathBuf::from("C:/art/hero.piks");
        first
            .write_now(&doc(), None, Origin::file(&origin))
            .expect("write");
        abandon(first);
        let mut second = Recovery::in_dir(&dir);
        let orphan = second.find_orphan().expect("found");
        assert_eq!(orphan.origin, Some(origin));
        assert_eq!(orphan.display_name(), "hero.piks");
        second.adopt(&orphan).expect("adopt");
        assert_eq!(second.find_orphan(), None);
        drop(second);
        fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn background_writes_finish_and_a_normal_exit_leaves_nothing() {
        let dir = temp_dir("clean");
        let mut session = Recovery::in_dir(&dir);
        assert!(session.write_in_background(doc(), None, Origin::NONE));
        session.wait_for_writer();
        assert_eq!(session.take_error().map(|e| e.to_string()), None);
        drop(session);
        assert_eq!(fs::read_dir(&dir).expect("dir").count(), 0);
        fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn a_disabled_session_refuses_rather_than_pretending() {
        let mut session = Recovery::disabled();
        assert!(session.write_now(&doc(), None, Origin::NONE).is_err());
        assert!(!session.write_in_background(doc(), None, Origin::NONE));
        assert_eq!(session.find_orphan(), None);
    }

    #[test]
    fn a_copy_that_will_not_open_is_kept_but_not_offered_again() {
        let dir = temp_dir("aside");
        let mut first = Recovery::in_dir(&dir);
        first.write_now(&doc(), None, Origin::NONE).expect("write");
        abandon(first);
        let second = Recovery::in_dir(&dir);
        let kept = Recovery::set_aside(&second.find_orphan().expect("found"));
        assert!(kept.is_file());
        assert_eq!(kept.extension().and_then(|e| e.to_str()), Some("bad"));
        assert_eq!(second.find_orphan(), None);
        drop(second);
        fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn discarding_removes_the_abandoned_files() {
        let dir = temp_dir("discard");
        let mut first = Recovery::in_dir(&dir);
        first.write_now(&doc(), None, Origin::NONE).expect("write");
        abandon(first);
        let second = Recovery::in_dir(&dir);
        Recovery::discard(&second.find_orphan().expect("found"));
        assert_eq!(second.find_orphan(), None);
        drop(second);
        fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn copies_from_older_builds_read_the_same_and_only_a_named_file_is_protected() {
        let path = PathBuf::from("C:/art/hero.piks");
        let copy = PathBuf::from("C:/art/copy.piks");
        assert_eq!(parse_origin(""), (None, None, None));
        let one_line = parse_origin("C:/art/hero.piks");
        assert_eq!(one_line, (Some(path.clone()), None, None));
        let two_lines = parse_origin("C:/art/hero.piks\n5 9");
        assert_eq!(two_lines, (Some(path.clone()), Some((5, 9)), None));
        let (none, kept) = (None, Some(path.clone()));
        let protected = Origin::of(&none, &kept);
        let written = origin_text(protected, None);
        let full = std::path::absolute(&path).expect("full");
        assert_eq!(
            parse_origin(&written),
            (Some(full.clone()), None, Some(full.clone()))
        );
        let saved = Some(copy.clone());
        let elsewhere = Origin::of(&saved, &kept);
        let written = origin_text(elsewhere, Some((5, 9)));
        let copy = std::path::absolute(&copy).expect("full");
        assert_eq!(
            parse_origin(&written),
            (Some(copy), Some((5, 9)), Some(full))
        );
        assert_eq!(parse_origin("\n\nC:/art/hero.piks"), (None, None, None));
        let marked = parse_origin("C:/art/hero.piks\n\nprotected");
        assert_eq!(marked, (Some(path.clone()), None, Some(path)));
    }
}

#[cfg(test)]
#[path = "recovery_pair_tests.rs"]
mod pair_tests;

#[cfg(test)]
#[path = "recovery_exit_tests.rs"]
mod exit_tests;
