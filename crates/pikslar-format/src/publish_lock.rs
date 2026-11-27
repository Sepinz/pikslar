//! One save of a file at a time (B1, section 13, requirement 4).
//!
//! Two Pikslar windows or processes saving the same file could each replace it and read it
//! back between the other's replace and read back, so that one saw bytes that were not its
//! own. A save therefore holds a lock file beside the project, `name.pikslar-lock`, opened
//! with no sharing and deleted when the handle closes (so a killed program leaves none
//! locked): a second save waits for about two seconds, then says another save is under way.
//! A folder or file system where the lock cannot be made does not stop the save.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a save waits between tries for the lock. About two seconds in all.
const PAUSES_MS: [u64; 8] = [25, 50, 100, 200, 400, 400, 400, 400];
const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_SHARING_VIOLATION: i32 = 32;
#[cfg(windows)]
const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
/// Opens a link itself, never what it points at: closing the lock then deletes the link at
/// most, never a file someone linked there (review finding S2, RC3).
#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// Added to a project's name for its lock file.
pub const LOCK_SUFFIX: &str = ".pikslar-lock";

/// The lock file of `path`.
pub fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(LOCK_SUFFIX);
    PathBuf::from(name)
}

/// Held while a save puts its file in place; the lock file goes when this is dropped.
pub(crate) struct SaveLock(#[allow(dead_code)] Option<File>);

fn open(lock: &Path) -> io::Result<File> {
    // Something other than a plain file at the lock's name is not Pikslar's: it is left alone,
    // and the save goes on without the lock.
    if fs::symlink_metadata(lock).is_ok_and(|m| !m.file_type().is_file()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a lock file",
        ));
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .share_mode(0)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(lock)
}

/// Takes the lock of `path`. An error is only that another save holds it.
pub(crate) fn acquire(path: &Path) -> io::Result<SaveLock> {
    let lock = lock_path(path);
    for pause in PAUSES_MS {
        match open(&lock) {
            Ok(file) => return Ok(SaveLock(Some(file))),
            Err(e) if is_held(&e) => std::thread::sleep(Duration::from_millis(pause)),
            // Somewhere a lock file cannot be made: the save goes on without.
            Err(_) => return Ok(SaveLock(None)),
        }
    }
    match open(&lock) {
        Ok(file) => Ok(SaveLock(Some(file))),
        Err(e) if is_held(&e) => {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!(
                    "another save of {name} is under way, in another window or program. Wait \
                     for it to finish and save again"
                ),
            ))
        }
        Err(_) => Ok(SaveLock(None)),
    }
}

/// Another handle on the lock file, or one just closed whose file is going.
fn is_held(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(ERROR_SHARING_VIOLATION | ERROR_ACCESS_DENIED)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pikslar-plock-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).expect("temp dir");
        d
    }

    #[test]
    fn a_lock_is_gone_once_the_save_lets_it_go() {
        let d = dir("plain");
        let path = d.join("walk.piks");
        let lock = acquire(&path).expect("taken");
        assert!(lock.0.is_some());
        drop(lock);
        assert!(!lock_path(&path).exists());
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_folder_at_the_locks_name_is_kept_and_the_save_goes_on() {
        let d = dir("folder");
        let path = d.join("walk.piks");
        fs::create_dir(lock_path(&path)).expect("folder in the way");
        let lock = acquire(&path).expect("goes on");
        assert!(lock.0.is_none());
        drop(lock);
        assert!(lock_path(&path).is_dir());
        fs::remove_dir_all(d).expect("cleanup");
    }

    /// A link at the lock's name, to someone's file, never has that file deleted. Making a
    /// link needs a Windows setting not every computer has; without it the test says so.
    #[cfg(windows)]
    #[test]
    fn a_link_at_the_locks_name_never_deletes_what_it_points_at() {
        let d = dir("link");
        let path = d.join("walk.piks");
        let theirs = d.join("theirs.txt");
        fs::write(&theirs, b"someone's").expect("their file");
        if let Err(e) = std::os::windows::fs::symlink_file(&theirs, lock_path(&path)) {
            eprintln!("links cannot be made here ({e}); only the folder case was checked");
            fs::remove_dir_all(d).expect("cleanup");
            return;
        }
        drop(acquire(&path).expect("goes on"));
        assert_eq!(fs::read(&theirs).expect("still there"), b"someone's");
        fs::remove_dir_all(d).expect("cleanup");
    }
}
