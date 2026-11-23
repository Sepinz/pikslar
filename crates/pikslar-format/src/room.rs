//! Whether a file can be saved at a path at all, asked before anything is written.
//!
//! Every save writes a temporary file beside the target, under the target's name with more
//! added, and puts it in place only once it reads back. Windows' file systems allow at most
//! 255 characters in one name and 32,767 in a whole path, the extended start included. A
//! target whose name leaves no room for the temporary file's would get as far as writing and
//! then fail with a message about the wrong thing; this refuses it first, saying what to
//! change, while the original is still untouched.

use crate::FormatError;
use std::ffi::OsStr;
use std::path::Path;

/// The most characters (UTF-16 units) one name in a folder may have.
pub const LONGEST_NAME: usize = 255;
/// The most characters (UTF-16 units) in a whole path in the extended form.
pub const LONGEST_PATH: usize = 32_767;
/// The longest extended start, `\\?\UNC\`, which may replace the two of a share's `\\`.
const START: usize = 8;

fn units(s: &OsStr) -> usize {
    s.to_string_lossy().encode_utf16().count()
}

/// Refuses `path` when a file named as it is with `extra` more characters could not be made:
/// its name or its whole path would be too long.
pub fn check_room(path: &Path, extra: usize) -> Result<(), FormatError> {
    let name = path.file_name().map_or(0, units);
    if name + extra > LONGEST_NAME {
        return Err(FormatError::NameTooLong {
            most: LONGEST_NAME.saturating_sub(extra),
        });
    }
    let whole = std::path::absolute(path)
        .map_or_else(|_| units(path.as_os_str()), |p| units(p.as_os_str()));
    if whole + extra + START >= LONGEST_PATH {
        return Err(FormatError::PathTooLong);
    }
    Ok(())
}

/// [`check_room`] for `path`, with `temp` the name its temporary file will have.
pub fn check_room_beside(path: &Path, temp: &Path) -> Result<(), FormatError> {
    let name = |p: &Path| p.file_name().map_or(0, units);
    check_room(path, name(temp).saturating_sub(name(path)))
}

#[cfg(test)]
#[path = "room_tests.rs"]
mod tests;
