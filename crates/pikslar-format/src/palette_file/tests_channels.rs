//! GIMP palettes with Aseprite's `Channels: RGBA` extension (Aseprite's
//! `docs/gpl-palette-extension.md`): the header line, then four numbers a colour.

use super::*;

/// The example of Aseprite's description of the extension.
const ASEPRITE_RGBA: &[u8] = b"GIMP Palette
Channels: RGBA
#
  0   0   0   0 Transparent
254  91  89 255 Red
247 165  71 255 Orange
243 206  82 255 Yellow
106 205  91 255 Green
 87 185 242 255 Blue
209 134 223 255 Purple
165 165 167 255 Gray
";

#[test]
fn the_aseprite_rgba_example_is_read_opaque_with_a_note() {
    let r = gpl::read(ASEPRITE_RGBA).expect("Aseprite's own example");
    let rgb: Vec<[u8; 4]> = r.palette.colors.clone();
    assert_eq!(rgb.len(), 8);
    assert_eq!(rgb[0], [0, 0, 0, 255]);
    assert_eq!(rgb[1], [254, 91, 89, 255]);
    assert_eq!(rgb[7], [165, 165, 167, 255]);
    // The alpha is not a part of the name.
    assert_eq!(r.palette.color_name(0), Some("Transparent"));
    assert_eq!(r.palette.color_name(1), Some("Red"));
    let partly = r.notes.iter().filter(|n| n.contains("1 color(s)")).count();
    assert_eq!(partly, 1, "{:?}", r.notes);
}

#[test]
fn rgba_colours_that_are_all_opaque_add_no_alpha_note() {
    let r = gpl::read(b"GIMP Palette\nChannels: RGBA\n1 2 3 255\n4 5 6 255 x\n").expect("read");
    assert_eq!(r.palette.colors, [[1, 2, 3, 255], [4, 5, 6, 255]]);
    assert_eq!(r.palette.color_name(1), Some("x"));
    assert!(
        r.notes.iter().all(|n| !n.contains("clear")),
        "{:?}",
        r.notes
    );
}

#[test]
fn channels_rgb_is_the_plain_format() {
    let plain = gpl::read(b"GIMP Palette\n1 2 3 7\n").expect("plain");
    let said = gpl::read(b"GIMP Palette\nChannels:  RGB \n1 2 3 7\n").expect("said");
    assert_eq!(said, plain);
    assert_eq!(said.palette.color_name(0), Some("7"));
}

#[test]
fn bad_channels_lines_are_refused_with_their_line() {
    use PaletteFormat::Gpl;
    for (bytes, line) in [
        (&b"GIMP Palette\nChannels: CMYK\n1 2 3\n"[..], 2),
        (b"GIMP Palette\nChannels:\n1 2 3\n", 2),
        (b"GIMP Palette\nChannels: RGBA\n1 2 3\n", 3),
        (b"GIMP Palette\nChannels: RGBA\n1 2 3 256\n", 3),
        (b"GIMP Palette\n1 2 3\nChannels: RGBA\n", 3),
    ] {
        let e = read(bytes, Gpl).expect_err("refused");
        assert_eq!(e.line, line, "{e} for {:?}", String::from_utf8_lossy(bytes));
    }
}
