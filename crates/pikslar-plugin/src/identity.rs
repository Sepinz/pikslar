//! Which file an opened handle is, and where it really is.
//!
//! A write checks names it was given against what is on the disk: whether the file at the
//! chosen place is still the original (its volume and file index, which a rename keeps), and
//! which folder a held handle really is (its final path, with every link and junction on the
//! way resolved), so a folder Pikslar keeps its own files in cannot be reached by another
//! name.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// One file on one volume: the same while it is renamed or moved within the volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileId {
    volume: u64,
    index: u64,
}

/// What is at a path now, by identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Found {
    Absent,
    Is(FileId),
    /// Something is there, or may be, but it could not be told which file it is.
    Unknown,
}

/// What is at `path` now, without following a link.
pub(crate) fn found(path: &Path) -> Found {
    match crate::save::open_itself(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Found::Absent,
        Err(_) => Found::Unknown,
        Ok(file) => file_id(&file).map_or(Found::Unknown, Found::Is),
    }
}

#[cfg(windows)]
mod system {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Default)]
    pub struct ByHandleFileInformation {
        pub attributes: u32,
        pub times: [u32; 6],
        pub volume_serial: u32,
        pub size_high: u32,
        pub size_low: u32,
        pub links: u32,
        pub index_high: u32,
        pub index_low: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetFileInformationByHandle(
            file: *mut c_void,
            info: *mut ByHandleFileInformation,
        ) -> i32;
        pub fn GetFinalPathNameByHandleW(
            file: *mut c_void,
            path: *mut u16,
            len: u32,
            flags: u32,
        ) -> u32;
    }
}

/// The identity of the opened file.
#[cfg(windows)]
pub(crate) fn file_id(file: &File) -> io::Result<FileId> {
    use std::os::windows::io::AsRawHandle;
    let mut info = system::ByHandleFileInformation::default();
    // SAFETY: the handle is open while `file` is borrowed, and `info` is a writable
    // BY_HANDLE_FILE_INFORMATION.
    let ok = unsafe { system::GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    match ok {
        0 => Err(io::Error::last_os_error()),
        _ => Ok(FileId {
            volume: u64::from(info.volume_serial),
            index: (u64::from(info.index_high) << 32) | u64::from(info.index_low),
        }),
    }
}

#[cfg(not(windows))]
pub(crate) fn file_id(file: &File) -> io::Result<FileId> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata()?;
    Ok(FileId {
        volume: meta.dev(),
        index: meta.ino(),
    })
}

/// Where the opened file or folder really is: every link, junction and short name on the way
/// resolved, as `\\?\C:\...` or `\\?\UNC\...`. `_path` is the name it was opened by.
#[cfg(windows)]
pub(crate) fn final_path(file: &File, _path: &Path) -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    let mut buffer = vec![0u16; 512];
    loop {
        let len = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: the handle is open while `file` is borrowed, and `buffer` holds `len`
        // writable u16s. Flags 0: the normalized name with a drive letter.
        let got = unsafe {
            system::GetFinalPathNameByHandleW(file.as_raw_handle(), buffer.as_mut_ptr(), len, 0)
        };
        let got = usize::try_from(got).unwrap_or(usize::MAX);
        match got {
            0 => return Err(io::Error::last_os_error()),
            n if n < buffer.len() => {
                buffer.truncate(n);
                return Ok(PathBuf::from(std::ffi::OsString::from_wide(&buffer)));
            }
            // Too small: `n` is the length needed, with its terminating NUL.
            n if n <= 32 * 1024 => buffer.resize(n, 0),
            _ => return Err(io::Error::other("a path longer than Windows allows")),
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn final_path(_file: &File, path: &Path) -> io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

/// True when `inner` is `outer` or inside it, both being final paths. Names are compared
/// without case, as Windows does.
pub(crate) fn is_within(inner: &Path, outer: &Path) -> bool {
    let fold = |p: &Path| -> Vec<String> {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    let (inner, outer) = (fold(inner), fold(outer));
    !outer.is_empty() && inner.len() >= outer.len() && inner[..outer.len()] == outer[..]
}

/// The final path of an existing folder, by its name, or `None` when there is no such folder
/// (then nothing can be inside it).
/// Links on the way, and the folder itself being one, are followed: what counts is where its
/// files really are.
pub(crate) fn final_folder(path: &Path) -> io::Result<Option<PathBuf>> {
    match std::fs::canonicalize(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        other => other.map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_file_keeps_its_identity_when_renamed_and_another_file_differs() {
        let dir = std::env::temp_dir().join(format!("pikslar-identity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a folder");
        fs::write(dir.join("a.txt"), "a").expect("written");
        fs::write(dir.join("b.txt"), "b").expect("written");
        let a = found(&dir.join("a.txt"));
        assert!(matches!(a, Found::Is(_)), "{a:?}");
        assert_ne!(a, found(&dir.join("b.txt")));
        fs::rename(dir.join("a.txt"), dir.join("c.txt")).expect("renamed");
        assert_eq!(found(&dir.join("c.txt")), a);
        assert_eq!(found(&dir.join("a.txt")), Found::Absent);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_folder_is_within_itself_and_its_parents_only() {
        let p = |s: &str| PathBuf::from(s);
        assert!(is_within(&p(r"C:\A\Pikslar"), &p(r"c:\a\pikslar")));
        assert!(is_within(
            &p(r"C:\A\Pikslar\plugins\x"),
            &p(r"C:\A\Pikslar")
        ));
        assert!(!is_within(&p(r"C:\A\Pikslar2"), &p(r"C:\A\Pikslar")));
        assert!(!is_within(&p(r"C:\A"), &p(r"C:\A\Pikslar")));
        assert!(!is_within(&p(r"C:\A"), &p("")));
    }

    #[test]
    fn a_final_path_resolves_a_junction_on_the_way() {
        let dir = std::env::temp_dir().join(format!("pikslar-final-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("real").join("inner")).expect("a folder");
        let real = final_folder(&dir.join("real"))
            .expect("read")
            .expect("there");
        assert_eq!(final_folder(&dir.join("missing")).expect("read"), None);
        #[cfg(windows)]
        {
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(dir.join("link"))
                .arg(dir.join("real"))
                .stdout(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if made {
                let inner = final_folder(&dir.join("link").join("inner"))
                    .expect("read")
                    .expect("there");
                assert!(is_within(&inner, &real), "{inner:?} {real:?}");
            }
        }
        let _ = fs::remove_dir_all(dir);
    }
}
