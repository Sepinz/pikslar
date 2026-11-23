//! Putting a finished file in place of another, the way Windows keeps what the original was.
//!
//! `ReplaceFileW` gives the new file the original's permissions (its DACL), attributes,
//! creation time, named streams and short name, and moves the original to a backup name, so
//! it can be put back. A plain rename over the original would give the new file its
//! folder's inherited permissions instead and drop the original's streams.

use std::io;
use std::path::Path;
use std::ptr::null_mut;

const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
/// The new file could not take the original's name; the original is now at the backup name.
const ERROR_UNABLE_TO_MOVE_REPLACEMENT_2: i32 = 1177;

#[link(name = "kernel32")]
extern "system" {
    fn ReplaceFileW(
        replaced: *const u16,
        replacement: *const u16,
        backup: *const u16,
        flags: u32,
        exclude: *mut core::ffi::c_void,
        reserved: *mut core::ffi::c_void,
    ) -> i32;
    fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
}

/// Every name goes to Windows whole and in the extended form, so a path past the old 260
/// character limit is replaced like any other (`long_path.rs`).
fn wide(path: &Path) -> io::Result<Vec<u16>> {
    crate::long_path::extended(path)
}

fn checked(ok: i32) -> io::Result<()> {
    match ok {
        0 => Err(io::Error::last_os_error()),
        _ => Ok(()),
    }
}

/// Puts `replacement` where `target` is, keeping the original's permissions and streams, and
/// leaving the original at `backup` (replaced if it exists). On an error the one case where
/// Windows leaves the original at the backup name is undone here if it can be; when that move
/// back fails too, the original stays at `backup` and the first error is returned. Callers
/// must look at what is at `target` before removing `backup`.
pub fn replace_file(target: &Path, replacement: &Path, backup: &Path) -> io::Result<()> {
    let (t, r, b) = (wide(target)?, wide(replacement)?, wide(backup)?);
    // SAFETY: the three names are NUL-terminated and outlive the call; the two optional
    // pointers are null as the call allows.
    let ok = unsafe {
        ReplaceFileW(
            t.as_ptr(),
            r.as_ptr(),
            b.as_ptr(),
            0,
            null_mut(),
            null_mut(),
        )
    };
    let Err(e) = checked(ok) else {
        return Ok(());
    };
    if e.raw_os_error() == Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT_2) {
        // A failure here leaves the original at `backup`, which the caller finds and keeps.
        let _ = move_over(backup, target);
    }
    Err(e)
}

/// Moves `from` to `to`, which must not exist: never replaces a file there.
pub fn move_new(from: &Path, to: &Path) -> io::Result<()> {
    let (f, t) = (wide(from)?, wide(to)?);
    // SAFETY: both names are NUL-terminated and outlive the call.
    checked(unsafe { MoveFileExW(f.as_ptr(), t.as_ptr(), MOVEFILE_WRITE_THROUGH) })
}

/// Moves `from` over `to`, replacing it.
pub fn move_over(from: &Path, to: &Path) -> io::Result<()> {
    let (f, t) = (wide(from)?, wide(to)?);
    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
    // SAFETY: both names are NUL-terminated and outlive the call.
    checked(unsafe { MoveFileExW(f.as_ptr(), t.as_ptr(), flags) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn folder(label: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pikslar-replace-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a folder");
        dir
    }

    #[test]
    fn a_replaced_file_keeps_its_streams_and_the_original_goes_to_the_backup() {
        let dir = folder("streams");
        let (target, fresh, backup) = (dir.join("a.txt"), dir.join("a.tmp"), dir.join("a.bak"));
        fs::write(&target, "old").expect("written");
        // A named stream: what a rename over the file would lose.
        let stream = dir.join("a.txt:note");
        fs::write(&stream, "kept").expect("a stream");
        fs::write(&fresh, "new").expect("written");
        fs::write(&backup, "").expect("reserved");
        replace_file(&target, &fresh, &backup).expect("replaced");
        assert_eq!(fs::read(&target).expect("read"), b"new");
        assert_eq!(fs::read(&stream).expect("the stream"), b"kept");
        assert_eq!(fs::read(&backup).expect("read"), b"old");
        assert!(!fresh.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_new_file_never_replaces_one_and_a_backup_can_be_put_back() {
        let dir = folder("new");
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        fs::write(&a, "a").expect("written");
        fs::write(&b, "b").expect("written");
        assert!(move_new(&a, &b).is_err());
        assert_eq!(fs::read(&b).expect("read"), b"b");
        move_over(&a, &b).expect("moved");
        assert_eq!(fs::read(&b).expect("read"), b"a");
        assert!(!a.exists());
        move_new(&b, &a).expect("moved");
        assert_eq!(fs::read(&a).expect("read"), b"a");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_replace_leaves_the_original() {
        let dir = folder("fail");
        let (target, backup) = (dir.join("a.txt"), dir.join("a.bak"));
        fs::write(&target, "old").expect("written");
        assert!(replace_file(&target, &dir.join("missing.tmp"), &backup).is_err());
        assert_eq!(fs::read(&target).expect("read"), b"old");
        let _ = fs::remove_dir_all(dir);
    }
}
