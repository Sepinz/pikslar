//! The folders Windows starts programs from at sign-in. A file put there, whatever its kind,
//! is a program the user never chose to run, so a plugin's file is never written in or under
//! them, however the dialog's destination was reached.

use std::path::PathBuf;

/// The user's and the common Startup folders that exist, as the system names them. Empty off
/// Windows, where there are none.
#[cfg(windows)]
pub(crate) fn folders() -> Vec<PathBuf> {
    use std::ffi::{c_void, OsString};
    use std::os::windows::ffi::OsStringExt;

    #[repr(C)]
    struct Guid(u32, u16, u16, [u8; 8]);

    // FOLDERID_Startup and FOLDERID_CommonStartup.
    const STARTUP: Guid = Guid(
        0xB97D_20BB,
        0xF46A,
        0x4C97,
        [0xBA, 0x10, 0x5E, 0x36, 0x08, 0x43, 0x08, 0x54],
    );
    const COMMON_STARTUP: Guid = Guid(
        0x82A5_EA35,
        0xD9CD,
        0x47C5,
        [0x96, 0x29, 0xE1, 0x5D, 0x2F, 0x71, 0x4E, 0x6E],
    );

    type KnownFolder =
        unsafe extern "system" fn(*const Guid, u32, *mut c_void, *mut *mut u16) -> i32;
    type Free = unsafe extern "system" fn(*mut c_void);

    /// `LOAD_LIBRARY_SEARCH_SYSTEM32`: System32 only, never beside the exe.
    const SYSTEM32_ONLY: u32 = 0x800;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *const c_void;
    }

    // The two system calls are looked up when asked for, not linked: a program that links
    // this crate but never writes a plugin's file (the command line) then imports neither
    // library, which it must not load.
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain([0]).collect() };
    // SAFETY: the names are NUL-terminated; a missing library or function gives null, which is
    // checked before use. The libraries stay loaded (system libraries, loaded anyway).
    let (ask_folder, free) = unsafe {
        let shell = LoadLibraryExW(
            wide("shell32.dll").as_ptr(),
            std::ptr::null_mut(),
            SYSTEM32_ONLY,
        );
        let ole = LoadLibraryExW(
            wide("ole32.dll").as_ptr(),
            std::ptr::null_mut(),
            SYSTEM32_ONLY,
        );
        if shell.is_null() || ole.is_null() {
            return Vec::new();
        }
        let folder = GetProcAddress(shell, c"SHGetKnownFolderPath".as_ptr().cast());
        let free = GetProcAddress(ole, c"CoTaskMemFree".as_ptr().cast());
        if folder.is_null() || free.is_null() {
            return Vec::new();
        }
        // SAFETY (the casts): these are the documented signatures of the two functions.
        (
            std::mem::transmute::<*const c_void, KnownFolder>(folder),
            std::mem::transmute::<*const c_void, Free>(free),
        )
    };

    let ask = |id: &Guid| -> Option<PathBuf> {
        let mut raw: *mut u16 = std::ptr::null_mut();
        // SAFETY: `id` is a valid GUID, flags 0 and a null token mean the current user, and
        // `raw` is a writable pointer the call fills in.
        let hr = unsafe { ask_folder(id, 0, std::ptr::null_mut(), &mut raw) };
        if raw.is_null() {
            return None;
        }
        let path = (hr >= 0).then(|| {
            // SAFETY: on success `raw` points to a NUL-terminated UTF-16 string, which is
            // read up to its NUL (found before reading past it) and not kept.
            let wide = unsafe {
                let mut len = 0;
                while *raw.add(len) != 0 {
                    len += 1;
                }
                std::slice::from_raw_parts(raw, len)
            };
            PathBuf::from(OsString::from_wide(wide))
        });
        // SAFETY: `raw` was allocated by the call above, which says to free it this way even
        // on failure, and is not used afterwards.
        unsafe { free(raw.cast()) };
        path
    };
    [ask(&STARTUP), ask(&COMMON_STARTUP)]
        .into_iter()
        .flatten()
        .collect()
}

#[cfg(not(windows))]
pub(crate) fn folders() -> Vec<PathBuf> {
    Vec::new()
}
