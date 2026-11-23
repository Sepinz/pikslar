//! Every way a palette file is refused, with the line it names.

use super::*;

fn err(format: PaletteFormat, bytes: &[u8]) -> PaletteError {
    read(bytes, format).expect_err("should be refused")
}

fn at(format: PaletteFormat, bytes: &[u8], line: usize, word: &str) {
    let e = err(format, bytes);
    assert_eq!(e.line, line, "{e}");
    assert!(e.message.contains(word), "'{}' lacks '{word}'", e.message);
}

use PaletteFormat::{Gpl, Hex, Jasc};

#[test]
fn gpl_header_problems() {
    at(Gpl, b"", 1, "GIMP Palette");
    at(Gpl, b"\n\nGIMP Palette\n1 2 3\n", 1, "GIMP Palette");
    at(Gpl, b"JASC-PAL\n0100\n1\n1 2 3\n", 1, "GIMP Palette");
    at(Gpl, b"GIMP Palettes\n1 2 3\n", 1, "GIMP Palette");
}

#[test]
fn gpl_empty_palette_is_refused_for_the_whole_file() {
    for body in [
        "GIMP Palette\n",
        "GIMP Palette\nName: x\nColumns: 3\n# c\n\n",
    ] {
        let e = err(Gpl, body.as_bytes());
        assert_eq!(e.line, 0);
        assert!(e.message.contains("no colors"));
    }
}

#[test]
fn gpl_bad_values_name_the_line() {
    at(Gpl, b"GIMP Palette\n1 2 3\n256 0 0\n", 3, "256");
    at(Gpl, b"GIMP Palette\n1 2 3\n-1 0 0\n", 3, "-1");
    at(Gpl, b"GIMP Palette\n\n1 +2 3\n", 3, "+2");
    at(Gpl, b"GIMP Palette\n1.5 2 3\n", 2, "1.5");
    at(Gpl, b"GIMP Palette\n1 2 x\n", 2, "'x'");
    at(
        Gpl,
        b"GIMP Palette\n99999999999999999999 2 3\n",
        2,
        "not a colour",
    );
}

#[test]
fn gpl_short_color_lines() {
    at(Gpl, b"GIMP Palette\n1 2\n", 2, "found 2 numbers");
    at(Gpl, b"GIMP Palette\n7\n", 2, "found 1 number");
    at(Gpl, b"GIMP Palette\nOrange 1 2 3\n", 2, "Orange");
}

#[test]
fn gpl_header_fields_are_checked_and_only_allowed_before_colours() {
    at(Gpl, b"GIMP Palette\nColumns: lots\n1 2 3\n", 2, "Columns");
    at(Gpl, b"GIMP Palette\nColumns: 257\n1 2 3\n", 2, "Columns");
    at(Gpl, b"GIMP Palette\nColumns: -1\n1 2 3\n", 2, "Columns");
    at(Gpl, b"GIMP Palette\nColumns:\n1 2 3\n", 2, "Columns");
    at(Gpl, b"GIMP Palette\n1 2 3\nName: late\n", 3, "'Name:'");
}

#[test]
fn jasc_header_problems() {
    at(Jasc, b"", 1, "JASC-PAL");
    at(Jasc, b"GIMP Palette\n", 1, "JASC-PAL");
    at(Jasc, b"JASC-PAL\n", 2, "ends before");
    at(Jasc, b"JASC-PAL\n0200\n1\n1 2 3\n", 2, "0100");
    at(Jasc, b"JASC-PAL\n0100\n", 3, "ends before");
    at(
        Jasc,
        b"JASC-PAL\n0100\nmany\n1 2 3\n",
        3,
        "number of colors",
    );
    at(Jasc, b"JASC-PAL\n0100\n-3\n1 2 3\n", 3, "number of colors");
    at(
        Jasc,
        b"JASC-PAL\n0100\n99999999999\n1 2 3\n",
        3,
        "number of colors",
    );
    at(Jasc, b"JASC-PAL\n0100\n\n1 2 3\n", 3, "number of colors");
}

#[test]
fn jasc_color_problems() {
    at(Jasc, b"JASC-PAL\n0100\n1\n1 2\n", 4, "found 2");
    at(
        Jasc,
        b"JASC-PAL\n0100\n2\n1 2 3\n1 2 3 255\n",
        5,
        "exactly three",
    );
    at(Jasc, b"JASC-PAL\n0100\n1\n1 2 300\n", 4, "300");
    at(Jasc, b"JASC-PAL\n0100\n1\n1 two 3\n", 4, "two");
    let e = err(Jasc, b"JASC-PAL\r\n0100\r\n0\r\n");
    assert_eq!(
        (e.line, e.message.as_str()),
        (0, "The palette file has no colors")
    );
}

#[test]
fn hex_problems() {
    at(Hex, b"1a1c2c\n1a1c2\n", 2, "six hex digits");
    at(Hex, b"1a1c2c\n1a1c2cff\n", 2, "six hex digits");
    at(Hex, b"1a1c2c\n##1a1c2c\n", 2, "six hex digits");
    at(Hex, b"1a1c2c\n1a1c2g\n", 2, "six hex digits");
    at(Hex, b"1a1c2c\n1a1c2c ; sky\n", 2, "six hex digits");
    at(Hex, b"\n\n# comment\n", 3, "six hex digits");
    at(Hex, "1a1c2c\n\u{e9}1a1c2\n".as_bytes(), 2, "six hex digits");
    let e = err(Hex, b"\r\n  \r\n");
    assert_eq!(e.line, 0);
}

#[test]
fn utf16_is_refused_with_a_clear_message() {
    for bom in [[0xFF, 0xFE], [0xFE, 0xFF]] {
        for f in [Gpl, Jasc, Hex] {
            let mut b = bom.to_vec();
            b.extend_from_slice(b"G\0I\0M\0P\0");
            let e = err(f, &b);
            assert_eq!(e.line, 0);
            assert!(e.message.contains("UTF-16"));
        }
    }
}

#[test]
fn a_line_over_the_limit_is_refused_but_the_limit_itself_is_fine() {
    let name = "n".repeat(MAX_LINE_BYTES - "1 2 3 ".len());
    let ok = format!("GIMP Palette\n1 2 3 {name}\n");
    assert!(read(ok.as_bytes(), Gpl).is_ok());
    let long = format!("GIMP Palette\n1 2 3 {name}n\n");
    at(Gpl, long.as_bytes(), 2, "1,024");
    let crlf_ok = format!("GIMP Palette\r\n1 2 3 {name}\r\n");
    assert!(read(crlf_ok.as_bytes(), Gpl).is_ok());
    at(
        Hex,
        format!("1a1c2c\n{}\n", "0".repeat(2_000)).as_bytes(),
        2,
        "1,024",
    );
    at(
        Jasc,
        format!("JASC-PAL\n0100\n{}\n", "1".repeat(2_000)).as_bytes(),
        3,
        "1,024",
    );
    at(
        Gpl,
        format!("{}\n", "G".repeat(2_000)).as_bytes(),
        1,
        "1,024",
    );
}

#[test]
fn a_file_over_four_mib_is_refused() {
    let big = vec![b'\n'; MAX_FILE_BYTES + 1];
    for f in [Gpl, Jasc, Hex] {
        let e = err(f, &big);
        assert_eq!(e.line, 0);
        assert!(e.message.contains("4 MiB"));
    }
    let edge = vec![b'\n'; MAX_FILE_BYTES];
    assert_eq!(err(Hex, &edge).line, 0);
    assert!(!err(Hex, &edge).message.contains("4 MiB"));
}

#[test]
fn more_than_65535_colours_is_refused_at_the_first_one_over() {
    let hex_body = "000000\n".repeat(MAX_COLORS + 1);
    at(Hex, hex_body.as_bytes(), MAX_COLORS + 1, "65,535");
    let gpl_body = format!("GIMP Palette\n{}", "0 0 0\n".repeat(MAX_COLORS + 1));
    at(Gpl, gpl_body.as_bytes(), MAX_COLORS + 2, "65,535");
    let pal_body = format!("JASC-PAL\n0100\n3\n{}", "0 0 0\n".repeat(MAX_COLORS + 1));
    at(Jasc, pal_body.as_bytes(), MAX_COLORS + 4, "65,535");
    let exactly = "000000\n".repeat(MAX_COLORS);
    assert_eq!(
        read(exactly.as_bytes(), Hex)
            .expect("at the limit")
            .palette
            .colors
            .len(),
        MAX_COLORS
    );
}

#[test]
fn error_messages_never_carry_raw_control_bytes_or_unbounded_text() {
    let mut body = b"GIMP Palette\n".to_vec();
    body.extend_from_slice(b"\x1b[31m");
    body.extend(std::iter::repeat_n(b'z', 900));
    body.push(b'\n');
    let e = err(Gpl, &body);
    assert!(!e.message.chars().any(char::is_control));
    assert!(e.message.len() < 200);
}
