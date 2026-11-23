//! Reading and writing palette files: fixtures, tolerance, exact output and round trips.

use super::*;

const SUNSET: &[u8] = include_bytes!("../../tests/fixtures/palette/sunset.gpl");
const PLAIN: &[u8] = include_bytes!("../../tests/fixtures/palette/plain.gpl");
const LOSPEC: &[u8] = include_bytes!("../../tests/fixtures/palette/lospec.hex");
const RGB_PAL: &[u8] = include_bytes!("../../tests/fixtures/palette/rgb.pal");
const SHORT: &[u8] = include_bytes!("../../tests/fixtures/palette/short_count.pal");

fn opaque(rgb: &[[u8; 3]]) -> Vec<[u8; 4]> {
    rgb.iter().map(|c| [c[0], c[1], c[2], 255]).collect()
}

fn crlf(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    text.replace('\n', "\r\n").into_bytes()
}

fn bom(bytes: &[u8]) -> Vec<u8> {
    let mut v = vec![0xEF, 0xBB, 0xBF];
    v.extend_from_slice(bytes);
    v
}

#[test]
fn gpl_fixture_is_read_with_names() {
    let r = gpl::read(SUNSET).expect("fixture");
    assert_eq!(r.palette.name.as_deref(), Some("Sunset Glow"));
    assert_eq!(
        r.palette.colors,
        opaque(&[
            [255, 128, 0],
            [20, 10, 60],
            [0, 0, 0],
            [128, 128, 128],
            [255, 255, 255]
        ])
    );
    assert_eq!(r.palette.color_name(0), Some("Orange"));
    assert_eq!(r.palette.color_name(1), Some("Deep violet"));
    assert_eq!(r.palette.color_name(2), None);
    assert_eq!(r.palette.color_name(3), Some("Mid grey  with  spaces"));
    // Swatches have no names, so the person is told the file's are not kept.
    assert_eq!(r.notes.len(), 1, "{:?}", r.notes);
    assert!(r.notes[0].contains("4 color name(s)"), "{:?}", r.notes);
}

#[test]
fn gpl_without_names_has_no_note() {
    let r = gpl::read(
        b"GIMP Palette
1 2 3
4 5 6
",
    )
    .expect("read");
    assert!(r.notes.is_empty());
}

#[test]
fn gpl_fourth_number_is_part_of_the_name_not_alpha() {
    let r = gpl::read(PLAIN).expect("fixture");
    assert_eq!(r.palette.colors, opaque(&[[1, 2, 3], [4, 5, 6]]));
    assert_eq!(r.palette.name, None);
    assert_eq!(r.palette.color_name(1), Some("7"));
}

#[test]
fn gpl_accepts_crlf_bom_and_trailing_spaces() {
    let plain = gpl::read(SUNSET).expect("lf");
    assert_eq!(gpl::read(&crlf(SUNSET)).expect("crlf"), plain);
    assert_eq!(gpl::read(&bom(SUNSET)).expect("bom"), plain);
    assert_eq!(gpl::read(&bom(&crlf(SUNSET))).expect("both"), plain);
    let spaced = b"  gimp palette  \r\n  Name:   Pad  \r\n\t10\t20\t30\t  Named  \t \r\n";
    let r = gpl::read(spaced).expect("padded");
    assert_eq!(r.palette.name.as_deref(), Some("Pad"));
    assert_eq!(r.palette.colors, opaque(&[[10, 20, 30]]));
    assert_eq!(r.palette.color_name(0), Some("Named"));
}

#[test]
fn gpl_without_final_line_end_is_read() {
    let r = gpl::read(b"GIMP Palette\n1 2 3").expect("no newline");
    assert_eq!(r.palette.colors, opaque(&[[1, 2, 3]]));
}

#[test]
fn gpl_non_utf8_names_are_replaced_not_refused() {
    let r = gpl::read(b"GIMP Palette\n1 2 3 caf\xE9\n").expect("latin 1");
    assert_eq!(r.palette.color_name(0), Some("caf\u{FFFD}"));
}

#[test]
fn gpl_control_characters_in_names_become_spaces() {
    let r = gpl::read(b"GIMP Palette\n1 2 3 a\x01b\n").expect("control");
    assert_eq!(r.palette.color_name(0), Some("a b"));
}

#[test]
fn hex_fixture_is_read_in_either_case_with_or_without_hash() {
    let r = hex::read(LOSPEC).expect("fixture");
    assert_eq!(
        r.palette.colors,
        opaque(&[
            [0x1a, 0x1c, 0x2c],
            [0x5d, 0x27, 0x5d],
            [0xb1, 0x3e, 0x53],
            [0xef, 0x7d, 0x57],
            [0xff, 0xcd, 0x75]
        ])
    );
    assert_eq!(hex::read(&crlf(LOSPEC)).expect("crlf"), r);
    assert_eq!(hex::read(&bom(LOSPEC)).expect("bom"), r);
    assert!(r.palette.names.is_empty() && r.palette.name.is_none());
}

#[test]
fn jasc_fixture_is_read() {
    let r = jasc::read(RGB_PAL).expect("fixture");
    assert_eq!(
        r.palette.colors,
        opaque(&[[255, 0, 0], [0, 255, 0], [0, 0, 255]])
    );
    assert!(r.notes.is_empty());
    assert_eq!(jasc::read(&crlf(RGB_PAL)).expect("crlf"), r);
    assert_eq!(jasc::read(&bom(&crlf(RGB_PAL))).expect("bom"), r);
}

#[test]
fn jasc_count_that_disagrees_keeps_the_colours_with_a_note() {
    let r = jasc::read(SHORT).expect("fixture");
    assert_eq!(r.palette.colors, opaque(&[[10, 20, 30], [40, 50, 60]]));
    assert_eq!(r.notes.len(), 1);
    assert!(r.notes[0].contains("256") && r.notes[0].contains('2'));
    let more = jasc::read(b"JASC-PAL\n0100\n1\n1 1 1\n2 2 2\n").expect("more");
    assert_eq!(more.palette.colors.len(), 2);
    assert_eq!(more.notes.len(), 1);
}

#[test]
fn exact_gpl_output() {
    let p = PaletteData {
        name: Some("Two".into()),
        colors: vec![[1, 22, 255, 9], [0, 0, 0, 255]],
        names: vec![Some("Red-ish".into()), None],
    };
    let out = String::from_utf8(gpl::write(&p).expect("write")).expect("utf8");
    assert_eq!(
        out,
        "GIMP Palette\nName: Two\nColumns: 0\n#\n  1  22 255\tRed-ish\n  0   0   0\tUntitled\n"
    );
}

#[test]
fn exact_jasc_output_is_crlf() {
    let p = PaletteData::from_colors(vec![[1, 22, 255, 0], [0, 0, 0, 255]]);
    assert_eq!(
        jasc::write(&p).expect("write"),
        b"JASC-PAL\r\n0100\r\n2\r\n1 22 255\r\n0 0 0\r\n"
    );
}

#[test]
fn exact_hex_output_is_lowercase_without_hash() {
    let p = PaletteData::from_colors(vec![[0xAB, 0xCD, 0xEF, 1], [0, 0, 5, 255]]);
    assert_eq!(hex::write(&p).expect("write"), b"abcdef\n000005\n");
}

fn sample(n: usize) -> PaletteData {
    let colors: Vec<[u8; 4]> = (0..n)
        .map(|i| [(i * 7) as u8, ((i * 13) >> 1) as u8, (i * 31) as u8, 255])
        .collect();
    PaletteData::from_colors(colors)
}

#[test]
fn every_format_round_trips_colours() {
    for n in [1, 2, 16, 256, 5_000, MAX_COLORS] {
        let p = sample(n);
        for f in [PaletteFormat::Gpl, PaletteFormat::Jasc, PaletteFormat::Hex] {
            let bytes = write(&p, f).expect("write");
            let back = read(&bytes, f).expect("read back");
            assert_eq!(back.palette.colors, p.colors, "{f:?} with {n} colours");
            assert!(back.notes.is_empty());
            assert_eq!(read_any(&bytes).expect("sniffed"), back);
        }
    }
}

#[test]
fn gpl_round_trips_names() {
    let p = PaletteData {
        name: Some("Mine".into()),
        colors: opaque(&[[1, 2, 3], [4, 5, 6]]),
        names: vec![Some("Sky blue".into()), Some("Caf\u{e9} \u{1F600}".into())],
    };
    let back = gpl::read(&gpl::write(&p).expect("write"))
        .expect("read")
        .palette;
    assert_eq!(back, p);
}

#[test]
fn writers_clean_and_cut_names_so_the_reader_accepts_the_file() {
    let long = "\u{e9}".repeat(500);
    let p = PaletteData {
        name: Some(format!("a\nb{long}")),
        colors: opaque(&[[1, 2, 3]]),
        names: vec![Some(format!("x\r\ny{long}"))],
    };
    let out = gpl::write(&p).expect("write");
    let back = gpl::read(&out)
        .expect("the writer's own file is accepted")
        .palette;
    assert!(back.name.expect("name").starts_with("a b"));
    assert!(back.names[0].as_deref().expect("name").starts_with("x  y"));
    assert!(back.names[0].as_deref().map_or(0, str::len) <= MAX_NAME_BYTES);
}

#[test]
fn writing_an_empty_or_too_large_palette_is_refused() {
    for f in [PaletteFormat::Gpl, PaletteFormat::Jasc, PaletteFormat::Hex] {
        assert!(write(&PaletteData::default(), f).is_err());
        assert!(write(&sample(MAX_COLORS + 1), f).is_err());
    }
}

#[test]
fn format_by_extension_and_by_first_line() {
    assert_eq!(
        PaletteFormat::from_extension(".GPL"),
        Some(PaletteFormat::Gpl)
    );
    assert_eq!(
        PaletteFormat::from_extension("pal"),
        Some(PaletteFormat::Jasc)
    );
    assert_eq!(
        PaletteFormat::from_extension("hex"),
        Some(PaletteFormat::Hex)
    );
    assert_eq!(PaletteFormat::from_extension("png"), None);
    assert_eq!(PaletteFormat::Jasc.extension(), "pal");
    assert_eq!(PaletteFormat::sniff(&bom(SUNSET)), PaletteFormat::Gpl);
    assert_eq!(PaletteFormat::sniff(RGB_PAL), PaletteFormat::Jasc);
    assert_eq!(PaletteFormat::sniff(LOSPEC), PaletteFormat::Hex);
    assert_eq!(PaletteFormat::sniff(b""), PaletteFormat::Hex);
}

#[test]
fn error_text_names_the_line_and_maps_to_the_limit_error() {
    let e = PaletteError::at(7, "bad");
    assert_eq!(e.to_string(), "Line 7: bad");
    assert_eq!(PaletteError::at(0, "whole").to_string(), "whole");
    assert_eq!(e.limit(), LimitError::PaletteFile);
}

#[test]
fn gpl_direction_and_zero_width_characters_in_names_become_spaces() {
    let r = gpl::read("GIMP Palette\n1 2 3 a\u{202E}b\u{200B}c\n".as_bytes()).expect("hidden");
    assert_eq!(r.palette.color_name(0), Some("a b c"));
}
