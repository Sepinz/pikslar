//! Which reparse points redirect somewhere else.
//!
//! A reparse point's tag says what it is. Tags with the name-surrogate bit (bit 29, Windows'
//! `IsReparseTagNameSurrogate`) stand for another name: symbolic links, junctions (mount
//! points), WSL links and the like. Those are refused wherever a plugin's files are read or
//! written, because following one leads somewhere the user did not choose. Other reparse
//! points keep the file where it is: OneDrive's and other cloud providers' placeholders
//! (`IO_REPARSE_TAG_CLOUD_*`), deduplicated files, app execution aliases' neighbours. Those
//! are plain files and folders to the user, and refusing them would refuse a OneDrive-backed
//! Documents folder.
//!
//! The tag is read from an opened handle (`FILE_ATTRIBUTE_TAG_INFO` through
//! `GetFileInformationByHandleEx`), so what is checked is what was opened.

use std::fs::File;
use std::io;

/// The name-surrogate bit of a reparse tag.
const NAME_SURROGATE: u32 = 0x2000_0000;

/// True for a tag that stands for another name: a link, a junction, anything redirecting.
pub(crate) const fn is_name_surrogate(tag: u32) -> bool {
    tag & NAME_SURROGATE != 0
}

/// True when the opened file or folder is a reparse point that redirects (see
/// [`is_name_surrogate`]). A plain file, a plain folder and a cloud placeholder are not.
#[cfg(windows)]
pub(crate) fn redirects(file: &File) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;

    const FILE_ATTRIBUTE_TAG_INFO: i32 = 9;
    const REPARSE_POINT: u32 = 0x400;

    #[repr(C)]
    #[derive(Default)]
    struct AttributeTagInfo {
        attributes: u32,
        reparse_tag: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandleEx(
            file: *mut core::ffi::c_void,
            class: i32,
            info: *mut core::ffi::c_void,
            size: u32,
        ) -> i32;
    }

    let mut info = AttributeTagInfo::default();
    let size = std::mem::size_of::<AttributeTagInfo>() as u32;
    // SAFETY: the handle is open for as long as `file` is borrowed, and `info` is a writable
    // FILE_ATTRIBUTE_TAG_INFO of exactly `size` bytes.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FILE_ATTRIBUTE_TAG_INFO,
            (&mut info as *mut AttributeTagInfo).cast(),
            size,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info.attributes & REPARSE_POINT != 0 && is_name_surrogate(info.reparse_tag))
}

#[cfg(not(windows))]
pub(crate) fn redirects(file: &File) -> io::Result<bool> {
    // A handle opened elsewhere is opened through links; the caller checks the name first.
    Ok(file.metadata()?.file_type().is_symlink())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirecting_tags_are_told_from_ones_that_keep_the_file_in_place() {
        let redirecting = [
            (0xA000_0003, "IO_REPARSE_TAG_MOUNT_POINT (junction)"),
            (0xA000_000C, "IO_REPARSE_TAG_SYMLINK"),
            (0xA000_001D, "IO_REPARSE_TAG_LX_SYMLINK"),
            (0xA000_0019, "IO_REPARSE_TAG_GLOBAL_REPARSE"),
            (0xA000_0010, "IO_REPARSE_TAG_IIS_CACHE"),
            (0xA000_0027, "IO_REPARSE_TAG_WCI_LINK"),
        ];
        let in_place = [
            (0x9000_001A, "IO_REPARSE_TAG_CLOUD"),
            (0x9000_101A, "IO_REPARSE_TAG_CLOUD_1"),
            (0x9000_A01A, "IO_REPARSE_TAG_CLOUD_A"),
            (0x9000_F01A, "IO_REPARSE_TAG_CLOUD_F"),
            (0x8000_0021, "IO_REPARSE_TAG_ONEDRIVE"),
            (0x8000_0013, "IO_REPARSE_TAG_DEDUP"),
            (0x8000_0014, "IO_REPARSE_TAG_NFS"),
            (0x8000_0017, "IO_REPARSE_TAG_WOF"),
            (0x8000_001B, "IO_REPARSE_TAG_APPEXECLINK"),
            (0x8000_0023, "IO_REPARSE_TAG_AF_UNIX"),
            (0x9000_001C, "IO_REPARSE_TAG_PROJFS"),
            (0xC000_0004, "IO_REPARSE_TAG_HSM"),
            (0, "no tag"),
        ];
        for (tag, name) in redirecting {
            assert!(is_name_surrogate(tag), "{name}");
        }
        for (tag, name) in in_place {
            assert!(!is_name_surrogate(tag), "{name}");
        }
    }

    #[test]
    fn plain_files_and_folders_do_not_redirect_and_a_junction_does() {
        let dir = std::env::temp_dir().join(format!("pikslar-reparse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real")).expect("a folder");
        std::fs::write(dir.join("file.txt"), "x").expect("written");
        let open = |p: &std::path::Path| crate::package::open_no_follow(p).expect("opened");
        assert!(!redirects(&open(&dir.join("file.txt"))).expect("read"));
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let folder = |p: &std::path::Path| {
                std::fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(0x0020_0000 | 0x0200_0000)
                    .open(p)
                    .expect("opened")
            };
            assert!(!redirects(&folder(&dir.join("real"))).expect("read"));
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(dir.join("link"))
                .arg(dir.join("real"))
                .stdout(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if made {
                assert!(redirects(&folder(&dir.join("link"))).expect("read"));
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
