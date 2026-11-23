//! Local diagnostics (`docs/diagnostics.md`): a small rolling log of saving, recovery, Rewind,
//! export and open events, beside the settings, so an intermittent failure leaves evidence.
//!
//! Nothing here can fail or delay what it records. A line is a few dozen bytes appended from the
//! calling thread without a flush to disk; if the folder cannot be used the log turns itself off
//! for the session and keeps the reason. A line holds the time, the version, what happened, how
//! long it took, a Windows error code and a file's base name: never a folder, a document's
//! contents, a layer, tag or slice name, or anything a script said or was given.

#[cfg(test)]
pub(crate) use crate::diag_redact::MOST_NAME;
pub(crate) use crate::diag_redact::{base_name, reduce_paths};
use crate::diag_writer::{Sink, Writer};
use crate::files::FileError;
use std::cell::RefCell;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The most one log file holds; the one before it is kept as [`OLDER_NAME`].
pub(crate) const MOST_BYTES: u64 = 64 * 1024;
pub(crate) const LOG_DIR: &str = "log";
pub(crate) const LOG_NAME: &str = "log.txt";
pub(crate) const OLDER_NAME: &str = "log.1.txt";
pub(crate) const CRASH_DIR: &str = "crash";
pub(crate) const CRASH_PREFIX: &str = "crash-";
/// Which crash note was last mentioned at launch.
pub(crate) const SEEN_NAME: &str = "seen.txt";
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// One thing that happened, as a line of the log.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Event<'a> {
    kind: &'static str,
    outcome: &'static str,
    phase: Option<&'static str>,
    ms: Option<u128>,
    code: Option<i32>,
    file: Option<&'a Path>,
    count: Option<u64>,
}

impl<'a> Event<'a> {
    pub fn new(kind: &'static str, outcome: &'static str) -> Self {
        Event {
            kind,
            outcome,
            phase: None,
            ms: None,
            code: None,
            file: None,
            count: None,
        }
    }

    /// How many of something, such as lines dropped.
    pub fn count(self, count: u64) -> Self {
        Event {
            count: Some(count),
            ..self
        }
    }

    pub fn phase(self, phase: &'static str) -> Self {
        Event {
            phase: Some(phase),
            ..self
        }
    }

    pub fn code(self, code: Option<i32>) -> Self {
        Event { code, ..self }
    }

    /// The Windows error code of `e`, if it has one.
    pub fn io(self, e: &io::Error) -> Self {
        self.code(e.raw_os_error())
    }

    /// The step a save failed at, and its Windows error code.
    pub fn failed_at(self, f: &crate::files::SaveFailure) -> Self {
        self.failed(&f.error).phase(f.phase)
    }

    /// Where a save failed, and its Windows error code.
    pub fn failed(self, e: &FileError) -> Self {
        let (phase, code) = save_failure(e);
        self.phase(phase).code(code)
    }

    pub fn took(self, started: Instant) -> Self {
        Event {
            ms: Some(started.elapsed().as_millis()),
            ..self
        }
    }

    /// Only the file's base name is written.
    pub fn file(self, path: &'a Path) -> Self {
        Event {
            file: Some(path),
            ..self
        }
    }
}

/// The line `e` makes at `at`.
pub(crate) fn line_at(at: SystemTime, e: &Event<'_>) -> String {
    let mut line = format!("{} {VERSION} {} {}", utc(at), e.kind, e.outcome);
    if let Some(phase) = e.phase {
        line.push_str(&format!(" phase={phase}"));
    }
    if let Some(ms) = e.ms {
        line.push_str(&format!(" ms={ms}"));
    }
    if let Some(code) = e.code {
        line.push_str(&format!(" code={code}"));
    }
    if let Some(count) = e.count {
        line.push_str(&format!(" n={count}"));
    }
    if let Some(file) = e.file {
        line.push_str(&format!(" file={}", base_name(file)));
    }
    line.push_str("\r\n");
    line
}

/// `at` as `YYYY-MM-DDTHH:MM:SSZ`.
pub(crate) fn utc(at: SystemTime) -> String {
    let secs = at.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days);
    let (h, min, s) = (rest / 3600, rest % 3600 / 60, rest % 60);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
}

/// The calendar date `days` after 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    (y, m, d)
}

/// The Windows error a file held by another program gave, kept inside the message that says so.
fn in_use(e: &io::Error) -> Option<i32> {
    let held = e
        .get_ref()?
        .downcast_ref::<pikslar_format::publish::InUse>()?;
    Some(held.code)
}

/// Where a save failed, as a word, and the Windows error code when there is one.
pub(crate) fn save_failure(e: &FileError) -> (&'static str, Option<i32>) {
    use pikslar_format::upgrade::UpgradeError;
    use pikslar_format::FormatError;
    let io_phase = |e: &io::Error| match (in_use(e).or(e.raw_os_error()), e.kind()) {
        (Some(code), _) => ("write", Some(code)),
        (None, io::ErrorKind::InvalidData) => ("read-back", None),
        (None, io::ErrorKind::PermissionDenied) => ("read-only", None),
        (None, _) => ("write", None),
    };
    match e {
        FileError::Format(FormatError::Io(io)) => io_phase(io),
        FileError::Format(_) => ("encode", None),
        FileError::Limit(_) => ("check", None),
        FileError::Unverified => ("verify", None),
        FileError::Upgrade(UpgradeError::Failed(FormatError::Io(io))) => {
            ("upgrade", io_phase(io).1)
        }
        FileError::Upgrade(UpgradeError::BackupInTheWay(_)) => ("upgrade-backup", None),
        FileError::Upgrade(_) => ("upgrade", None),
        FileError::Export(_) | FileError::Palette(_) => ("other", None),
    }
}

/// The two log files in one folder.
#[derive(Debug)]
pub(crate) struct Log {
    dir: PathBuf,
    file: Option<fs::File>,
    /// Why the log turned itself off, if it did.
    problem: Option<String>,
}

impl Log {
    pub fn at(dir: PathBuf) -> Log {
        Log {
            dir,
            file: None,
            problem: None,
        }
    }

    /// Adds `line`, starting a new file first when it would pass [`MOST_BYTES`]. A failure
    /// turns the log off for the session and is never reported to the caller.
    pub fn append(&mut self, line: &str) {
        if self.problem.is_some() {
            return;
        }
        if let Err(e) = self.try_append(line) {
            self.problem = Some(e.to_string());
        }
    }

    /// The file stays open between lines. Its size is asked of the open file, so another
    /// Pikslar writing the same log keeps it within the bound too.
    fn try_append(&mut self, line: &str) -> io::Result<()> {
        let path = self.dir.join(LOG_NAME);
        let add = u64::try_from(line.len()).unwrap_or(u64::MAX);
        let size = match &self.file {
            Some(file) => file.metadata()?.len(),
            None => 0,
        };
        if self.file.is_none() || size.saturating_add(add) > MOST_BYTES {
            self.file = None;
            fs::create_dir_all(&self.dir)?;
            let size = match fs::metadata(&path) {
                Ok(meta) => meta.len(),
                Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
                Err(e) => return Err(e),
            };
            if size.saturating_add(add) > MOST_BYTES {
                fs::rename(&path, self.dir.join(OLDER_NAME))?;
            }
            self.file = Some(OpenOptions::new().create(true).append(true).open(&path)?);
        }
        match &mut self.file {
            Some(file) => file.write_all(line.as_bytes()),
            None => Ok(()),
        }
    }

    /// Lets go of the open file, so it can be deleted or moved.
    fn close(&mut self) {
        self.file = None;
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }
}

/// Removes the log and the crash notes under `root`, and their folders once empty. Nothing
/// else is touched.
pub(crate) fn delete_all(root: &Path) -> io::Result<()> {
    let gone = |r: io::Result<()>| match r {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    };
    let log = root.join(LOG_DIR);
    for name in [LOG_NAME, OLDER_NAME] {
        gone(fs::remove_file(log.join(name)))?;
    }
    let crash = root.join(CRASH_DIR);
    if let Ok(entries) = fs::read_dir(&crash) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == SEEN_NAME || (name.starts_with(CRASH_PREFIX) && name.ends_with(".txt")) {
                gone(fs::remove_file(entry.path()))?;
            }
        }
    }
    // A folder that still holds something else stays.
    let _ = fs::remove_dir(&log);
    let _ = fs::remove_dir(&crash);
    Ok(())
}

/// The log as the writer thread keeps it: the files, and the folder they are deleted from.
struct LogSink {
    log: Log,
    root: PathBuf,
}

impl Sink for LogSink {
    fn line(&mut self, line: &str) {
        self.log.append(line);
    }

    fn dropped(&mut self, count: u64) {
        let e = Event::new("diagnostics", "dropped").count(count);
        self.log.append(&line_at(SystemTime::now(), &e));
    }

    fn delete(&mut self) -> io::Result<()> {
        self.log.close();
        delete_all(&self.root)
    }

    fn problem(&self) -> Option<String> {
        self.log.problem().map(str::to_string)
    }
}

/// How long the end of the program waits for the last lines to be written.
const LAST_LINES: Duration = Duration::from_millis(500);
/// How long a delete from the View menu waits for the writer.
const DELETE_WAIT: Duration = Duration::from_secs(2);

static ON: AtomicBool = AtomicBool::new(false);
static ROOT: OnceLock<PathBuf> = OnceLock::new();
static WRITER: OnceLock<Writer> = OnceLock::new();

/// Starts diagnostics under `root` (the settings folder), or none at all. Recording is on only
/// if `on`: the persisted setting, read before this, so a person who turned it off gets no
/// line and no crash note, even from the start of a launch.
pub(crate) fn start(root: Option<PathBuf>, on: bool) {
    let Some(root) = root else {
        return;
    };
    let sink = LogSink {
        log: Log::at(root.join(LOG_DIR)),
        root: root.clone(),
    };
    if let Some(writer) = Writer::spawn(sink) {
        let _ = WRITER.set(writer);
    }
    let _ = ROOT.set(root);
    set_on(on);
}

/// Turns recording on or off, as the setting says. Never on without a folder.
pub(crate) fn set_on(on: bool) {
    ON.store(on && ROOT.get().is_some(), Ordering::Relaxed);
}

pub(crate) fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The folder diagnostics live under, when they were started.
pub(crate) fn root() -> Option<&'static Path> {
    ROOT.get().map(PathBuf::as_path)
}

/// Why the log turned itself off this session, if it did.
pub(crate) fn problem() -> Option<String> {
    WRITER.get().and_then(Writer::problem)
}

/// Deletes the log and the crash notes. Recording goes on afterwards if it is on.
pub(crate) fn delete() -> io::Result<()> {
    let Some(root) = root() else {
        return Ok(());
    };
    match WRITER.get() {
        Some(writer) => writer.delete(DELETE_WAIT),
        None => delete_all(root),
    }
}

/// Waits a moment for the last lines to reach the file, as the program ends.
pub(crate) fn flush() {
    if let Some(writer) = WRITER.get() {
        writer.flush(LAST_LINES);
    }
}

/// Records `e`, or does nothing when diagnostics are off. Never fails, never panics and never
/// waits: the line is handed to the writer thread, or dropped and counted if it is behind.
pub(crate) fn note(e: Event<'_>) {
    if Capture::took(&e) || !is_on() {
        return;
    }
    let line = line_at(SystemTime::now(), &e);
    if let Some(writer) = WRITER.get() {
        writer.send(line);
    }
}

thread_local! {
    /// A test's own log for what this thread notes, and whether it is on.
    static CAPTURE: RefCell<Option<(Log, bool)>> = const { RefCell::new(None) };
}

/// While it lives, what this thread notes goes to a log in `dir` (or nowhere when off),
/// whatever the program's own diagnostics are. For tests.
pub(crate) struct Capture;

impl Capture {
    #[cfg(test)]
    pub fn to(dir: PathBuf, on: bool) -> Capture {
        CAPTURE.with(|c| *c.borrow_mut() = Some((Log::at(dir), on)));
        Capture
    }

    /// True if a capture took `e`.
    fn took(e: &Event<'_>) -> bool {
        CAPTURE.with(|c| match c.borrow_mut().as_mut() {
            Some((log, on)) => {
                if *on {
                    log.append(&line_at(SystemTime::now(), e));
                }
                true
            }
            None => false,
        })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        CAPTURE.with(|c| *c.borrow_mut() = None);
    }
}

#[cfg(test)]
#[path = "diag_tests.rs"]
mod tests;
