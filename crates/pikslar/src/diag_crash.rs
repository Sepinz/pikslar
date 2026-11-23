//! Crash notes (`docs/diagnostics.md`): when the program panics, a hook writes a few lines (the
//! time, the version, the message cut short with folders and quoted words removed, and where in
//! the program) to `crash\crash-<time>-<process>.txt` beside the settings, and keeps the newest
//! five. The release build still aborts: the hook runs first. The next launch mentions a new
//! note once on the status line.
//!
//! The hook takes no lock the program holds, allocates only a few bounded strings, writes one
//! small file, and never runs twice at once.

use crate::diag::{self, CRASH_DIR, CRASH_PREFIX, SEEN_NAME};
use std::fs;
use std::panic::PanicHookInfo;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

/// How many crash notes are kept.
pub(crate) const MOST_NOTES: usize = 5;
/// The most characters of a panic's message a note keeps.
pub(crate) const MOST_MESSAGE: usize = 300;
/// What is looked at of a message before it is cut, so a huge one costs little.
const MOST_READ: usize = 4 * MOST_MESSAGE;
/// The most entries of the crash folder looked through.
const MOST_ENTRIES: usize = 256;

/// Set while the hook runs: a panic inside it never writes a second note.
static IN_HOOK: AtomicBool = AtomicBool::new(false);

/// A note's text.
pub(crate) fn note_text(at: SystemTime, message: &str, file: &str, line: u32) -> String {
    let read: String = message.chars().take(MOST_READ).collect();
    let said: String = diag::reduce_paths(&read)
        .chars()
        .take(MOST_MESSAGE)
        .collect();
    let file = file.rsplit(['\\', '/']).next().unwrap_or(file);
    format!(
        "Pikslar crash note\r\ntime: {}\r\nversion: {}\r\nmessage: {said}\r\nlocation: {file}:{line}\r\n",
        diag::utc(at),
        env!("CARGO_PKG_VERSION"),
    )
}

/// A note's file name, which sorts by time.
pub(crate) fn note_name(at: SystemTime, pid: u32) -> String {
    let stamp: String = diag::utc(at)
        .chars()
        .filter(|c| !matches!(c, '-' | ':'))
        .collect();
    format!("{CRASH_PREFIX}{stamp}-{pid}.txt")
}

/// The crash notes in `dir`, oldest first.
fn notes(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .take(MOST_ENTRIES)
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(CRASH_PREFIX) && n.ends_with(".txt"))
        .collect();
    names.sort();
    names
}

/// Removes all but the newest [`MOST_NOTES`] notes. Best effort.
pub(crate) fn prune(dir: &Path) {
    let names = notes(dir);
    let extra = names.len().saturating_sub(MOST_NOTES);
    for name in &names[..extra] {
        let _ = fs::remove_file(dir.join(name));
    }
}

/// The newest note, if it is newer than the last one mentioned; it counts as mentioned now.
pub(crate) fn take_unseen(dir: &Path) -> Option<String> {
    let newest = notes(dir).pop()?;
    let seen = fs::read_to_string(dir.join(SEEN_NAME)).unwrap_or_default();
    if newest.as_str() <= seen.trim() {
        return None;
    }
    let _ = fs::write(dir.join(SEEN_NAME), &newest);
    Some(newest)
}

fn message_of<'a>(info: &'a PanicHookInfo<'_>) -> &'a str {
    let payload = info.payload();
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic without a message")
}

/// Writes the note for `info`, when diagnostics are on. Errors are ignored: nothing can be done
/// about them while the program is ending.
fn write_note(info: &PanicHookInfo<'_>) {
    let Some(root) = diag::root().filter(|_| diag::is_on()) else {
        return;
    };
    let dir = root.join(CRASH_DIR);
    let now = SystemTime::now();
    let (file, line) = info
        .location()
        .map_or(("unknown", 0), |l| (l.file(), l.line()));
    let text = note_text(now, message_of(info), file, line);
    if fs::create_dir_all(&dir).is_ok() {
        let _ = fs::write(dir.join(note_name(now, std::process::id())), text);
        prune(&dir);
    }
}

/// Installs the hook, before the program's own hook (which still runs afterwards).
pub(crate) fn install() {
    crate::diag_redact::capture_identity();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !IN_HOOK.swap(true, Ordering::SeqCst) {
            write_note(info);
            IN_HOOK.store(false, Ordering::SeqCst);
        }
        previous(info);
    }));
}

impl crate::app::App {
    /// Says once on the status line that the last session left a crash note in `dir`.
    pub(crate) fn mention_crash(&mut self, dir: &Path) {
        if take_unseen(dir).is_some() {
            self.notify(
                "Pikslar closed unexpectedly last time. A short crash note was kept \
                 (View, Delete crash and saving notes removes it)"
                    .to_string(),
            );
        }
    }
}

#[cfg(test)]
#[path = "diag_crash_tests.rs"]
mod tests;
