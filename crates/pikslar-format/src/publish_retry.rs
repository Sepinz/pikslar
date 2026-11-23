//! A target another program holds while a save replaces it: tried again for about two
//! seconds, then said for what it is (B1, section 13, requirement 4).
//!
//! Windows error 32 (sharing violation), 33 (part of the file locked) and 1224 (the file is
//! mapped into memory) all mean another program has the file, and are tried again. Error 5
//! (access denied) means that too when a program holds the file without letting it be
//! deleted, but also that this account may not replace it: a look at the file tells them
//! apart, and a denial is said at once, as a denial.

use crate::publish::Publish;
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

/// How long a publish waits between tries while another program holds the target. About two
/// seconds in all.
const RETRY_PAUSES_MS: [u64; 8] = [25, 50, 100, 200, 400, 400, 400, 400];
pub(crate) const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;
const ERROR_USER_MAPPED_FILE: i32 = 1224;
/// The access a replace needs: to delete the file and to write it.
#[cfg(windows)]
const DELETE_AND_WRITE: u32 = 0x0001_0000 | 0x0002;

/// Another program had the target for the whole of the retries, or this account may not
/// replace it.
#[derive(Debug)]
pub struct InUse {
    /// The Windows error the last try gave: 5, 32, 33 or 1224.
    pub code: i32,
    /// A look at the file found this account may not replace it: no other program is to blame.
    pub denied: bool,
    name: String,
}

impl std::fmt::Display for InUse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (name, code) = (&self.name, self.code);
        if self.denied {
            return write!(
                f,
                "permission denied: this account may not replace {name} (Windows error {code}). \
                 Save it somewhere else, or ask whoever manages the file for access"
            );
        }
        match code {
            ERROR_LOCK_VIOLATION => write!(
                f,
                "another program has locked part of {name}, which keeps it from being replaced \
                 (Windows error {code}). Close it there and save again"
            ),
            ERROR_USER_MAPPED_FILE => write!(
                f,
                "{name} is open in another program that maps it into memory, which keeps it \
                 from being replaced (Windows error {code}). Close it there and save again"
            ),
            _ => write!(
                f,
                "{name} is open in another program, which keeps it from being replaced (Windows \
                 error {code}). Close it there, or wait for it to finish, and save again"
            ),
        }
    }
}

impl std::error::Error for InUse {}

/// The code of an error that may mean another program holds the file.
fn held(e: &io::Error) -> Option<i32> {
    e.raw_os_error().filter(|c| {
        [
            ERROR_ACCESS_DENIED,
            ERROR_SHARING_VIOLATION,
            ERROR_LOCK_VIOLATION,
            ERROR_USER_MAPPED_FILE,
        ]
        .contains(c)
    })
}

/// True if opening `target` to delete and write it is refused as access denied: this
/// account may not replace it. A program holding it gives error 32 instead.
#[cfg(windows)]
pub(crate) fn may_not_replace(target: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    let opened = fs::OpenOptions::new()
        .access_mode(DELETE_AND_WRITE)
        .open(target);
    matches!(opened, Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED))
}

/// Elsewhere access denied is a denial.
#[cfg(not(windows))]
pub(crate) fn may_not_replace(_target: &Path) -> bool {
    true
}

fn name_of(target: &Path) -> String {
    target.file_name().map_or_else(
        || target.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// One try. `Ok(None)` is worth trying again, with the error it gave.
fn attempt(
    publish: &mut dyn Publish,
    target: &Path,
    step: &mut dyn FnMut(&mut dyn Publish) -> io::Result<()>,
) -> io::Result<Option<io::Error>> {
    match step(publish) {
        Ok(()) => Ok(None),
        Err(e) => match held(&e) {
            Some(ERROR_ACCESS_DENIED) if publish.denied(target) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                InUse {
                    code: ERROR_ACCESS_DENIED,
                    denied: true,
                    name: name_of(target),
                },
            )),
            Some(_) => Ok(Some(e)),
            None => Err(e),
        },
    }
}

/// Runs `step` until it works, fails for another reason, or the retries are spent.
pub(crate) fn retrying(
    publish: &mut dyn Publish,
    target: &Path,
    mut step: impl FnMut(&mut dyn Publish) -> io::Result<()>,
) -> io::Result<()> {
    let Some(mut last) = attempt(publish, target, &mut step)? else {
        return Ok(());
    };
    for pause in RETRY_PAUSES_MS {
        std::thread::sleep(Duration::from_millis(pause));
        match attempt(publish, target, &mut step)? {
            None => return Ok(()),
            Some(e) => last = e,
        }
    }
    let code = held(&last).unwrap_or(ERROR_SHARING_VIOLATION);
    let in_use = InUse {
        code,
        denied: false,
        name: name_of(target),
    };
    Err(io::Error::new(last.kind(), in_use))
}
