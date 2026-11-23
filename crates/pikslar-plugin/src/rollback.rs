//! Putting a staged write in place, and taking it back, without ever losing the original.
//!
//! A replace that fails may have done nothing, moved the original to its backup name and
//! stopped, or replaced the file and still reported an error. So after any error nothing is
//! assumed: what is at the chosen place is told by its identity ([`crate::identity`]), and
//!
//! - the original still there: nothing to do;
//! - the new file there, or nothing: the original is put back from its backup, if the backup
//!   is the original;
//! - anything else: nothing is touched.
//!
//! A backup is removed only once the place is seen to hold what it should: the original after
//! a rollback, or the new file after a success. Otherwise it is kept beside the file, so the
//! user's bytes are never deleted.

use crate::files::FileError;
use crate::identity::{found, FileId, Found};
use crate::save::{check_target, Disk, Seen};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

/// How far a staged write went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tried {
    /// Not put in place: the chosen place was never touched.
    Not,
    Done,
    /// The replace or move reported an error: anything may have happened.
    Failed,
}

/// A write ready to be put in place.
pub(crate) struct Staged<'a> {
    pub target: &'a Path,
    pub fresh: PathBuf,
    /// The temporary file's identity, which the target has once it is put in place.
    pub fresh_id: FileId,
    /// Made for the original when there is one; holds it once the target is replaced.
    pub backup: Option<PathBuf>,
    /// The original's identity, when there is one.
    pub original: Option<FileId>,
    pub now: Seen,
    pub tried: Tried,
    /// Held open until the end, so the folder stays the one checked.
    pub _folder: File,
}

fn refused(why: &'static str) -> FileError {
    FileError::Refused(why)
}

/// Checks the target once more, by kind and by identity, then puts the write in place.
pub(crate) fn place(s: &mut Staged<'_>, disk: &mut dyn Disk) -> Result<(), FileError> {
    let changed = refused("the chosen place changed while it was being written");
    if check_target(s.target)? != s.now {
        return Err(changed);
    }
    if let Some(original) = s.original {
        if found(s.target) != Found::Is(original) {
            return Err(changed);
        }
    }
    let done = match &s.backup {
        Some(backup) => disk.replace(s.target, &s.fresh, backup),
        None => disk.place_new(&s.fresh, s.target),
    };
    s.tried = match done {
        Ok(()) => Tried::Done,
        Err(_) => Tried::Failed,
    };
    done.map_err(|e| FileError::Io(format!("could not write the file: {}", e.kind())))
}

/// Takes back one write. True when the place is seen to be as it was before, so its backup
/// may go; false when the original could not be put back (its backup is then kept).
fn take_back(s: &Staged<'_>, disk: &mut dyn Disk) -> bool {
    if s.tried == Tried::Not {
        return true;
    }
    let at = found(s.target);
    match (s.original, &s.backup) {
        // A new file: only Pikslar's own is removed.
        (None, _) => match at {
            Found::Is(id) if id == s.fresh_id => fs::remove_file(s.target).is_ok(),
            Found::Absent | Found::Is(_) | Found::Unknown => true,
        },
        (Some(original), Some(backup)) => match at {
            Found::Is(id) if id == original => true,
            Found::Is(id) if id != s.fresh_id => false,
            Found::Unknown => false,
            Found::Absent | Found::Is(_) => {
                found(backup) == Found::Is(original)
                    && disk.put_back(backup, s.target).is_ok()
                    && found(s.target) == Found::Is(original)
            }
        },
        (Some(_), None) => false,
    }
}

/// Removes the temporary file, and the backup when `drop_backup`.
fn clean(s: &Staged<'_>, drop_backup: bool) {
    let _ = fs::remove_file(&s.fresh);
    if let (Some(backup), true) = (&s.backup, drop_backup) {
        let _ = fs::remove_file(backup);
    }
}

/// Undoes every write that was tried, last first, and cleans up after all of them.
pub(crate) fn undo(staged: &[Staged<'_>], disk: &mut dyn Disk) -> Result<(), FileError> {
    let mut all_back = true;
    for s in staged.iter().rev() {
        let back = take_back(s, disk);
        clean(s, back);
        all_back &= back;
    }
    match all_back {
        true => Ok(()),
        false => Err(FileError::NotPutBack),
    }
}

/// After every write is in place: removes each temporary file, and each backup whose place
/// is seen to hold the new file. A backup whose place holds anything else is kept.
pub(crate) fn finish(staged: &[Staged<'_>]) {
    for s in staged {
        clean(s, found(s.target) == Found::Is(s.fresh_id));
    }
}
