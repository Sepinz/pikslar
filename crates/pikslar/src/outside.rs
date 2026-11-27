//! A project another program changed since it was opened or saved here (B1, section 13,
//! requirement 5).
//!
//! Opening and each save remember the file's size and write time and the print of its bytes.
//! Before Save writes over the document's own file the window looks again: a file as it was,
//! or with new times and the same bytes, is saved over without a word; any other change asks
//! Overwrite, Save as or Cancel. The saving thread looks once more just before the file is
//! replaced, holding it open meanwhile so no other program can open it to write.
//!
//! What is left: a program that replaces the file by renaming over it (as Pikslar does) can
//! still do so between that last look and the replace, because the hold must let the replace
//! itself delete the file. On FAT and exFAT drives write times are kept to two seconds, so a
//! change of the same size within two seconds of the remembered time is not seen.

use crate::app::App;
use crate::diag::{note, Event};
use crate::recovery::{file_stamp, FileStamp};
use pikslar_format::publish::Stamp;
use pikslar_platform::{ChangedChoice, Window};
use std::fs::{self, File};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long the window, answering meanwhile, waits for the bytes to be compared before a
/// save. A file not read by then counts as changed, so the person is asked.
pub(crate) const COMPARE_WAIT: Duration = Duration::from_secs(5);

const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_DELETE: u32 = 4;

/// How a project's file looked when this session last read or wrote it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) stamp: FileStamp,
    /// The print of its bytes, when known; without one only the stamp is compared.
    pub(crate) print: Option<u128>,
}

impl Seen {
    pub(crate) fn of(path: &Path, print: Option<u128>) -> Option<Seen> {
        file_stamp(path).map(|stamp| Seen { stamp, print })
    }

    /// How a file just saved looks: the stamp it read back with and the print of its bytes.
    /// Without a stamp (the file changed while it was read back) no stamp can match, so the
    /// next save compares the bytes, and asks if they are not these.
    pub(crate) fn after_save(stamp: Option<Stamp>, print: u128) -> Seen {
        let stamp = stamp.and_then(|s| {
            let at = s.modified.duration_since(std::time::UNIX_EPOCH).ok()?;
            Some((at.as_nanos() as u64, s.len))
        });
        Seen {
            stamp: stamp.unwrap_or((0, u64::MAX)),
            print: Some(print),
        }
    }
}

/// `seen`, brought up to date, if `path` holds what it did: the same stamp, the same bytes
/// under a new stamp, or no file at all (nothing there can be lost). None if it changed.
pub(crate) fn still_same(path: &Path, seen: &Seen) -> Option<Seen> {
    let Some(now) = file_stamp(path) else {
        return Some(*seen);
    };
    if now == seen.stamp {
        return Some(*seen);
    }
    let print = seen.print?;
    let bytes = fs::read(path).ok()?;
    (pikslar_rewind::hash128(&bytes) == print).then_some(Seen {
        stamp: now,
        print: Some(print),
    })
}

/// As [`still_same`], reading the bytes on another thread while the window answers, for at
/// most `within`: slow storage never freezes the window. A stamp that matches settles it
/// without reading. A read not done in time, or one that cannot start, counts as a change.
fn still_same_answering(
    window: Window,
    path: &Path,
    seen: Seen,
    (within, delay): (Duration, Option<Duration>),
) -> Option<Seen> {
    match file_stamp(path) {
        None => return Some(seen),
        Some(now) if now == seen.stamp => return Some(seen),
        Some(_) => {}
    }
    seen.print?;
    let (put, take) = mpsc::channel();
    let target = path.to_path_buf();
    let started = std::thread::Builder::new()
        .name("compare".into())
        .spawn(move || {
            if let Some(delay) = delay {
                std::thread::sleep(delay);
            }
            // Nobody listens once the window stopped waiting; the answer then goes unused.
            let _ = put.send(still_same(&target, &seen));
        });
    if started.is_err() {
        return None;
    }
    let end = Instant::now() + within;
    let mut answer = None;
    pikslar_platform::wait_answering(
        window,
        || {
            answer = take.try_recv().ok();
            answer.is_none() && Instant::now() < end
        },
        |_| {},
    );
    if answer.is_none() {
        note(Event::new("save", "compare-slow").file(path));
    }
    answer.flatten()
}

/// Opens `path` so that no other program can open it to write until the handle is dropped,
/// while a replace can still move it away.
pub(crate) fn hold(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .open(path)
}

/// The file changed while it was being saved.
#[derive(Debug)]
pub(crate) struct ChangedWhileSaving(pub(crate) String);

impl std::fmt::Display for ChangedWhileSaving {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} was changed by another program while saving, so it was not replaced. Save \
             again to choose what to do",
            self.0
        )
    }
}

impl std::error::Error for ChangedWhileSaving {}

/// What to do before saving over the document's own file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outside {
    /// Save, and expect the file to look like this until it is replaced.
    Go(Option<Seen>),
    SaveAs,
    Cancel,
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    )
}

impl App {
    /// Looks at the file before saving over it, and asks if another program changed it.
    pub(crate) fn check_outside(&mut self, window: Window, path: &Path) -> Outside {
        let own = self
            .path
            .as_deref()
            .is_some_and(|p| crate::files::same_file(p, path));
        let Some(seen) = self.seen.filter(|_| own) else {
            return Outside::Go(None);
        };
        if let Some(same) = still_same_answering(window, path, seen, self.compare) {
            return Outside::Go(Some(same));
        }
        note(Event::new("save", "changed-outside").file(path));
        match (self.ask_changed)(window, &name_of(path)) {
            ChangedChoice::Overwrite => Outside::Go(Seen::of(path, None)),
            ChangedChoice::SaveAs => Outside::SaveAs,
            ChangedChoice::Cancel => Outside::Cancel,
        }
    }
}

#[cfg(test)]
#[path = "outside_tests.rs"]
mod tests;
