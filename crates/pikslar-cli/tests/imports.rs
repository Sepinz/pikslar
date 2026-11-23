//! The command line program must not load window or graphics libraries. This reads the DLLs
//! named in its import tables, ordinary and delayed, straight from the executable.

use std::path::Path;

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Sections as (address in memory, size, offset in the file).
fn sections(b: &[u8], pe: usize) -> Option<Vec<(usize, usize, usize)>> {
    let count = u16_at(b, pe + 6)?;
    let first = pe + 24 + u16_at(b, pe + 20)?;
    (0..count)
        .map(|i| {
            let s = first + i * 40;
            let size = u32_at(b, s + 8)?.max(u32_at(b, s + 16)?);
            Some((u32_at(b, s + 12)?, size, u32_at(b, s + 20)?))
        })
        .collect()
}

fn offset(secs: &[(usize, usize, usize)], rva: usize) -> Option<usize> {
    secs.iter()
        .find(|(va, size, _)| (*va..va + size).contains(&rva))
        .map(|(va, _, raw)| rva - va + raw)
}

fn name_at(b: &[u8], at: usize) -> Option<String> {
    let end = b.get(at..)?.iter().position(|&c| c == 0)?;
    Some(String::from_utf8_lossy(&b[at..at + end]).to_ascii_lowercase())
}

/// Reads one import directory: `entry` is its data directory, `stride` and `name` its layout.
fn directory(
    b: &[u8],
    pe: usize,
    (entry, stride, name): (usize, usize, usize),
) -> Option<Vec<String>> {
    let opt = pe + 24;
    let dirs = opt + if u16_at(b, opt)? == 0x20b { 112 } else { 96 };
    let rva = u32_at(b, dirs + entry * 8)?;
    if rva == 0 {
        return Some(Vec::new());
    }
    let secs = sections(b, pe)?;
    let mut at = offset(&secs, rva)?;
    let mut names = Vec::new();
    while b.get(at..at + stride)?.iter().any(|&c| c != 0) {
        names.push(name_at(b, offset(&secs, u32_at(b, at + name)?)?)?);
        at += stride;
    }
    Some(names)
}

fn imports(path: &Path) -> Vec<String> {
    let b = std::fs::read(path).expect("read the program");
    let pe = u32_at(&b, 0x3c).expect("DOS header");
    assert_eq!(
        b.get(pe..pe + 4),
        Some(&b"PE\0\0"[..]),
        "a Windows executable"
    );
    let mut all = directory(&b, pe, (1, 20, 12)).expect("import table");
    all.extend(directory(&b, pe, (13, 32, 4)).expect("delayed import table"));
    all
}

#[test]
fn the_program_loads_no_window_or_graphics_library() {
    let found = imports(Path::new(env!("CARGO_BIN_EXE_pikslar-cli")));
    assert!(
        found.iter().any(|n| n == "kernel32.dll"),
        "the reader works: {found:?}"
    );
    let forbidden = [
        "user32.dll",
        "gdi32.dll",
        "d3d11.dll",
        "d3d12.dll",
        "dxgi.dll",
        "dwmapi.dll",
        "comdlg32.dll",
        "shell32.dll",
        "ole32.dll",
        "uxtheme.dll",
        "opengl32.dll",
    ];
    for name in forbidden {
        assert!(!found.iter().any(|n| n == name), "{name} in {found:?}");
    }
}
