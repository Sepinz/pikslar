use super::*;

const SECTION_VA: u32 = 0x1000;
const SECTION_RAW: u32 = 0x200;

fn put16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

/// A minimal PE32+ file with one section at 0x1000 holding an import table for
/// `KERNEL32.dll` (ExitProcess by name, ordinal 7) and `api-ms-win-crt-heap-l1-1-0.dll`
/// (malloc), and the given time stamp.
fn sample(timestamp: u32) -> Vec<u8> {
    let mut b = vec![0u8; 0x600];
    b[0..2].copy_from_slice(b"MZ");
    put32(&mut b, 0x3C, 0x80);
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    let coff = 0x84;
    put16(&mut b, coff, 0x8664);
    put16(&mut b, coff + 2, 1);
    put32(&mut b, coff + 4, timestamp);
    put16(&mut b, coff + 16, 240);
    let opt = coff + 20;
    put16(&mut b, opt, 0x20B);
    put32(&mut b, opt + 108, 16);
    // Import directory (index 1): descriptors at the section's start.
    put32(&mut b, opt + 112 + 8, SECTION_VA);
    put32(&mut b, opt + 112 + 12, 60);
    let table = opt + 240;
    b[table..table + 5].copy_from_slice(b".idat");
    put32(&mut b, table + 8, 0x400);
    put32(&mut b, table + 12, SECTION_VA);
    put32(&mut b, table + 16, 0x400);
    put32(&mut b, table + 20, SECTION_RAW);
    let raw = |rva: u32| (rva - SECTION_VA + SECTION_RAW) as usize;
    // Descriptor 0: KERNEL32.dll, lookup table at 0x1100, name at 0x1200.
    let d = raw(SECTION_VA);
    put32(&mut b, d, 0x1100);
    put32(&mut b, d + 12, 0x1200);
    put32(&mut b, d + 16, 0x1100);
    // Descriptor 1: the CRT heap set, lookup table at 0x1140, name at 0x1220.
    put32(&mut b, d + 20, 0x1140);
    put32(&mut b, d + 32, 0x1220);
    put32(&mut b, d + 36, 0x1140);
    put64(&mut b, raw(0x1100), 0x1300);
    put64(&mut b, raw(0x1108), 0x8000_0000_0000_0007);
    put64(&mut b, raw(0x1140), 0x1320);
    let text = |b: &mut Vec<u8>, at: usize, s: &[u8]| b[at..at + s.len()].copy_from_slice(s);
    text(&mut b, raw(0x1200), b"KERNEL32.dll\0");
    text(&mut b, raw(0x1220), b"api-ms-win-crt-heap-l1-1-0.dll\0");
    text(&mut b, raw(0x1302), b"ExitProcess\0");
    text(&mut b, raw(0x1322), b"malloc\0");
    b
}

#[test]
fn reads_the_time_stamp() {
    assert_eq!(timestamp(&sample(0)), Ok(0));
    assert_eq!(timestamp(&sample(0x5F00_0001)), Ok(0x5F00_0001));
}

#[test]
fn lists_each_imported_dll_and_its_functions() {
    let found = imports(&sample(0)).expect("an import table");
    assert_eq!(
        found,
        vec![
            Import {
                dll: "KERNEL32.dll".into(),
                functions: vec!["ExitProcess".into(), "#7".into()],
            },
            Import {
                dll: "api-ms-win-crt-heap-l1-1-0.dll".into(),
                functions: vec!["malloc".into()],
            },
        ]
    );
}

#[test]
fn refuses_what_is_not_a_pe32_plus_file() {
    assert!(imports(b"MZ").is_err());
    let mut b = sample(0);
    b[0x80] = b'X';
    assert!(imports(&b).is_err());
    let mut b = sample(0);
    put16(&mut b, 0x84 + 20, 0x10B);
    assert!(imports(&b).is_err(), "PE32 is not read");
}

#[test]
fn a_broken_table_is_an_error_not_a_panic() {
    let mut b = sample(0);
    put32(&mut b, 0x200 + 12, 0xFFFF_FF00);
    assert!(imports(&b).is_err());
    let mut b = sample(0);
    b.truncate(0x3F0);
    assert!(imports(&b).is_err());
}

#[test]
fn finds_text_in_either_case_as_bytes_or_utf16() {
    let mut b = b"xx C:\\Users\\Someone\\x yy".to_vec();
    b.extend("D:/Work/Tree".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(find_text(&b, "c:\\users\\someone"), vec![3]);
    assert_eq!(find_text(&b, "SOMEONE"), vec![12]);
    let wide_at = b.len() - 24;
    assert_eq!(find_text(&b, "d:/work/tree"), vec![wide_at]);
    assert!(find_text(&b, "elsewhere").is_empty());
    assert!(
        find_text(&b, "").is_empty(),
        "an empty needle finds nothing"
    );
}

#[test]
fn import_names_are_judged_against_the_allowed_sets() {
    for ok in [
        "KERNEL32.dll",
        "user32.dll",
        "api-ms-win-crt-runtime-l1-1-0.dll",
        "msvcrt.dll",
        "bcryptprimitives.dll",
    ] {
        assert!(is_system_dll(ok), "{ok}");
    }
    for bad in [
        "libgcc_s_seh-1.dll",
        "libwinpthread-1.dll",
        "VCRUNTIME140.dll",
        "vcruntime140_1.dll",
        "msvcp140.dll",
        "zlib1.dll",
    ] {
        assert!(!is_system_dll(bad), "{bad}");
    }
}
