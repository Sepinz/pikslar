//! Replacing a project saved by an older version, keeping the original beside it.
//!
//! The rules are in `docs/format-v3.md`. The original's bytes are copied to a backup that is
//! never overwritten, and the project is replaced only if it still holds exactly those bytes.
//! A failure at any step leaves the project as it was.

use crate::atomic::{matches_on_disk, SaveStep, TEMP_SUFFIX};
use crate::publish::{write_atomic_with, Placed, Publish, Rename};
use crate::{piks, read_limited, FormatError, MAX_FILE_BYTES};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// A moment during an upgrade at which a test can make things go wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpgradeStep {
    /// The original has been read.
    Read,
    /// The backup holds the original and has been read back.
    BackedUp,
    /// A step of the atomic save that replaces the project.
    Save(SaveStep),
}

/// Called at each step. An error abandons the upgrade.
pub type UpgradeCheck<'a> = &'a mut dyn FnMut(UpgradeStep) -> io::Result<()>;

#[derive(Debug)]
pub enum UpgradeError {
    /// Something else already has the backup's name, so it is kept and no upgrade happens.
    BackupInTheWay(PathBuf),
    /// The project changed on disk during the upgrade, and was not replaced.
    Changed,
    Failed(FormatError),
}

impl fmt::Display for UpgradeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpgradeError::BackupInTheWay(path) => write!(
                f,
                "{} already exists and holds something else, so the project was left as it was",
                file_name(path)
            ),
            UpgradeError::Changed => {
                f.write_str("The project changed on disk while saving, so it was left as it was")
            }
            UpgradeError::Failed(e) => write!(f, "{e}. The project was left as it was"),
        }
    }
}

impl From<io::Error> for UpgradeError {
    fn from(e: io::Error) -> Self {
        UpgradeError::Failed(FormatError::Io(e))
    }
}

impl From<FormatError> for UpgradeError {
    fn from(e: FormatError) -> Self {
        UpgradeError::Failed(e)
    }
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// The version of the project at `path`, if it is one older than what this build writes.
/// Anything that is not a readable project header is not an older project.
pub fn older_version(path: &Path) -> Option<u32> {
    let mut header = [0u8; 8];
    File::open(path).ok()?.read_exact(&mut header).ok()?;
    let version = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    (&header[..4] == piks::MAGIC && (1..piks::WRITTEN_VERSION).contains(&version))
        .then_some(version)
}

/// Where the original of a project of this version is kept: its name plus `.v2.bak`.
pub fn backup_path(path: &Path, version: u32) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".v{version}.bak"));
    PathBuf::from(name)
}

/// True if something already has the backup's name and does not hold exactly the project as it
/// is now, so an upgrade would have nowhere to keep the original.
pub fn backup_in_the_way(path: &Path, version: u32) -> bool {
    let backup = backup_path(path, version);
    match fs::symlink_metadata(&backup) {
        Err(_) => return false,
        Ok(meta) if meta.file_type().is_symlink() => return true,
        Ok(_) => {}
    }
    match read_limited(path, MAX_FILE_BYTES) {
        Ok(original) => !matches_on_disk(&backup, &original).unwrap_or(false),
        Err(_) => true,
    }
}

/// Writes the backup under a temporary name, then links it in under its own name, which
/// fails rather than replaces if something took that name meanwhile. Where links are not
/// possible, it is created directly under its name, never replacing anything.
fn write_backup(backup: &Path, original: &[u8]) -> io::Result<()> {
    let mut temp = backup.as_os_str().to_owned();
    temp.push(TEMP_SUFFIX);
    let temp = PathBuf::from(temp);
    // One left by a kill may be another name for some file, so it is unlinked, never written.
    let _ = fs::remove_file(&temp);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    write_synced(&mut file, original)?;
    drop(file);
    let linked = check(&temp, original).and_then(|()| fs::hard_link(&temp, backup));
    // The backup is in place or the upgrade is abandoned. Either way the temporary copy is spare.
    let _ = fs::remove_file(&temp);
    match linked {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists && !backup.exists() => {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(backup)?;
            let written = write_synced(&mut file, original).and_then(|()| check(backup, original));
            if written.is_err() {
                drop(file);
                // It was made here a moment ago and holds nothing anyone has seen.
                let _ = fs::remove_file(backup);
            }
            written
        }
        other => other,
    }
}

fn write_synced(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

fn check(path: &Path, expected: &[u8]) -> io::Result<()> {
    if matches_on_disk(path, expected)? {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the backup did not read back correctly",
        ))
    }
}

/// [`upgrade`], stopping at each step to ask whether to go on.
pub fn upgrade_checked(
    path: &Path,
    version: u32,
    bytes: &[u8],
    at: UpgradeCheck<'_>,
) -> Result<(), UpgradeError> {
    upgrade_with(path, version, bytes, at, &mut Rename).map(|_| ())
}

/// [`upgrade_checked`], putting the new project in place with `publish`, as any save does
/// (the version backup is made first, and is a file of its own). Gives how it was placed.
pub fn upgrade_with(
    path: &Path,
    version: u32,
    bytes: &[u8],
    at: UpgradeCheck<'_>,
    publish: &mut dyn Publish,
) -> Result<Placed, UpgradeError> {
    let original = read_limited(path, MAX_FILE_BYTES)?;
    if older_version(path) != Some(version) {
        return Err(UpgradeError::Changed);
    }
    at(UpgradeStep::Read)?;
    let backup = backup_path(path, version);
    if let Ok(meta) = fs::symlink_metadata(&backup) {
        // A link could lead back to the project itself, which is about to be replaced.
        if meta.file_type().is_symlink() || !matches_on_disk(&backup, &original).unwrap_or(false) {
            return Err(UpgradeError::BackupInTheWay(backup));
        }
    } else {
        write_backup(&backup, &original)?;
    }
    at(UpgradeStep::BackedUp)?;
    let mut changed = false;
    let mut step = |step, _: &Path| {
        at(UpgradeStep::Save(step))?;
        if step == SaveStep::Verified && !matches_on_disk(path, &original)? {
            changed = true;
            return Err(io::Error::other("the project changed"));
        }
        Ok(())
    };
    let saved = write_atomic_with(path, bytes, &mut step, publish);
    match saved {
        Err(_) if changed => Err(UpgradeError::Changed),
        other => Ok(other?),
    }
}

/// Keeps the project at `path`, of an older `version`, as its backup, then replaces it with
/// `bytes`.
pub fn upgrade(path: &Path, version: u32, bytes: &[u8]) -> Result<(), UpgradeError> {
    upgrade_checked(path, version, bytes, &mut |_| Ok(()))
}

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod tests;
