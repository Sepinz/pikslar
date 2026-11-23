//! Files dropped on the window from Explorer, through the shell's simple drop message.

use crate::ffi::{DragAcceptFiles, DragFinish, DragQueryFileW, H};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

/// More than anyone drops at once; the rest are left unread.
const MOST_FILES: u32 = 64;
/// Windows paths can reach 32,767 units with the long-path prefix.
const LONGEST_PATH: u32 = 32_768;

/// Lets Explorer drop files on `hwnd`.
///
/// # Safety
/// `hwnd` must be a live window owned by this thread.
pub(crate) unsafe fn accept_files(hwnd: H) {
    DragAcceptFiles(hwnd, 1);
}

/// The paths in a drop, in the order given, and frees the drop.
///
/// # Safety
/// `drop` must be the handle from a `WM_DROPFILES` message, not yet finished.
pub(crate) unsafe fn dropped_files(drop: H) -> Vec<PathBuf> {
    let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0).min(MOST_FILES);
    let mut paths = Vec::new();
    for index in 0..count {
        let length = DragQueryFileW(drop, index, std::ptr::null_mut(), 0);
        if length == 0 || length >= LONGEST_PATH {
            continue;
        }
        let mut unit = vec![0u16; length as usize + 1];
        let copied = DragQueryFileW(drop, index, unit.as_mut_ptr(), length + 1);
        unit.truncate(copied as usize);
        if !unit.is_empty() {
            paths.push(PathBuf::from(OsString::from_wide(&unit)));
        }
    }
    DragFinish(drop);
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{GlobalAlloc, GlobalLock, GlobalUnlock};

    const GMEM_MOVEABLE_ZEROED: u32 = 0x0042;

    /// A drop as Explorer makes one: a DROPFILES header, then wide paths, each ended by a zero,
    /// and a last zero.
    fn make_drop(paths: &[&str]) -> H {
        let mut units: Vec<u16> = Vec::new();
        for p in paths {
            units.extend(p.encode_utf16());
            units.push(0);
        }
        units.push(0);
        let header: [u32; 5] = [20, 0, 0, 0, 1];
        let bytes = 20 + units.len() * 2;
        // SAFETY: the block is allocated with room for the header and every unit, written
        // while locked, and handed to `dropped_files`, which frees it.
        unsafe {
            let memory = GlobalAlloc(GMEM_MOVEABLE_ZEROED, bytes);
            assert!(!memory.is_null());
            let at = GlobalLock(memory) as *mut u8;
            std::ptr::copy_nonoverlapping(header.as_ptr() as *const u8, at, 20);
            std::ptr::copy_nonoverlapping(units.as_ptr() as *const u8, at.add(20), units.len() * 2);
            GlobalUnlock(memory);
            memory
        }
    }

    #[test]
    fn dropped_paths_come_back_whole_and_in_order() {
        let drop = make_drop(&["C:\\art\\walk.piks", "D:\\Æøå 漢\\run.png"]);
        // SAFETY: a drop just made, not yet finished.
        let paths = unsafe { dropped_files(drop) };
        assert_eq!(
            paths,
            vec![
                PathBuf::from("C:\\art\\walk.piks"),
                PathBuf::from("D:\\Æøå 漢\\run.png")
            ]
        );
    }

    #[test]
    fn an_empty_drop_gives_nothing() {
        let drop = make_drop(&[]);
        // SAFETY: a drop just made, not yet finished.
        assert!(unsafe { dropped_files(drop) }.is_empty());
    }
}
