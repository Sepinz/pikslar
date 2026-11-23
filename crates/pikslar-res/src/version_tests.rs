use super::*;
use crate::read;

fn info() -> VersionInfo {
    VersionInfo {
        numbers: [0, 9, 0, 0],
        prerelease: true,
        strings: vec![
            ("ProductName".into(), "Pikslar".into()),
            ("FileVersion".into(), "0.9.0-beta.1".into()),
            ("Comments".into(), "odd".into()),
        ],
    }
}

#[test]
fn versions_parse_to_four_numbers_and_a_prerelease_flag() {
    assert_eq!(parse("0.1.0"), Ok(([0, 1, 0, 0], false)));
    assert_eq!(parse("0.9.0-beta.1"), Ok(([0, 9, 0, 0], true)));
    assert_eq!(parse("12.34.56"), Ok(([12, 34, 56, 0], false)));
    assert_eq!(parse("1.2.3+build.7"), Ok(([1, 2, 3, 0], false)));
}

#[test]
fn malformed_versions_are_refused() {
    for bad in ["", "1.2", "1.2.x", "1.2.3.4", "70000.0.0", "-1.0.0"] {
        assert!(parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn the_block_reads_back_with_its_fixed_part_and_strings_in_order() {
    let bytes = block(&info());
    let (fixed, strings) = read::version_strings(&bytes).expect("ok");
    assert_eq!(fixed[0], 0xFEEF_04BD);
    assert_eq!(fixed[2..6], [9, 0, 9, 0]);
    assert_eq!(fixed[7], 2, "VS_FF_PRERELEASE");
    assert_eq!(fixed[8], 0x0004_0004, "VOS_NT_WINDOWS32");
    assert_eq!(fixed[9], 1, "VFT_APP");
    let want: Vec<(String, String)> = info().strings;
    assert_eq!(strings, want);
}

#[test]
fn the_root_length_covers_the_whole_block_and_nodes_are_aligned() {
    let bytes = block(&info());
    assert_eq!(
        u16::from_le_bytes([bytes[0], bytes[1]]) as usize,
        bytes.len()
    );
    assert_eq!(bytes.len() % 4, 0);
    // "VS_VERSION_INFO" ends at 6 + 32 = 38, so the fixed part starts at 40.
    assert_eq!(&bytes[40..44], &0xFEEF_04BDu32.to_le_bytes());
}

#[test]
fn the_translation_is_english_us_unicode() {
    let bytes = block(&info());
    let tail = &bytes[bytes.len() - 4..];
    assert_eq!(tail, &[0x09, 0x04, 0xB0, 0x04]);
}

#[test]
fn a_release_build_has_no_flags() {
    let mut i = info();
    i.prerelease = false;
    let (fixed, _) = read::version_strings(&block(&i)).expect("ok");
    assert_eq!(fixed[7], 0);
}
