//! The Aseprite importer, on files built byte by byte from the public specification. No file
//! here was made by Aseprite itself.

use crate::ase_build::*;
use crate::{import_ase, AseWarning, FormatError};
use pikslar_core::layer::LayerKind;
use pikslar_core::{rgba, BlendMode, ColorMode, Direction, Document};

pub(crate) fn open(bytes: &[u8]) -> (Document, Vec<AseWarning>) {
    import_ase(bytes).expect("a file that imports")
}

pub(crate) fn refused(bytes: &[u8]) -> FormatError {
    import_ase(bytes).expect_err("a file that is refused")
}

pub(crate) fn one_layer(w: u16, h: u16, chunks: Vec<Vec<u8>>) -> Vec<u8> {
    let mut all = vec![plain("L")];
    all.extend(chunks);
    Ase::new(w, h, 32).frame(100, all).bytes()
}

pub(crate) fn pixel(doc: &Document, layer: usize, frame: usize, x: i32, y: i32) -> u32 {
    doc.layers()[layer].cel(frame).get(x, y)
}

fn has(warnings: &[AseWarning], what: &AseWarning) -> bool {
    warnings.contains(what)
}

#[test]
fn the_header_gives_the_canvas_and_frame_lengths() {
    let mut ase = Ase::new(7, 5, 32);
    ase.speed = 90;
    let ase = ase
        .frame(120, vec![plain("a")])
        .frame(0, vec![])
        .frame(65_535, vec![]);
    let (doc, warnings) = open(&ase.bytes());
    assert_eq!((doc.width, doc.height), (7, 5));
    let lengths: Vec<u32> = doc.frames.iter().map(|f| f.duration_ms).collect();
    // 0 means the header's speed; a very long frame is cut to the longest there may be.
    assert_eq!(lengths, [120, 90, 60_000]);
    assert!(has(&warnings, &AseWarning::FrameDuration));
    assert_eq!(doc.layers()[0].mode, ColorMode::Rgba);
}

#[test]
fn a_frame_of_length_zero_and_no_speed_is_a_tenth_of_a_second() {
    let mut ase = Ase::new(2, 2, 32);
    ase.speed = 0;
    let (doc, warnings) = open(&ase.frame(0, vec![plain("a")]).frame(3, vec![]).bytes());
    assert_eq!(doc.frames[0].duration_ms, 100);
    assert_eq!(doc.frames[1].duration_ms, 10);
    assert!(has(&warnings, &AseWarning::FrameDuration));
    assert_eq!(
        AseWarning::FrameDuration.to_string(),
        "Some frame times were outside 10 to 60000 ms and were set to the nearest of those"
    );
}

#[test]
fn a_raw_cel_lands_where_its_offset_says() {
    let pic = rgba_picture(2, 2, 10);
    let (doc, warnings) = open(&one_layer(8, 8, vec![cel_raw(0, 3, -1, 2, 2, &pic)]));
    assert!(warnings.is_empty(), "{warnings:?}");
    let want = |i: usize| rgba(pic[i * 4], pic[i * 4 + 1], pic[i * 4 + 2], pic[i * 4 + 3]);
    assert_eq!(pixel(&doc, 0, 0, 3, -1), want(0));
    assert_eq!(pixel(&doc, 0, 0, 4, -1), want(1));
    assert_eq!(pixel(&doc, 0, 0, 3, 0), want(2));
    assert_eq!(pixel(&doc, 0, 0, 4, 0), want(3));
    assert_eq!(pixel(&doc, 0, 0, 2, 0), 0);
    assert_eq!(doc.layers()[0].cel(0).position(), (3, -1));
}

#[test]
fn a_zlib_cel_reads_as_the_raw_one_does() {
    let pic = rgba_picture(37, 21, 4);
    let raw = open(&one_layer(40, 30, vec![cel_raw(0, 1, 2, 37, 21, &pic)])).0;
    let packed = open(&one_layer(40, 30, vec![cel_zlib(0, 1, 2, 37, 21, &pic)])).0;
    let stored = open(&one_layer(
        40,
        30,
        vec![cel(
            0,
            1,
            2,
            255,
            2,
            0,
            &[37, 0, 21, 0]
                .iter()
                .chain(&zlib_stored(&pic))
                .copied()
                .collect::<Vec<u8>>(),
        )],
    ))
    .0;
    for doc in [&packed, &stored] {
        for (x, y) in [(1, 2), (37, 22), (20, 10), (0, 0), (38, 23)] {
            assert_eq!(pixel(doc, 0, 0, x, y), pixel(&raw, 0, 0, x, y), "{x},{y}");
        }
    }
    assert_ne!(pixel(&raw, 0, 0, 20, 10), 0);
}

#[test]
fn a_transparent_pixel_is_clear() {
    let (doc, _) = open(&one_layer(
        4,
        4,
        vec![cel_raw(0, 0, 0, 1, 1, &[9, 9, 9, 0])],
    ));
    assert_eq!(pixel(&doc, 0, 0, 0, 0), 0);
}

#[test]
fn layer_flags_name_and_order_are_kept() {
    let ase = Ase::new(4, 4, 32).frame(
        100,
        vec![
            layer(1 | 2, 0, 0, 0, 255, "bottom"),
            layer(0, 0, 0, 0, 255, "hidden and locked"),
            layer(64 | 3, 0, 0, 0, 255, "reference"),
        ],
    );
    let (doc, warnings) = open(&ase.bytes());
    let names: Vec<&str> = doc.layers().iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["bottom", "hidden and locked", "reference"]);
    let l = doc.layers();
    assert!(l[0].visible && !l[0].locked);
    assert!(!l[1].visible && l[1].locked);
    assert_eq!(l[2].kind, LayerKind::Paint);
    assert!(has(
        &warnings,
        &AseWarning::ReferenceLayer("reference".into())
    ));
}

#[test]
fn layer_opacity_counts_only_when_the_header_says_so() {
    let layers = vec![layer(3, 0, 0, 0, 77, "a")];
    let mut ase = Ase::new(2, 2, 32);
    let without = open(&ase.frame(100, layers.clone()).bytes()).0;
    assert_eq!(without.layers()[0].opacity, 255);
    ase = Ase::new(2, 2, 32);
    ase.flags = 1;
    let with = open(&ase.frame(100, layers).bytes()).0;
    assert_eq!(with.layers()[0].opacity, 77);
}

#[test]
fn every_aseprite_blend_mode_is_kept_and_an_unknown_one_warns() {
    use pikslar_core::Blending;
    use BlendMode::*;
    let table: [(u16, BlendMode, bool); 20] = [
        (0, Normal, true),
        (1, Multiply, true),
        (2, Screen, true),
        (3, Overlay, true),
        (4, Darken, true),
        (5, Lighten, true),
        (6, ColorDodge, true),
        (7, ColorBurn, true),
        (8, HardLight, true),
        (9, SoftLight, true),
        (10, Difference, true),
        (11, Exclusion, true),
        (12, Hue, true),
        (13, Saturation, true),
        (14, Color, true),
        (15, Luminosity, true),
        (16, Addition, true),
        (17, Subtract, true),
        (18, Divide, true),
        (19, Normal, false),
    ];
    for (number, mode, exact) in table {
        let ase = Ase::new(2, 2, 32).frame(100, vec![layer(3, 0, 0, number, 255, "x")]);
        let (doc, warnings) = open(&ase.bytes());
        assert_eq!(doc.layers()[0].blend, mode, "mode {number}");
        // A mode other than Normal is drawn as Aseprite draws it.
        let wanted = match mode {
            Normal => Blending::Pikslar,
            _ => Blending::Aseprite,
        };
        assert_eq!(doc.blending, wanted, "mode {number}");
        let told = AseWarning::LayerBlend {
            layer: "x".into(),
            mode: number,
            shown_as: mode,
        };
        assert_eq!(has(&warnings, &told), !exact, "mode {number}");
    }
}

fn nested() -> Vec<u8> {
    // The shape of the specification's own example, with a layer beneath it.
    let first = vec![
        layer(3, 0, 0, 0, 255, "Base"),
        layer(3, 1, 0, 0, 255, "Foreground"),
        layer(3, 1, 1, 0, 255, "My set1"),
        layer(3, 0, 2, 0, 255, "Layer2"),
        layer(3, 0, 1, 0, 255, "Layer3"),
        cel_raw(0, 0, 0, 1, 1, &[1, 0, 0, 255]),
        cel_raw(3, 0, 0, 1, 1, &[0, 2, 0, 255]),
        cel_raw(4, 0, 0, 1, 1, &[0, 0, 3, 255]),
    ];
    Ase::new(4, 4, 32).frame(100, first).bytes()
}

#[test]
fn groups_come_after_what_they_hold_and_cels_follow_their_layers() {
    let (doc, warnings) = open(&nested());
    assert!(warnings.is_empty(), "{warnings:?}");
    let rows: Vec<(&str, u8, bool)> = doc
        .layers()
        .iter()
        .map(|l| (l.name.as_str(), l.depth, l.kind == LayerKind::Group))
        .collect();
    assert_eq!(
        rows,
        [
            ("Base", 0, false),
            ("Layer2", 2, false),
            ("My set1", 1, true),
            ("Layer3", 1, false),
            ("Foreground", 0, true),
        ]
    );
    doc.check_groups().expect("a valid list");
    assert_eq!(pixel(&doc, 0, 0, 0, 0), rgba(1, 0, 0, 255));
    assert_eq!(pixel(&doc, 1, 0, 0, 0), rgba(0, 2, 0, 255));
    assert_eq!(pixel(&doc, 3, 0, 0, 0), rgba(0, 0, 3, 255));
    assert!(doc.layers()[2].cels().iter().all(|c| c.is_blank()));
}

#[test]
fn a_collapsed_group_is_folded() {
    let ase = Ase::new(2, 2, 32).frame(
        100,
        vec![
            layer(3 | 32, 1, 0, 0, 255, "g"),
            layer(3, 0, 1, 0, 255, "c"),
        ],
    );
    let (doc, _) = open(&ase.bytes());
    assert!(doc.layers()[1].folded);
    assert!(!doc.layers()[0].folded);
}

#[test]
fn a_group_keeps_its_eye_and_lock_but_not_its_own_blend() {
    let mut ase = Ase::new(2, 2, 32);
    ase.flags = 3;
    let chunks = vec![layer(1, 1, 0, 1, 100, "g"), layer(3, 0, 1, 0, 255, "c")];
    let (doc, warnings) = open(&ase.frame(100, chunks).bytes());
    let group = &doc.layers()[1];
    assert!(group.visible && group.locked);
    assert_eq!((group.opacity, group.blend), (255, BlendMode::Normal));
    assert!(has(&warnings, &AseWarning::GroupSettings("g".into())));
}

#[test]
fn a_group_nested_four_deep_is_read_and_five_is_refused() {
    let mut four = vec![];
    for level in 0..4 {
        four.push(layer(3, 1, level, 0, 255, "g"));
    }
    four.push(layer(3, 0, 4, 0, 255, "leaf"));
    let (doc, _) = open(&Ase::new(2, 2, 32).frame(100, four.clone()).bytes());
    assert_eq!(doc.layers().iter().map(|l| l.depth).max(), Some(4));
    four.insert(4, layer(3, 1, 4, 0, 255, "g"));
    four.push(layer(3, 0, 5, 0, 255, "deeper"));
    let err = refused(&Ase::new(2, 2, 32).frame(100, four).bytes());
    assert!(matches!(err, FormatError::Unsupported(_)), "{err}");
}

#[test]
fn a_layer_deeper_than_its_parent_allows_is_refused() {
    let chunks = vec![layer(3, 0, 0, 0, 255, "a"), layer(3, 0, 1, 0, 255, "b")];
    let err = refused(&Ase::new(2, 2, 32).frame(100, chunks).bytes());
    assert!(matches!(err, FormatError::Corrupt(_)), "{err}");
}

#[test]
fn a_cel_on_a_group_is_left_out_with_a_warning() {
    let chunks = vec![
        layer(3, 1, 0, 0, 255, "g"),
        plain("c"),
        cel_raw(0, 0, 0, 1, 1, &[1, 1, 1, 255]),
    ];
    let (doc, warnings) = open(&Ase::new(2, 2, 32).frame(100, chunks).bytes());
    assert!(doc
        .layers()
        .iter()
        .flat_map(|l| l.cels())
        .all(|c| c.is_blank()));
    assert!(warnings.iter().any(|w| matches!(w, AseWarning::Skipped(_))));
}

#[test]
fn linked_cels_become_one_hold() {
    let pic = rgba_picture(3, 3, 2);
    let ase = Ase::new(8, 8, 32)
        .frame(100, vec![plain("L"), cel_zlib(0, 2, 1, 3, 3, &pic)])
        .frame(100, vec![cel_linked(0, 0)])
        .frame(100, vec![cel_linked(0, 1)])
        .frame(100, vec![cel_raw(0, 0, 0, 1, 1, &[5, 5, 5, 255])]);
    let (doc, warnings) = open(&ase.bytes());
    assert!(warnings.is_empty(), "{warnings:?}");
    let layer = &doc.layers()[0];
    let ids: Vec<u32> = layer.links().collect();
    assert_ne!(ids[0], 0);
    assert_eq!(ids[0], ids[1]);
    assert_eq!(ids[1], ids[2]);
    assert_eq!(ids[3], 0);
    for frame in 1..3 {
        assert_eq!(layer.cel(frame), layer.cel(0));
    }
    assert_ne!(pixel(&doc, 0, 2, 2, 1), 0);
    assert!(doc.is_held(0, 2));
}

#[test]
fn a_link_with_nothing_to_link_with_is_left_empty_with_a_warning() {
    let ase = Ase::new(4, 4, 32)
        .frame(100, vec![plain("L"), cel_linked(0, 1)])
        .frame(100, vec![cel_linked(0, 1)])
        .frame(100, vec![cel_linked(0, 9)]);
    let (doc, warnings) = open(&ase.bytes());
    assert!(doc.layers()[0].cels().iter().all(|c| c.is_blank()));
    assert!(has(&warnings, &AseWarning::UnresolvedLink("L".into())));
    assert!(doc.layers()[0].links().all(|l| l == 0));
}

#[test]
fn a_cel_z_index_is_not_kept_and_says_so() {
    let c = cel(0, 0, 0, 255, 0, 2, &[1, 0, 1, 0, 1, 2, 3, 255]);
    let (doc, warnings) = open(&one_layer(4, 4, vec![c]));
    assert_eq!(pixel(&doc, 0, 0, 0, 0), rgba(1, 2, 3, 255));
    assert!(has(&warnings, &AseWarning::CelZIndex("L".into())));
}

#[test]
fn indexed_pixels_use_the_palette_and_the_transparent_index() {
    let mut ase = Ase::new(4, 4, 8);
    ase.transparent = 1;
    let colors = [[10, 20, 30, 255], [40, 50, 60, 255], [70, 80, 90, 128]];
    let ase = ase.frame(
        100,
        vec![
            layer(3 | 8, 0, 0, 0, 255, "background"),
            layer(3, 0, 0, 0, 255, "ink"),
            palette(3, 0, &colors),
            cel_raw(0, 0, 0, 3, 1, &[0, 1, 2]),
            cel_raw(1, 0, 0, 3, 1, &[0, 1, 2]),
        ],
    );
    let (doc, warnings) = open(&ase.bytes());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(doc.layers().iter().all(|l| l.mode == ColorMode::Palette));
    let color = |id: u32| doc.colors.color(id);
    assert_eq!(color(pixel(&doc, 0, 0, 0, 0)), rgba(10, 20, 30, 255));
    // The background layer is opaque even at the transparent index; the other is clear.
    assert_eq!(color(pixel(&doc, 0, 0, 1, 0)), rgba(40, 50, 60, 255));
    assert_eq!(pixel(&doc, 1, 0, 1, 0), 0);
    assert_eq!(color(pixel(&doc, 1, 0, 2, 0)), rgba(70, 80, 90, 128));
    assert!(doc.colors.len() >= 257);
}

#[test]
fn an_index_past_the_palette_still_has_a_place() {
    let ase = Ase::new(4, 4, 8).frame(100, vec![plain("L"), cel_raw(0, 0, 0, 1, 1, &[200])]);
    let (doc, _) = open(&ase.bytes());
    let id = pixel(&doc, 0, 0, 0, 0);
    assert!((id as usize) < doc.colors.len());
    // A palette of 255 colors leaves index 255 with no color, and it still has a place.
    let full = palette(255, 0, &[[9, 9, 9, 255]; 255]);
    let pixels = cel_raw(0, 0, 0, 2, 1, &[254, 255]);
    let ase = Ase::new(4, 4, 8).frame(100, vec![plain("L"), full, pixels]);
    let (doc, _) = open(&ase.bytes());
    assert!((pixel(&doc, 0, 0, 1, 0) as usize) < doc.colors.len());
    assert_eq!(
        doc.colors.color(pixel(&doc, 0, 0, 0, 0)),
        rgba(9, 9, 9, 255)
    );
}

#[test]
fn more_warnings_than_are_kept_end_in_a_note() {
    let names: Vec<String> = (0..300).map(|i| format!("t{i}")).collect();
    let list: Vec<(u16, u16, u8, u16, &str)> =
        names.iter().map(|n| (5, 0, 0, 0, n.as_str())).collect();
    let ase = Ase::new(2, 2, 32).frame(100, vec![plain("L"), tags(&list)]);
    let (_, warnings) = open(&ase.bytes());
    // Notes of this kind keep clear of the places held for warnings about lost art.
    assert_eq!(warnings.len(), 193);
    assert_eq!(warnings.last(), Some(&AseWarning::TooMany));
}

#[test]
fn a_negative_z_index_is_noted_too() {
    let c = cel(0, 0, 0, 255, 0, -1, &[1, 0, 1, 0, 1, 2, 3, 255]);
    let (_, warnings) = open(&one_layer(4, 4, vec![c]));
    assert!(has(&warnings, &AseWarning::CelZIndex("L".into())));
    assert!(!warnings
        .iter()
        .any(|w| matches!(w, AseWarning::CelOpacity(_))));
}

#[test]
fn a_palette_chunk_sets_a_range_of_a_larger_palette() {
    let ase = Ase::new(4, 4, 32).frame(
        100,
        vec![
            plain("L"),
            palette(6, 2, &[[1, 2, 3, 255], [4, 5, 6, 255]]),
            palette(6, 4, &[[7, 8, 9, 255], [0, 0, 0, 0]]),
        ],
    );
    let (doc, _) = open(&ase.bytes());
    let colors = doc.colors.colors();
    assert_eq!(
        &colors[1..],
        &[
            0,
            0,
            rgba(1, 2, 3, 255),
            rgba(4, 5, 6, 255),
            rgba(7, 8, 9, 255),
            0
        ]
    );
    assert_eq!(doc.colors.swatches(), &[3, 4, 5]);
}

#[test]
fn old_palettes_are_read_and_the_new_one_wins() {
    let packets = vec![(0u8, vec![[1u8, 2, 3], [4, 5, 6]]), (3, vec![[7, 8, 9]])];
    let old = |kind| Ase::new(2, 2, 32).frame(100, vec![plain("L"), old_palette(kind, &packets)]);
    let (doc, _) = open(&old(0x0004).bytes());
    let colors = doc.colors.colors();
    assert_eq!(&colors[1..4], &[rgba(1, 2, 3, 255), rgba(4, 5, 6, 255), 0]);
    assert_eq!(colors[1 + 5], rgba(7, 8, 9, 255));
    // The six-bit kind spreads 0 to 63 over 0 to 255.
    let six = vec![(0u8, vec![[63u8, 0, 32]])];
    let ase = Ase::new(2, 2, 32).frame(100, vec![plain("L"), old_palette(0x0011, &six)]);
    assert_eq!(
        open(&ase.bytes()).0.colors.colors()[1],
        rgba(255, 0, 130, 255)
    );
    let both = Ase::new(2, 2, 32).frame(
        100,
        vec![
            plain("L"),
            palette(1, 0, &[[9, 9, 9, 255]]),
            old_palette(0x0004, &packets),
        ],
    );
    assert_eq!(
        &open(&both.bytes()).0.colors.colors()[1..],
        &[rgba(9, 9, 9, 255)]
    );
}

#[test]
fn a_zero_count_in_an_old_packet_means_256() {
    let colors = vec![[3u8, 3, 3]; 255];
    let mut data = vec![1, 0, 0, 0];
    data.extend(colors.concat());
    data.extend([3, 3, 3]);
    let ase = Ase::new(2, 2, 32).frame(100, vec![plain("L"), chunk(0x0004, &data)]);
    assert_eq!(open(&ase.bytes()).0.colors.colors().len(), 257);
}

#[test]
fn tags_keep_range_direction_and_repeat() {
    let list = [
        (0, 1, 0, 0, "idle"),
        (1, 2, 1, 3, "run"),
        (2, 2, 2, 1, "bounce"),
    ];
    let ase = Ase::new(2, 2, 32)
        .frame(100, vec![plain("L"), tags(&list)])
        .frame(100, vec![])
        .frame(100, vec![]);
    let (doc, warnings) = open(&ase.bytes());
    assert!(warnings.is_empty(), "{warnings:?}");
    let got: Vec<_> = doc
        .tags
        .iter()
        .map(|t| (t.name.as_str(), t.from, t.to, t.direction, t.repeat))
        .collect();
    assert_eq!(
        got,
        [
            ("idle", 0, 1, Direction::Forward, 0),
            ("run", 1, 2, Direction::Reverse, 3),
            ("bounce", 2, 2, Direction::PingPong, 1),
        ]
    );
}

#[test]
fn odd_tags_are_adjusted_or_left_out_with_a_warning() {
    let long = "n".repeat(40);
    let kept = "n".repeat(32);
    let list = [
        (0, 0, 3, 1000, long.as_str()),
        (0, 5, 0, 0, "past"),
        (1, 0, 0, 0, "backwards"),
    ];
    let ase = Ase::new(2, 2, 32).frame(100, vec![plain("L"), tags(&list)]);
    let (doc, warnings) = open(&ase.bytes());
    assert_eq!(doc.tags.len(), 1);
    let tag = &doc.tags[0];
    assert_eq!(
        (tag.direction, tag.repeat, tag.name.len()),
        (Direction::PingPong, 255, 32)
    );
    for w in [
        AseWarning::TagDirection(kept.clone()),
        AseWarning::TagRepeat(kept.clone()),
        AseWarning::Counted {
            kind: crate::Tally::TagName,
            count: 1,
            first: kept,
            frame: 0,
        },
        AseWarning::TagDropped("past".into()),
        AseWarning::TagDropped("backwards".into()),
    ] {
        assert!(has(&warnings, &w), "{w:?}");
    }
}

#[test]
fn what_is_not_read_is_named_in_a_warning() {
    let chunks = vec![
        plain("L"),
        layer(3, 2, 0, 0, 255, "tiles"),
        layer(3, 7, 0, 0, 255, "?"),
        cel(1, 0, 0, 255, 2, 0, &[0; 30]),
        cel(2, 0, 0, 255, 2, 0, &[0; 30]),
        cel(0, 0, 0, 255, 3, 0, &[0; 30]),
        chunk(0x2006, &[0; 36]),
        chunk(0x2007, &[0; 14]),
        chunk(0x2008, &[0; 12]),
        chunk(0x2016, &[0; 16]),
        chunk(0x2017, &[]),
        chunk(0x2020, &[0; 4]),
        chunk(0x2023, &[0; 4]),
        chunk(0x7777, &[1, 2, 3]),
        cel(0, 0, 0, 255, 9, 0, &[]),
    ];
    let mut ase = Ase::new(2, 2, 32);
    ase.ratio = (2, 1);
    let (doc, warnings) = open(&ase.frame(100, chunks).bytes());
    assert_eq!(doc.layers().len(), 1);
    for what in [
        "drawings on tilemap layers",
        "tilemap cels",
        "cels on layers that are not brought in",
        "cel extra data",
        "the color profile",
        "external files",
        "masks",
        "paths",
        "user data",
        "tilesets that are cut short",
        "chunks of an unknown type",
        "cels of an unknown type",
    ] {
        assert!(
            has(&warnings, &AseWarning::Skipped(what)),
            "{what}: {warnings:?}"
        );
    }
    // A tilemap layer with no tileset in the file cannot be brought in.
    assert!(has(&warnings, &AseWarning::TilemapLeftOut("tiles".into())));
    assert!(has(&warnings, &AseWarning::PixelRatio(2, 1)));
}

#[test]
fn a_layer_of_an_unknown_type_is_counted_but_left_out() {
    let chunks = vec![
        layer(3, 7, 0, 0, 255, "?"),
        plain("real"),
        cel_raw(1, 0, 0, 1, 1, &[1, 1, 1, 255]),
    ];
    let (doc, warnings) = open(&Ase::new(2, 2, 32).frame(100, chunks).bytes());
    assert_eq!(doc.layers().len(), 1);
    assert_eq!(pixel(&doc, 0, 0, 0, 0), rgba(1, 1, 1, 255));
    assert!(has(
        &warnings,
        &AseWarning::Skipped("layers of an unknown type")
    ));
}

#[test]
fn grayscale_files_open_as_rgba_layers_carrying_the_grey_and_its_alpha() {
    let pixels = [5, 255, 200, 128, 0, 0, 255, 7];
    for c in [
        cel_raw(0, 0, 0, 2, 2, &pixels),
        cel_zlib(0, 0, 0, 2, 2, &pixels),
    ] {
        let ase = Ase::new(4, 4, 16).frame(100, vec![plain("L"), c]);
        let (doc, warnings) = open(&ase.bytes());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(doc.layers()[0].mode, ColorMode::Rgba);
        let at = |x, y| pixel(&doc, 0, 0, x, y);
        assert_eq!(at(0, 0), rgba(5, 5, 5, 255));
        assert_eq!(at(1, 0), rgba(200, 200, 200, 128));
        assert_eq!(at(0, 1), rgba(0, 0, 0, 0));
        assert_eq!(at(1, 1), rgba(255, 255, 255, 7));
    }
}

#[test]
fn a_palette_in_a_later_frame_is_noted() {
    let ase = Ase::new(2, 2, 32)
        .frame(100, vec![plain("L"), palette(1, 0, &[[1, 1, 1, 255]])])
        .frame(100, vec![palette(1, 0, &[[2, 2, 2, 255]])]);
    let (doc, warnings) = open(&ase.bytes());
    assert_eq!(doc.colors.colors()[1], rgba(2, 2, 2, 255));
    assert!(has(&warnings, &AseWarning::PaletteChanges));
}

#[test]
fn warnings_are_told_once() {
    let c = |l| cel(l, 0, 0, 255, 0, 4, &[1, 0, 1, 0, 1, 2, 3, 255]);
    let ase = Ase::new(2, 2, 32)
        .frame(
            100,
            vec![plain("L"), c(0), chunk(0x2020, &[]), chunk(0x2020, &[])],
        )
        .frame(100, vec![c(0), chunk(0x2020, &[])]);
    let (_, warnings) = open(&ase.bytes());
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

#[test]
fn warnings_have_words() {
    let text = AseWarning::Skipped("slices").to_string();
    assert_eq!(text, "Left out: slices");
    let blend = AseWarning::LayerBlend {
        layer: "x".into(),
        mode: 19,
        shown_as: BlendMode::Normal,
    };
    assert!(blend.to_string().contains("Normal"));
}

#[test]
fn the_rich_files_import_with_what_they_hold() {
    let (doc, warnings) = open(&rich());
    assert_eq!(doc.layers().len(), 4);
    assert_eq!(doc.frames.len(), 3);
    assert_eq!(doc.tags.len(), 2);
    // Its color dodge layer is kept as it is now, with Aseprite-compatible blending.
    assert!(warnings.len() >= 3, "{warnings:?}");
    let top = doc.layers().iter().find(|l| l.name == "top").expect("top");
    assert_eq!(top.blend, BlendMode::ColorDodge);
    assert_eq!(doc.blending, pikslar_core::Blending::Aseprite);
    let (doc, _) = open(&rich_indexed());
    assert!(doc.layers().iter().all(|l| l.mode == ColorMode::Palette));
}

#[test]
fn a_read_asked_to_stop_stops_between_frames_and_keeps_nothing() {
    let pic = rgba_picture(2, 2, 10);
    let mut ase = Ase::new(4, 3, 32).frame(100, vec![plain("ink"), cel_raw(0, 0, 0, 2, 2, &pic)]);
    for _ in 0..5 {
        ase = ase.frame(100, vec![cel_raw(0, 0, 0, 2, 2, &pic)]);
    }
    let bytes = ase.bytes();
    let asked = std::cell::Cell::new(0);
    let stop_at_third = || {
        asked.set(asked.get() + 1);
        asked.get() >= 3
    };
    let stopped = crate::ase_read::import_ase_until(&bytes, &stop_at_third);
    assert!(matches!(stopped, Err(FormatError::Stopped)), "{stopped:?}");
    assert_eq!(asked.get(), 3, "asked once a frame, and no more once told");
    let whole = crate::ase_read::import_ase_until(&bytes, &|| false).expect("reads");
    assert_eq!(whole.0.frames.len(), 6);
    assert!(FormatError::Stopped.to_string().contains("stopped"));
}
