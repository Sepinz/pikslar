//! A replaced target that does not read back (B1, section 13, requirement 4): the original is
//! put back from the backup and checked, so a failed save leaves the file as it was. When it
//! cannot be put back, the error names the backup that holds it.

use crate::publish::{Publish, ReadBack, Stamp};
use crate::FormatError;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What a save that did not read back left at its target.
#[derive(Debug)]
pub struct NotKept {
    /// Why the read back failed.
    cause: String,
    /// What is at the target now.
    pub left: Left,
}

/// What is at a target whose replacement did not read back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Left {
    /// The original, put back and read back: the file is as it was before the save.
    AsItWas,
    /// Bytes not known to be right, written by another save or program after this one; the
    /// original, if any file is kept from before, is at the backup.
    Overwritten(Option<PathBuf>),
    /// Nothing: a first save's file was removed, as there was none before.
    Removed,
    /// Bytes not known to be right; the original is kept at this backup name.
    BackupAt(PathBuf),
    /// Bytes not known to be right; the original is not at its backup name, so nothing was
    /// put back and the target was not removed.
    Unknown,
}

impl NotKept {
    pub fn new(cause: String, left: Left) -> NotKept {
        NotKept { cause, left }
    }

    /// The backup this save left, if its original or its own bytes are kept there.
    pub fn backup(&self) -> Option<&Path> {
        match &self.left {
            Left::BackupAt(b) | Left::Overwritten(Some(b)) => Some(b),
            _ => None,
        }
    }

    /// True if the target no longer holds what it held before the save.
    pub fn target_changed(&self) -> bool {
        matches!(self.left, Left::BackupAt(_) | Left::Unknown)
    }

    /// True if the target no longer holds what it held before the save, whoever wrote it:
    /// work that was saved there is not there any more.
    pub fn original_gone(&self) -> bool {
        self.target_changed() || matches!(self.left, Left::Overwritten(_))
    }
}

impl std::fmt::Display for NotKept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cause = &self.cause;
        write!(f, "the saved file did not read back correctly ({cause}), ")?;
        match &self.left {
            Left::AsItWas => write!(
                f,
                "so the file was put back as it was before this save. Nothing was saved: try \
                 again, or save somewhere else"
            ),
            Left::Removed => write!(
                f,
                "so it was removed. Nothing was saved: try again, or save somewhere else"
            ),
            Left::BackupAt(backup) => write!(
                f,
                "and the file from before this save could not be put back. It is kept at {}: \
                 copy it somewhere safe before saving again",
                backup.display()
            ),
            Left::Overwritten(kept) => {
                write!(
                    f,
                    "and the file was written by another program or another save after this one, \
                     so nothing was put back over it. Check that file before relying on it"
                )?;
                match kept {
                    Some(b) => write!(f, ". The file this save replaced or left is kept at {}", b.display()),
                    None => Ok(()),
                }
            }
            Left::Unknown => write!(
                f,
                "and the file from before this save is not where it was kept, so nothing was put back \
                 or removed. Save somewhere else, and do not rely on this file"
            ),
        }
    }
}

impl std::error::Error for NotKept {}

/// The [`NotKept`] inside `e`, if a read back failed.
pub fn not_kept(e: &FormatError) -> Option<&NotKept> {
    let FormatError::Io(io) = e else {
        return None;
    };
    io.get_ref()?.downcast_ref::<NotKept>()
}

/// The target did not read back: puts the original back, or removes a first save's file,
/// and says what is there now.
pub(crate) fn undo_publish(
    publish: &mut dyn Publish,
    target: &Path,
    existed: bool,
    backup: Option<&Path>,
    mine: Option<Stamp>,
    cause: String,
) -> FormatError {
    let left = if written_since(target, mine) {
        // Another save or program wrote the target after this one: never rename or delete
        // over what it wrote.
        Left::Overwritten(backup.map(Path::to_path_buf))
    } else {
        match (backup, existed) {
            (Some(backup), _) => put_back(publish, target, backup),
            // An existing project with no original to put back is never deleted.
            (None, true) => Left::Unknown,
            (None, false) => match fs::remove_file(target) {
                Ok(()) => Left::Removed,
                Err(e) if e.kind() == io::ErrorKind::NotFound => Left::Removed,
                Err(_) => Left::Unknown,
            },
        }
    };
    io::Error::new(io::ErrorKind::InvalidData, NotKept { cause, left }).into()
}

/// True if `target` no longer looks as it did when this save put it there.
fn written_since(target: &Path, mine: Option<Stamp>) -> bool {
    let Some(mine) = mine else {
        return false;
    };
    match fs::metadata(target).and_then(|m| Ok((m.modified()?, m.len()))) {
        Ok((modified, len)) => Stamp { modified, len } != mine,
        Err(e) => e.kind() != io::ErrorKind::NotFound,
    }
}

/// Moves the original back from `backup` over `target` and checks it holds what the backup
/// held. Anything short of that leaves the backup where it is, named.
fn put_back(publish: &mut dyn Publish, target: &Path, backup: &Path) -> Left {
    let kept = Left::BackupAt(backup.to_path_buf());
    let Ok(original) = fs::read(backup) else {
        // Unreadable is still kept; gone since it was seen is not.
        return match backup.exists() {
            true => kept,
            false => Left::Unknown,
        };
    };
    if publish.restore(backup, target).is_err() {
        return match backup.exists() {
            true => kept,
            false => Left::Unknown,
        };
    }
    match publish.read_back(target, &original) {
        Ok(ReadBack::Matches(_)) => Left::AsItWas,
        // The original's bytes are still in memory: a last chance to keep them.
        _ => match fs::write(backup, &original) {
            Ok(()) => kept,
            Err(_) => Left::Unknown,
        },
    }
}
