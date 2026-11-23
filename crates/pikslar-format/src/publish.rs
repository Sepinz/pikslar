//! Putting a checked temporary file in its target's place (B1, section 13, requirement 4).
//!
//! [`write_atomic_with`] writes and checks a temporary file of its own, then hands it to a
//! [`Publish`]: the program gives one that replaces an existing target the way Windows keeps
//! what the original was (`ReplaceFileW`, leaving the original at the backup name) and moves
//! a first save into place without ever replacing a file. The target is read back, and only
//! then is the backup removed; a target that does not read back gets its original back
//! (`publish_back.rs`). A target another program holds is tried again for about two seconds
//! before the save says so (`publish_retry.rs`).
//!
//! A save holds a lock file beside the target while it replaces and reads back, so two saves
//! of one file never interleave (`publish_lock.rs`).
//!
//! Each save's temporary file has a name of its own, made new, so no save writes or publishes
//! another's. One left by a save
//! that was stopped is removed by a later save once it is ten minutes old.
//!
//! What is on disk if the program stops at any moment: before the replace, the old file is at
//! the target; after it, the new file is at the target and the old one at the backup name;
//! after the backup goes, only the new file. Never neither.

use crate::atomic::{
    refuse_read_only, run_temp_path, write_checked_new, StepCheck, RUN_TAG_LEN, TEMP_SUFFIX,
};
use crate::publish_back::undo_publish;
use crate::publish_lock::acquire;
use crate::publish_retry::{may_not_replace, retrying};
use crate::FormatError;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub use crate::publish_retry::InUse;

/// Added to a project's name for the original while its replacement takes its place.
pub const BACKUP_SUFFIX: &str = ".pikslar-bak";
/// A temporary file this old is left from a save that stopped, not one under way.
const STALE_TEMP: Duration = Duration::from_secs(10 * 60);
const ERROR_HANDLE_DISK_FULL: i32 = 39;
const ERROR_DISK_FULL: i32 = 112;

/// Where the original waits while its replacement takes its place.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(BACKUP_SUFFIX);
    PathBuf::from(name)
}

/// A step of putting the file in place, at which a test can stop the save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Published {
    /// The new file is at the target; the original, if there was one, at the backup name.
    Replaced,
    /// The target read back correctly; the backup is about to go.
    ReadBack,
}

/// How a file looked: its write time and its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub modified: SystemTime,
    pub len: u64,
}

/// What reading a target back found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadBack {
    /// It does not hold the bytes written.
    Differs,
    /// It holds them, with how it looked as they were read: none if that changed meanwhile.
    Matches(Option<Stamp>),
}

/// Reads `path` back through one handle, and gives its stamp as of the read, unless the file
/// changed while it was read.
pub fn read_back_stamped(path: &Path, bytes: &[u8]) -> io::Result<ReadBack> {
    let mut file = fs::File::open(path)?;
    let stamp = |file: &fs::File| -> io::Result<Stamp> {
        let meta = file.metadata()?;
        Ok(Stamp {
            modified: meta.modified()?,
            len: meta.len(),
        })
    };
    let before = stamp(&file)?;
    if !crate::atomic::matches_file(&mut file, before.len, bytes)? {
        return Ok(ReadBack::Differs);
    }
    let after = stamp(&file)?;
    Ok(ReadBack::Matches((after == before).then_some(before)))
}

/// A save in place.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Placed {
    /// How the file looked as it read back, if it held still meanwhile.
    pub stamp: Option<Stamp>,
    /// The backup of the original, which could not be removed afterwards and is still there.
    pub backup_left: Option<PathBuf>,
}

/// How a checked temporary file takes its target's place.
pub trait Publish {
    /// `temp` takes the place of the existing `target`, and the original goes to `backup`.
    fn replace(&mut self, target: &Path, temp: &Path, backup: &Path) -> io::Result<()>;
    /// `temp` becomes `target`, which does not exist. Never replaces a file there.
    fn place_new(&mut self, temp: &Path, target: &Path) -> io::Result<()>;
    /// Called at each step; an error stops the save there.
    fn reached(&mut self, _step: Published, _target: &Path) -> io::Result<()> {
        Ok(())
    }
    /// Reads `target` back, to see whether it holds exactly `bytes`.
    fn read_back(&mut self, target: &Path, bytes: &[u8]) -> io::Result<ReadBack> {
        read_back_stamped(target, bytes)
    }
    /// Puts the original, kept at `backup`, back at `target`, over a file that did not read
    /// back.
    fn restore(&mut self, backup: &Path, target: &Path) -> io::Result<()> {
        fs::rename(backup, target)
    }
    /// After access denied: true if this account may not replace `target` at all, rather
    /// than another program holding it.
    fn denied(&mut self, target: &Path) -> bool {
        may_not_replace(target)
    }
    /// Removes the backup once the new file read back.
    fn remove_backup(&mut self, backup: &Path) -> io::Result<()> {
        match fs::remove_file(backup) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            done => done,
        }
    }
}

/// Renames, as the standard library does: the original is gone rather than kept as a backup.
pub struct Rename;

impl Publish for Rename {
    fn replace(&mut self, target: &Path, temp: &Path, _backup: &Path) -> io::Result<()> {
        fs::rename(temp, target)
    }

    fn place_new(&mut self, temp: &Path, target: &Path) -> io::Result<()> {
        fs::rename(temp, target)
    }
}

/// Eight hex digits no other save is likely to have: from the time, the process and a count.
fn save_tag() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SAVES: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let seed = format!(
        "{now}/{}/{}",
        std::process::id(),
        SAVES.fetch_add(1, Ordering::Relaxed)
    );
    let hash = seed.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{:0width$x}", hash >> 32, width = RUN_TAG_LEN)
}

/// True for the name of a temporary file a save of `name` makes, or made before names were
/// a save's own.
fn is_save_temp(file: &str, name: &str) -> bool {
    let Some(rest) = file.strip_prefix(name) else {
        return false;
    };
    let Some(tag) = rest.strip_suffix(TEMP_SUFFIX) else {
        return false;
    };
    tag.is_empty()
        || tag
            .strip_prefix('.')
            .is_some_and(|t| t.len() == RUN_TAG_LEN && t.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Removes temporary files of saves of `path` that stopped long ago. One that may belong to
/// a save under way is never touched.
fn remove_stale_temps(path: &Path) {
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let name = name.to_string_lossy();
    let folder = if folder.as_os_str().is_empty() {
        Path::new(".")
    } else {
        folder
    };
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age >= STALE_TEMP);
        if stale && entry.path().is_file() && is_save_temp(&file.to_string_lossy(), &name) {
            // Left by a stopped save; one still in use stays.
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// `e`, said as a full disk when it is one.
fn disk_full_said(e: io::Error) -> io::Error {
    match e.raw_os_error() {
        Some(ERROR_DISK_FULL | ERROR_HANDLE_DISK_FULL) => io::Error::new(
            e.kind(),
            format!(
                "the disk is full ({e}). The file was left as it was: free some space, or \
                 save somewhere else"
            ),
        ),
        _ => e,
    }
}

/// [`crate::atomic::write_atomic_checked`], putting the file in place through `publish`.
pub fn write_atomic_with(
    path: &Path,
    bytes: &[u8],
    at: StepCheck<'_>,
    publish: &mut dyn Publish,
) -> Result<Placed, FormatError> {
    refuse_read_only(path)?;
    let temp = run_temp_path(path, &save_tag());
    crate::room::check_room_beside(path, &temp)?;
    remove_stale_temps(path);
    write_checked_new(&temp, bytes, at).map_err(disk_full_said)?;
    let _lock = match acquire(path) {
        Ok(lock) => lock,
        Err(e) => {
            let _ = fs::remove_file(&temp);
            return Err(e.into());
        }
    };
    let backup = backup_path(path);
    let existed = fs::symlink_metadata(path).is_ok();
    let backup_before = backup.exists();
    let placed = match existed {
        true => retrying(publish, path, |p| p.replace(path, &temp, &backup)),
        false => retrying(publish, path, |p| p.place_new(&temp, path)),
    };
    if let Err(e) = placed {
        // The save already failed, and the temporary file is this save's own.
        let _ = fs::remove_file(&temp);
        return Err(with_backup_named(e, path, &backup, backup_before).into());
    }
    let mine = stamp_of(path);
    publish.reached(Published::Replaced, path)?;
    let (stamp, cause) = match publish.read_back(path, bytes) {
        Ok(ReadBack::Matches(stamp)) => (stamp, None),
        Ok(ReadBack::Differs) => (None, Some("its bytes differ from what was written".into())),
        Err(e) => (None, Some(e.to_string())),
    };
    if let Some(cause) = cause {
        let original = (existed && backup.exists()).then_some(backup.as_path());
        return Err(undo_publish(publish, path, existed, original, mine, cause));
    }
    publish.reached(Published::ReadBack, path)?;
    let backup_left = match publish.remove_backup(&backup) {
        Err(_) if backup.exists() => Some(backup),
        _ => None,
    };
    Ok(Placed { stamp, backup_left })
}

/// How `path` looks now, if it can be looked at.
fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some(Stamp {
        modified: meta.modified().ok()?,
        len: meta.len(),
    })
}

/// `e`, saying where the original is if a failed replace left it only at the backup name, or
/// naming a backup already there, which can keep a replace out.
fn with_backup_named(e: io::Error, path: &Path, backup: &Path, backup_before: bool) -> io::Error {
    let shown = backup.display();
    let why = if !path.exists() && backup.exists() {
        format!("{e}. The file before this save is at {shown}")
    } else if backup_before && backup.exists() {
        format!(
            "{e}. {shown} was already there, from an earlier save; if another program has it \
             open, close it there, or move it away, and save again"
        )
    } else {
        return e;
    };
    io::Error::new(e.kind(), why)
}

#[cfg(test)]
#[path = "publish_tests.rs"]
mod tests;
