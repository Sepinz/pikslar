//! The built program carries its icon, version information and manifest, read back two ways:
//! by parsing the PE file, and through the Windows APIs that Explorer and the loader use.

use pikslar_res::{read, RT_GROUP_ICON, RT_ICON, RT_MANIFEST, RT_VERSION};
use std::path::Path;

/// The built program, or the file `PIKSLAR_EXE` names (the release script checks its own).
fn exe_path() -> String {
    std::env::var("PIKSLAR_EXE").unwrap_or_else(|_| env!("CARGO_BIN_EXE_pikslar").into())
}

fn resources() -> Vec<(read::Leaf, Vec<u8>)> {
    let bytes = std::fs::read(exe_path()).expect("the built exe");
    read::pe_resources(&bytes).expect("a readable resource tree")
}

fn of_type(ty: u16) -> Vec<(read::Leaf, Vec<u8>)> {
    resources()
        .into_iter()
        .filter(|(l, _)| l.ty == ty)
        .collect()
}

#[test]
fn there_is_exactly_one_manifest_and_it_is_the_programs_own() {
    let manifests = of_type(RT_MANIFEST);
    assert_eq!(
        manifests.len(),
        1,
        "a second manifest means the MinGW default was linked too"
    );
    let (leaf, data) = &manifests[0];
    assert_eq!(leaf.id, 1);
    let text = std::str::from_utf8(data).expect("UTF-8");
    for needle in [
        "PerMonitorV2",
        "true/pm",
        "<longPathAware",
        "asInvoker",
        "8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
    assert!(
        !text.contains("Common-Controls"),
        "no Common Controls 6 dependency"
    );
}

#[test]
fn the_icon_group_lists_images_that_exist() {
    let groups = of_type(RT_GROUP_ICON);
    assert_eq!(groups.len(), 1);
    let (leaf, group) = &groups[0];
    assert_eq!(leaf.id, 1, "the id the window class loads");
    let count = u16::from_le_bytes([group[4], group[5]]) as usize;
    assert_eq!(count, 8, "16, 20, 24, 32, 40, 48, 64 and 256");
    let icons = of_type(RT_ICON);
    for i in 0..count {
        let at = 6 + 14 * i;
        let id = u16::from_le_bytes([group[at + 12], group[at + 13]]);
        let size =
            u32::from_le_bytes([group[at + 8], group[at + 9], group[at + 10], group[at + 11]]);
        let image = icons
            .iter()
            .find(|(l, _)| l.id == id)
            .expect("the listed image");
        assert_eq!(image.1.len() as u32, size);
    }
}

#[test]
fn the_version_block_names_the_program_and_its_version() {
    let versions = of_type(RT_VERSION);
    assert_eq!(versions.len(), 1);
    let (fixed, strings) = read::version_strings(&versions[0].1).expect("a version block");
    let get = |k: &str| {
        strings
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(fixed[0], 0xFEEF_04BD);
    assert_eq!(get("ProductName"), Some("Pikslar"));
    assert_eq!(get("FileDescription"), Some("Pikslar pixel-art editor"));
    assert_eq!(get("ProductVersion"), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(get("FileVersion"), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(get("OriginalFilename"), Some("pikslar.exe"));
    assert!(get("Comments").is_some_and(|c| c.starts_with("Commit ")));
    assert!(get("LegalCopyright").is_some_and(|c| !c.is_empty()));
}

mod windows_api {
    //! The same file read through version.dll and user32, as File Properties and the shell do.

    use super::{exe_path, Path};

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryExW(path: *const u16, file: isize, flags: u32) -> isize;
        fn FreeLibrary(module: isize) -> i32;
    }
    #[link(name = "user32")]
    extern "system" {
        fn LoadImageW(m: isize, name: usize, kind: u32, cx: i32, cy: i32, flags: u32) -> isize;
        fn DestroyIcon(icon: isize) -> i32;
    }
    #[link(name = "version", kind = "raw-dylib")]
    extern "system" {
        fn GetFileVersionInfoSizeW(path: *const u16, handle: *mut u32) -> u32;
        fn GetFileVersionInfoW(path: *const u16, handle: u32, len: u32, out: *mut u8) -> i32;
        fn VerQueryValueW(
            block: *const u8,
            sub: *const u16,
            out: *mut *const u8,
            len: *mut u32,
        ) -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    fn exe() -> Vec<u16> {
        let path = exe_path();
        assert!(Path::new(&path).is_file());
        wide(&path)
    }

    #[test]
    fn windows_reads_the_product_name_and_version() {
        let path = exe();
        let mut handle = 0;
        // SAFETY: null-terminated path and out-pointers that live through each call.
        let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut handle) };
        assert!(size > 0);
        let mut block = vec![0u8; size as usize];
        let ok = unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, block.as_mut_ptr()) };
        assert_ne!(ok, 0);
        let query = |key: &str| {
            let sub = wide(&format!(r"\StringFileInfo\040904B0\{key}"));
            let (mut p, mut n) = (std::ptr::null(), 0u32);
            // SAFETY: `block` holds the version data; VerQueryValueW points into it.
            let found = unsafe { VerQueryValueW(block.as_ptr(), sub.as_ptr(), &mut p, &mut n) };
            assert_ne!(found, 0, "{key}");
            let units = unsafe { std::slice::from_raw_parts(p as *const u16, n as usize) };
            String::from_utf16_lossy(units)
                .trim_end_matches('\0')
                .to_string()
        };
        assert_eq!(query("ProductName"), "Pikslar");
        assert_eq!(query("ProductVersion"), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn windows_loads_the_icon_at_small_large_and_256_pixels() {
        let path = exe();
        // LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE: no code runs.
        // SAFETY: a null-terminated path; the module is freed below.
        let module = unsafe { LoadLibraryExW(path.as_ptr(), 0, 0x2 | 0x20) };
        assert_ne!(module, 0);
        for size in [16, 20, 24, 32, 40, 48, 64, 256] {
            // SAFETY: a loaded module and resource id 1, IMAGE_ICON.
            let icon = unsafe { LoadImageW(module, 1, 1, size, size, 0) };
            assert_ne!(icon, 0, "{size}");
            unsafe { DestroyIcon(icon) };
        }
        unsafe { FreeLibrary(module) };
    }
}
