//! Paths for Windows' own file calls, at any length.
//!
//! `ReplaceFileW` and `MoveFileExW` take a plain path only while it is shorter than the old
//! limit of 260 characters (`MAX_PATH`), unless the person turned on long paths for the whole
//! system. Past it they fail with "the system cannot find the path specified" (error 3), which
//! is how a save to a deep folder failed (`docs/b1-unattended-report.md`). In the extended form
//! (`\\?\C:\...`, or `\\?\UNC\server\share\...` for a network share) they take up to 32,767.
//!
//! Windows does not tidy an extended path: it is used exactly as given. So the path is first
//! made whole the way Windows itself makes a plain one whole (`GetFullPathNameW`, through
//! [`std::path::absolute`]): from the current folder, with `.` and `..` worked out and `/` read
//! as `\`. The standard library does the same for its own file calls, so the temporary file it
//! writes and the name it is moved to are the same file. Links and junctions are not followed
//! here; that stays the file system's to do, as with a plain path.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

/// The start of an extended path to a drive.
const VERBATIM: &str = r"\\?\";
/// The start of an extended path to a network share.
const VERBATIM_UNC: &str = r"\\?\UNC\";

/// How a whole path begins.
#[derive(Debug, PartialEq, Eq)]
enum Start {
    /// `\\?\...` or `\\.\...`: already as Windows takes it, at any length.
    AsIs,
    /// `\\server\share\...`.
    Share,
    /// `C:\...`.
    Drive,
    /// Anything else, given to Windows unchanged.
    Other,
}

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn start(path: &[u16]) -> Start {
    let slash = |c: u16| c == u16::from(b'\\') || c == u16::from(b'/');
    let at = |i: usize| path.get(i).copied().unwrap_or(0);
    let letter = char::from_u32(u32::from(at(0))).is_some_and(|c| c.is_ascii_alphabetic());
    let marked = at(2) == u16::from(b'?') || at(2) == u16::from(b'.');
    if slash(at(0)) && slash(at(1)) && marked && slash(at(3)) {
        return Start::AsIs;
    }
    if slash(at(0)) && slash(at(1)) {
        return Start::Share;
    }
    if letter && at(1) == u16::from(b':') && slash(at(2)) {
        return Start::Drive;
    }
    Start::Other
}

/// `path` as a NUL-terminated name Windows' file calls take at any length: whole, and in the
/// extended form when it is on a drive or a share.
pub(crate) fn extended(path: &Path) -> io::Result<Vec<u16>> {
    let whole = std::path::absolute(path)?;
    let name: Vec<u16> = whole.as_os_str().encode_wide().collect();
    if name.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a file name cannot hold a NUL character",
        ));
    }
    let mut out = Vec::with_capacity(name.len() + VERBATIM_UNC.len() + 1);
    match start(&name) {
        Start::Share => {
            out.extend(units(VERBATIM_UNC));
            out.extend_from_slice(&name[2..]);
        }
        Start::Drive => {
            out.extend(units(VERBATIM));
            out.extend_from_slice(&name);
        }
        Start::AsIs | Start::Other => out.extend_from_slice(&name),
    }
    out.push(0);
    Ok(out)
}

#[cfg(test)]
#[path = "long_path_tests.rs"]
mod tests;
