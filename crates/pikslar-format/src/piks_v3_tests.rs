//! Version 3: round trips, the fixed write order, and every rule in `docs/format-v3.md`.

use super::*;
use crate::piks::{load, load_noted, load_recovery, save};
use pikslar_core::tile::Tile;
use pikslar_core::{rgba, Grid, LayerCmd, PaletteCmd};

fn ink(width: usize, height: usize, at: &[(usize, usize, u32)]) -> Grid {
    let mut cells = vec![0; width * height];
    at.iter().for_each(|&(x, y, v)| cells[y * width + x] = v);
    Grid::from_cells(width, height, &cells)
}

/// Two layers, three frames, a hold, a shared drawing, tags, a swatch and a carried section.
fn sample() -> Document {
    let mut doc = Document::new(40, 24, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Palette).expect("room");
    let red = rgba(200, 30, 10, 255);
    doc.set_cel(0, 0, ink(40, 24, &[(0, 0, red), (39, 23, red)]))
        .expect("fits");
    doc.add_frame(1, true).expect("room");
    doc.add_frame(2, false).expect("room");
    doc.set_cel(0, 2, ink(40, 24, &[(20, 10, red)]))
        .expect("fits");
    let id = Arc::make_mut(&mut doc.colors)
        .add_swatch(rgba(1, 2, 3, 255))
        .expect("room");
    doc.set_cel(1, 1, ink(40, 24, &[(5, 5, id)])).expect("fits");
    doc.hold_across(1, 1, 1, 2).expect("frames");
    doc.add_tag("walk", 0, 1).expect("valid");
    doc.layers_mut()[1].name = "Shading é".into();
    doc.layers_mut()[1].blend = BlendMode::Multiply;
    doc.layers_mut()[1].opacity = 90;
    doc.layers_mut()[0].visible = false;
    doc.layers_mut()[1].locked = true;
    doc.foreign.push(pikslar_core::ForeignSection {
        tag: *b"xtra",
        data: Arc::from(&b"carried"[..]),
    });
    doc
}

fn assert_same(a: &Document, b: &Document) {
    assert_eq!((a.width, a.height), (b.width, b.height));
    assert_eq!(a.frames, b.frames);
    assert_eq!(a.colors.colors(), b.colors.colors());
    assert_eq!(a.colors.swatches(), b.colors.swatches());
    assert_eq!(a.layers(), b.layers());
    assert_eq!(a.tags, b.tags);
    assert_eq!(a.foreign, b.foreign);
    for (x, y) in a.layers().iter().zip(b.layers()) {
        assert!(x.links().map(|l| l != 0).eq(y.links().map(|l| l != 0)));
        assert_eq!(x.persisted_id(), y.persisted_id());
    }
}

#[test]
fn projects_round_trip_exactly() {
    let doc = sample();
    let (back, notes) = load_noted(&encode(&doc, &[])).expect("valid");
    assert_same(&doc, &back);
    assert!(notes.can_save_over(), "{notes:?}");
}

#[test]
fn a_file_just_opened_saves_the_same_bytes() {
    let bytes = encode(&sample(), &[]);
    assert_eq!(encode(&load(&bytes).expect("valid"), &[]), bytes);
}

#[test]
fn a_version_two_file_upgrades_with_the_same_pixels() {
    let doc = sample();
    let from_v2 = load(&save(&doc)).expect("version 2");
    let upgraded = load(&encode(&from_v2, &[])).expect("version 3");
    assert_same(&from_v2, &upgraded);
}

#[test]
fn shared_drawings_and_holds_stay_shared() {
    let back = load(&encode(&sample(), &[])).expect("valid");
    let layer = &back.layers()[1];
    assert!(layer.cel(1).same_image(layer.cel(2)));
    assert_ne!(layer.link(1), 0);
    assert_eq!(layer.link(1), layer.link(2));
    assert!(
        back.layers()[0].cel(0).same_image(back.layers()[0].cel(1)),
        "copied frame"
    );
}

#[test]
fn saved_layer_numbers_survive_a_round_trip() {
    let mut doc = sample();
    doc.keep_persisted_ids(&[9, 4]).expect("distinct");
    let back = load(&encode(&doc, &[])).expect("valid");
    let ids: Vec<u32> = back
        .layers()
        .iter()
        .map(|l| l.persisted_id().get())
        .collect();
    assert_eq!(ids, [9, 4]);
}

#[test]
fn only_the_table_a_drawing_uses_is_stored() {
    let mut doc = Document::new(4096, 4096, ColorMode::Rgba).expect("valid");
    doc.set_cel(0, 0, ink(4096, 4096, &[(4000, 4000, 7)]))
        .expect("fits");
    assert!(
        encode(&doc, &[]).len() < 200,
        "one tile, not a full canvas table"
    );
}

#[test]
fn a_hold_whose_frames_disagree_keeps_every_drawing() {
    let mut doc = sample();
    let other = doc.layers()[0].cel(0).clone();
    doc.put_cel(1, 2, other).expect("frame");
    let back = load(&encode(&doc, &[])).expect("valid");
    assert_eq!(
        back.layers()[1].cel(2).to_cells(40, 24),
        doc.layers()[1].cel(2).to_cells(40, 24)
    );
    assert_eq!(back.layers()[1].link(2), 0);
}

#[test]
fn recovery_copies_keep_floating_pixels_beside_the_document() {
    let doc = sample();
    let bytes = encode(&doc, &[(crate::piks::FLOATING, Vec::new())]);
    let back = load_recovery(&bytes).expect("document");
    assert!(
        back.floating_damaged,
        "an empty floating section is reported, not fatal"
    );
    assert_same(&doc, &back.doc);
}

#[test]
fn names_are_cut_on_a_character_boundary() {
    let mut doc = sample();
    doc.layers_mut()[0].name = "é".repeat(3000);
    let back = load(&encode(&doc, &[])).expect("valid");
    let name = &back.layers()[0].name;
    assert!(name.len() <= MAX_NAME_BYTES && name.chars().all(|c| c == 'é'));
    assert_eq!(name.len(), 4096);
}

// Files written by hand, to test what the writer never produces.

struct Raw {
    features: u32,
    width: u32,
    height: u32,
    frames: usize,
    layers: Vec<Vec<u8>>,
    images: Vec<Vec<u8>>,
    cels: Vec<Vec<u8>>,
    sections: Vec<([u8; 4], Vec<u8>)>,
    trailing: Vec<u8>,
}

fn layer_record(id: u32, visible: u8, blend: u8, mode: u8, name: &str, extra: &[u8]) -> Vec<u8> {
    let mut r = Out(Vec::new());
    r.u32(id);
    r.u8(visible);
    r.u8(255);
    r.u8(blend);
    r.u8(mode);
    r.u16(name.len() as u16);
    r.0.extend_from_slice(name.as_bytes());
    r.0.extend_from_slice(extra);
    let mut out = Out(Vec::new());
    out.u16(r.0.len() as u16);
    out.0.extend_from_slice(&r.0);
    out.0
}

/// An image of solid tiles, 0 meaning no tile.
fn image_record(kind: u8, x0: i16, y0: i16, tw: u16, th: u16, solids: &[u32]) -> Vec<u8> {
    let mut out = Out(Vec::new());
    out.u8(kind);
    out.u16(x0 as u16);
    out.u16(y0 as u16);
    out.u16(tw);
    out.u16(th);
    for &v in solids {
        write_tile(&mut out, (v != 0).then_some(&Tile::Solid(v)));
    }
    out.0
}

fn cel_record(kind: u8, image: u32, x: i16, y: i16, link: u32, extra: &[u8]) -> Vec<u8> {
    let mut out = Out(Vec::new());
    out.u8((CEL_FIELDS + extra.len()) as u8);
    out.u8(kind);
    out.u32(image);
    out.u16(x as u16);
    out.u16(y as u16);
    out.u32(link);
    out.0.extend_from_slice(extra);
    out.0
}

/// A 32 by 32 project of one layer and one frame, drawn in its first tile.
fn basic() -> Raw {
    Raw {
        features: 0,
        width: 32,
        height: 32,
        frames: 1,
        layers: vec![layer_record(1, 1, 0, 1, "a", &[])],
        images: vec![image_record(0, 0, 0, 1, 1, &[5])],
        cels: vec![cel_record(0, 0, 0, 0, 0, &[])],
        sections: Vec::new(),
        trailing: Vec::new(),
    }
}

impl Raw {
    fn bytes(&self) -> Vec<u8> {
        let mut out = Out(Vec::new());
        out.0.extend_from_slice(MAGIC);
        out.u32(VERSION);
        out.u32(self.features);
        out.u32(self.width);
        out.u32(self.height);
        out.u32s(&[]);
        out.u32s(&[]);
        out.u32s(&vec![100; self.frames]);
        out.u32(self.layers.len() as u32);
        self.layers.iter().for_each(|r| out.0.extend_from_slice(r));
        out.u32(self.images.len() as u32);
        self.images.iter().for_each(|r| out.0.extend_from_slice(r));
        self.cels.iter().for_each(|r| out.0.extend_from_slice(r));
        out.u32(self.sections.len() as u32);
        for (tag, data) in &self.sections {
            out.0.extend_from_slice(tag);
            out.u32(data.len() as u32);
            out.0.extend_from_slice(data);
        }
        out.0.extend_from_slice(&self.trailing);
        let crc = crc32(&[&out.0]);
        out.u32(crc);
        out.0
    }

    fn with(mut self, change: impl FnOnce(&mut Raw)) -> Raw {
        change(&mut self);
        self
    }
}

fn refused(raw: Raw) -> FormatError {
    match load(&raw.bytes()) {
        Err(e) => e,
        Ok(_) => panic!("loaded a file that should be refused"),
    }
}

fn is_unsupported(e: FormatError) -> bool {
    matches!(e, FormatError::Unsupported(_))
}

#[test]
fn a_hand_written_file_loads() {
    let (doc, notes) = load_noted(&basic().bytes()).expect("valid");
    assert_eq!(doc.layers()[0].cel(0).get(3, 3), 5);
    assert!(notes.can_save_over());
}

#[test]
fn newer_versions_and_required_features_are_refused() {
    let mut newer = basic().bytes();
    newer[4..8].copy_from_slice(&4u32.to_le_bytes());
    let end = newer.len() - 4;
    let crc = crc32(&[&newer[..end]]);
    newer[end..].copy_from_slice(&crc.to_le_bytes());
    assert!(matches!(load(&newer), Err(FormatError::Unsupported(_))));
    assert!(is_unsupported(refused(
        basic().with(|r| r.features = 1 << 31)
    )));
}

#[test]
fn unknown_kinds_and_color_modes_are_refused() {
    assert!(is_unsupported(refused(
        basic().with(|r| { r.images[0] = image_record(2, 0, 0, 1, 1, &[5]) })
    )));
    assert!(is_unsupported(refused(
        basic().with(|r| { r.cels[0] = cel_record(1, 0, 0, 0, 0, &[]) })
    )));
    assert!(is_unsupported(refused(basic().with(|r| {
        r.layers[0] = layer_record(1, 1, 0, 2, "a", &[])
    }))));
}

#[test]
fn records_shorter_than_their_fields_are_refused() {
    let mut short_layer = layer_record(1, 1, 0, 1, "", &[]);
    short_layer.truncate(short_layer.len() - 1);
    short_layer[0] -= 1;
    refused(basic().with(|r| r.layers[0] = short_layer));
    let mut long_name = layer_record(1, 1, 0, 1, "abc", &[]);
    long_name[2 + 8] = 9;
    refused(basic().with(|r| r.layers[0] = long_name));
    let mut short_cel = cel_record(0, 0, 0, 0, 0, &[]);
    short_cel.truncate(short_cel.len() - 1);
    short_cel[0] -= 1;
    refused(basic().with(|r| r.cels[0] = short_cel));
}

#[test]
fn longer_records_and_unknown_values_are_kept_and_protect_the_file() {
    let cases = [
        basic().with(|r| r.layers[0] = layer_record(1, 1, 0, 1, "a", &[1, 0, 0, 7])),
        basic().with(|r| r.cels[0] = cel_record(0, 0, 0, 0, 0, &[9])),
        basic().with(|r| r.layers[0] = layer_record(1, 2, 0, 1, "a", &[])),
        // 19 is the first blend number no version gives a meaning.
        basic().with(|r| r.layers[0] = layer_record(1, 1, 19, 1, "a", &[])),
    ];
    for raw in cases {
        let (doc, notes) = load_noted(&raw.bytes()).expect("kept");
        assert_eq!(doc.layers()[0].cel(0).get(0, 0), 5);
        assert!(doc.layers()[0].visible);
        assert_eq!(doc.layers()[0].blend, BlendMode::Normal);
        assert!(!notes.can_save_over() && notes.message().is_some());
    }
}

#[test]
fn every_blend_mode_is_saved_by_its_number_and_read_back() {
    // The numbers files store, 0 to 18. Older files depend on them, so they never change.
    let numbered = [
        "Normal",
        "Multiply",
        "Screen",
        "Overlay",
        "Darken",
        "Lighten",
        "Addition",
        "Subtract",
        "Difference",
    ];
    assert_eq!(BlendMode::PIKSLAR.map(|m| m.name()), numbered);
    assert_eq!(BlendMode::ALL[..9], BlendMode::PIKSLAR);
    assert_eq!(BlendMode::ALL[18], BlendMode::Divide);
    let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
    for (n, &mode) in BlendMode::ALL.iter().enumerate() {
        doc.layers_mut()[0].blend = mode;
        let bytes = encode(&doc, &[]);
        let (back, notes) = load_noted(&bytes).expect("valid");
        assert_eq!(back.layers()[0].blend, mode);
        assert!(notes.can_save_over(), "{mode:?}");
        let raw = basic().with(|r| r.layers[0] = layer_record(1, 1, n as u8, 1, "a", &[]));
        assert_eq!(
            load(&raw.bytes()).expect("valid").layers()[0].blend,
            mode,
            "number {n}"
        );
    }
}

#[test]
fn a_lock_is_one_byte_after_the_name_written_only_when_set() {
    let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
    let unlocked = encode(&doc, &[]);
    doc.layers_mut()[0].locked = true;
    let locked = encode(&doc, &[]);
    assert_eq!(
        locked.len(),
        unlocked.len() + 1,
        "one byte, and none when unlocked"
    );
    let (back, notes) = load_noted(&locked).expect("valid");
    assert!(
        back.layers()[0].locked && notes.can_save_over(),
        "{notes:?}"
    );
    let (back, notes) = load_noted(&unlocked).expect("valid");
    assert!(!back.layers()[0].locked && notes.can_save_over());

    let raw = basic().with(|r| r.layers[0] = layer_record(1, 1, 0, 1, "a", &[1]));
    let (doc, notes) = load_noted(&raw.bytes()).expect("valid");
    assert!(doc.layers()[0].locked && notes.can_save_over());
    let raw = basic().with(|r| r.layers[0] = layer_record(1, 1, 0, 1, "a", &[5]));
    let (doc, notes) = load_noted(&raw.bytes()).expect("kept");
    assert!(doc.layers()[0].locked, "the flag known is kept");
    assert!(!notes.can_save_over(), "an unknown flag protects the file");
}

#[test]
fn image_tables_out_of_shape_or_reach_are_refused() {
    let shapes = [
        image_record(0, 0, 0, 0, 1, &[]),
        image_record(0, 0, 0, 1, 0, &[]),
        image_record(0, -2049, 0, 1, 1, &[5]),
        image_record(0, 2047, 0, 2, 1, &[5, 5]),
        image_record(0, 0, i16::MAX, 1, u16::MAX, &[]),
    ];
    for image in shapes {
        refused(basic().with(|r| r.images[0] = image));
    }
}

#[test]
fn a_table_larger_than_the_file_is_refused_before_it_is_made() {
    let huge = image_record(0, 0, 0, 512, 512, &[]);
    let raw = basic().with(|r| {
        (r.width, r.height) = (8192, 8192);
        r.images[0] = huge;
    });
    assert!(matches!(refused(raw), FormatError::Corrupt(_)));
    // As far as an image can reach, past the canvas on every side: still refused before the
    // table is made, because the file is too short to hold it.
    let beyond = image_record(0, -2048, -2048, 4096, 4096, &[]);
    assert!(matches!(
        refused(basic().with(|r| r.images[0] = beyond)),
        FormatError::Corrupt(_)
    ));
}

#[test]
fn image_references_must_exist_and_every_image_be_used() {
    refused(basic().with(|r| r.cels[0] = cel_record(0, 1, 0, 0, 0, &[])));
    refused(basic().with(|r| r.images.push(image_record(0, 0, 0, 1, 1, &[6]))));
    refused(basic().with(|r| r.images.clear()));
}

#[test]
fn layer_numbers_must_be_distinct_and_in_range() {
    let two = |a: u32, b: u32| {
        basic().with(|r| {
            r.layers = vec![
                layer_record(a, 1, 0, 1, "a", &[]),
                layer_record(b, 1, 0, 1, "b", &[]),
            ];
            r.cels.push(cel_record(0, 0, 0, 0, 0, &[]));
        })
    };
    assert!(load(&two(1, 2).bytes()).is_ok());
    refused(two(3, 3));
    refused(two(0, 1));
    refused(two(1, MAX_PERSISTED_ID + 1));
    assert!(load(&two(1, MAX_PERSISTED_ID).bytes()).is_ok());
}

#[test]
fn a_layer_added_after_the_highest_number_still_saves_a_file_that_opens() {
    let file = basic().with(|r| {
        r.layers = vec![layer_record(MAX_PERSISTED_ID, 1, 0, 1, "top", &[])];
    });
    let mut doc = load(&file.bytes()).expect("opens");
    doc.add_layer(1, pikslar_core::ColorMode::Rgba)
        .expect("room");
    let ids: Vec<u32> = doc
        .layers()
        .iter()
        .map(|l| l.persisted_id().get())
        .collect();
    assert!(
        ids.iter().all(|&id| (1..=MAX_PERSISTED_ID).contains(&id)),
        "{ids:?}"
    );
    load(&save(&doc)).expect("its own save opens");
}

#[test]
fn offsets_and_drawings_beyond_the_canvas_load_exactly_and_save_again() {
    // The basic image is one tile of 5 at local tile (0, 0).
    let moved = basic().with(|r| r.cels[0] = cel_record(0, 0, -20, 40, 0, &[]));
    let beyond = basic().with(|r| r.images[0] = image_record(0, -1, 2, 1, 1, &[5]));
    let far = basic().with(|r| {
        r.images[0] = image_record(0, -2048, 2047, 1, 1, &[5]);
        r.cels[0] = cel_record(0, 0, 24_576, -24_576, 0, &[]);
    });
    let cases = [
        (moved, (-20, 40), [(-20, 40), (-5, 55)], (0, 0)),
        (beyond, (0, 0), [(-16, 32), (-1, 47)], (0, 31)),
        (
            far,
            (24_576, -24_576),
            [(-8192, 8176), (-8177, 8191)],
            (5, 5),
        ),
    ];
    for (raw, at, [first, last], blank) in cases {
        let doc = load(&raw.bytes()).expect("loads");
        let cel = doc.layers()[0].cel(0);
        assert_eq!(cel.position(), (at.0 as i16, at.1 as i16));
        assert_eq!((cel.get(first.0, first.1), cel.get(last.0, last.1)), (5, 5));
        assert_eq!(cel.get(blank.0, blank.1), 0);
        let again = load(&save(&doc)).expect("its own save opens");
        assert_eq!(again.layers(), doc.layers(), "at {at:?}");
    }
}

#[test]
fn a_frame_placed_past_the_offset_range_is_refused() {
    for (x, y) in [(24_577, 0), (0, -24_577), (i16::MIN, 0), (0, i16::MAX)] {
        let raw = basic().with(|r| r.cels[0] = cel_record(0, 0, x, y, 0, &[]));
        assert!(matches!(refused(raw), FormatError::Corrupt(_)), "{x}, {y}");
    }
}

#[test]
fn tiles_that_draw_nothing_are_no_tiles() {
    let raw = basic().with(|r| {
        let mut image = image_record(0, 0, 0, 2, 1, &[5]);
        let mut blank = Out(Vec::new());
        write_tile(&mut blank, Some(&Tile::Solid(0)));
        image.extend_from_slice(&blank.0);
        r.images[0] = image;
    });
    let doc = load(&raw.bytes()).expect("valid");
    assert_eq!(doc.layers()[0].cel(0).image().table(), (0, 0, 1, 1));
}

#[test]
fn bad_holds_are_dropped_and_every_drawing_kept() {
    let across_modes = basic().with(|r| {
        r.layers.push(layer_record(2, 1, 0, 0, "b", &[]));
        r.frames = 2;
        r.cels = vec![
            cel_record(0, 0, 0, 0, 4, &[]),
            cel_record(0, 0, 0, 0, 0, &[]),
            cel_record(0, 0, 0, 0, 4, &[]),
            cel_record(0, 0, 0, 0, 0, &[]),
        ];
    });
    let different_drawings = basic().with(|r| {
        r.frames = 2;
        r.images.push(image_record(0, 0, 0, 1, 1, &[6]));
        r.cels = vec![
            cel_record(0, 0, 0, 0, 4, &[]),
            cel_record(0, 1, 0, 0, 4, &[]),
        ];
    });
    for raw in [across_modes, different_drawings] {
        let (doc, notes) = load_noted(&raw.bytes()).expect("kept");
        assert!(doc.layers().iter().all(|l| l.links().all(|id| id == 0)));
        assert!(!notes.can_save_over());
    }
    let (doc, _) = load_noted(
        &basic()
            .with(|r| {
                r.frames = 2;
                r.images.push(image_record(0, 0, 0, 1, 1, &[6]));
                r.cels = vec![
                    cel_record(0, 0, 0, 0, 4, &[]),
                    cel_record(0, 1, 0, 0, 4, &[]),
                ];
            })
            .bytes(),
    )
    .expect("kept");
    assert_eq!(
        doc.layers()[0].cel(1).get(0, 0),
        6,
        "the second drawing is not replaced"
    );
}

#[test]
fn a_link_joining_a_reference_and_a_paint_layer_is_dropped() {
    let raw = basic().with(|r| {
        r.features = REFERENCES;
        let extra = [0, REFERENCE_KIND];
        r.layers.push(layer_record(2, 1, 0, 1, "r", &extra));
        r.cels = vec![
            cel_record(0, 0, 0, 0, 3, &[]),
            cel_record(0, 0, 0, 0, 3, &[]),
        ];
    });
    let (doc, notes) = load_noted(&raw.bytes()).expect("kept");
    assert!(doc.layers().iter().all(|l| l.links().all(|id| id == 0)));
    assert!(!notes.can_save_over());
    assert_eq!(doc.layers()[0].cel(0).get(0, 0), 5, "every pixel kept");
    assert_eq!(doc.layers()[1].kind, LayerKind::Reference);
}

#[test]
fn a_link_across_layers_of_one_mode_is_kept() {
    let raw = basic().with(|r| {
        r.layers.push(layer_record(2, 1, 0, 1, "b", &[]));
        r.frames = 2;
        r.cels = vec![
            cel_record(0, 0, 0, 0, 1, &[]),
            cel_record(0, 0, 0, 0, 0, &[]),
            cel_record(0, 0, 0, 0, 0, &[]),
            cel_record(0, 0, 3, 0, 1, &[]),
        ];
    });
    let (doc, notes) = load_noted(&raw.bytes()).expect("kept");
    assert!(notes.can_save_over());
    let (a, b) = (doc.layers()[0].link(0), doc.layers()[1].link(1));
    assert!(a != 0 && a == b, "one link on both layers");
    assert!(doc.layers()[0].cel(0).same_image(doc.layers()[1].cel(1)));
    assert_eq!(doc.layers()[1].cel(1).position(), (3, 0));
    let back = load(&save(&doc)).expect("opens");
    assert_eq!(back.layers(), doc.layers(), "written back as read");
}

#[test]
fn a_hold_of_one_frame_is_none_and_gaps_are_allowed() {
    let raw = basic().with(|r| {
        r.frames = 3;
        r.cels = vec![
            cel_record(0, 0, 0, 0, 8, &[]),
            cel_record(0, 0, 0, 0, 0, &[]),
            cel_record(0, 0, 0, 0, 8, &[]),
        ];
    });
    let doc = load(&raw.bytes()).expect("valid");
    let layer = &doc.layers()[0];
    assert!(layer.link(0) != 0 && layer.link(0) == layer.link(2));
    let lone = basic().with(|r| r.cels[0] = cel_record(0, 0, 0, 0, 8, &[]));
    assert_eq!(load(&lone.bytes()).expect("valid").layers()[0].link(0), 0);
}

#[test]
fn a_name_too_long_to_keep_is_cut_and_protects_the_file() {
    let long = "x".repeat(MAX_NAME_BYTES + 10);
    let raw = basic().with(|r| r.layers[0] = layer_record(1, 1, 0, 1, &long, &[]));
    let (doc, notes) = load_noted(&raw.bytes()).expect("kept");
    assert_eq!(doc.layers()[0].name.len(), MAX_NAME_BYTES);
    assert!(!notes.can_save_over());
    let mut not_text = layer_record(1, 1, 0, 1, "ab", &[]);
    let at = not_text.len() - 1;
    not_text[at] = 0xff;
    let (_, notes) = load_noted(&basic().with(|r| r.layers[0] = not_text).bytes()).expect("kept");
    assert!(!notes.can_save_over());
}

#[test]
fn many_broken_holds_are_checked_quickly() {
    // Every link joins 32 layers of both color modes.
    let raw = basic().with(|r| {
        r.frames = 4096;
        r.layers = (1..=64)
            .map(|id| layer_record(id, 1, 0, (id % 2) as u8, "l", &[]))
            .collect();
        r.cels = (0..64u32)
            .flat_map(|layer| (0..4096u32).map(move |f| (layer, f)))
            .map(|(layer, f)| cel_record(0, 0, 0, 0, if layer < 32 { f + 1 } else { 0 }, &[]))
            .collect();
    });
    let started = std::time::Instant::now();
    let (_, notes) = load_noted(&raw.bytes()).expect("kept");
    assert!(!notes.can_save_over());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn bytes_after_the_last_section_are_refused() {
    refused(basic().with(|r| r.trailing = vec![0]));
}

#[test]
fn a_link_section_in_version_three_is_ignored() {
    let raw = basic().with(|r| r.sections.push((*b"LINK", vec![1, 2, 3])));
    let (doc, notes) = load_noted(&raw.bytes()).expect("valid");
    assert!(notes.can_save_over() && doc.foreign.is_empty());
}

#[test]
fn every_truncation_and_flipped_bit_is_rejected() {
    let bytes = encode(&sample(), &[]);
    for cut in (0..bytes.len()).step_by(5) {
        assert!(load(&bytes[..cut]).is_err(), "cut at {cut}");
    }
    for i in (0..bytes.len()).step_by(3) {
        let mut bad = bytes.clone();
        bad[i] ^= 0x04;
        assert!(load(&bad).is_err(), "flip at {i}");
    }
}

#[test]
fn hostile_contents_with_a_valid_checksum_never_panic() {
    let bytes = encode(&sample(), &[]);
    let mut x = 11u32;
    for _ in 0..5_000 {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut bad = bytes.clone();
        let at = (x >> 8) as usize % (bytes.len() - 4);
        bad[at] = (x >> 3) as u8;
        let end = bad.len() - 4;
        let crc = crc32(&[&bad[..end]]);
        bad[end..].copy_from_slice(&crc.to_le_bytes());
        if let Ok(doc) = load(&bad) {
            let again = encode(&doc, &[]);
            assert!(load(&again).is_ok(), "what loads saves and loads again");
        }
    }
}

#[test]
fn edits_made_through_the_editor_round_trip() {
    let mut e = pikslar_core::Editor::new(sample());
    e.apply(PaletteCmd::AddSwatch {
        color: rgba(4, 5, 6, 255),
    })
    .expect("room");
    e.apply(LayerCmd::Add {
        at: 0,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    let doc = e.doc().clone();
    assert_same(&doc, &load(&encode(&doc, &[])).expect("valid"));
}

#[test]
fn holds_come_back_numbered_as_they_were() {
    let mut doc = Document::new(4, 4, pikslar_core::ColorMode::Rgba).expect("valid");
    doc.add_layer(1, pikslar_core::ColorMode::Rgba)
        .expect("room");
    for _ in 0..4 {
        doc.add_frame(0, false).expect("room");
    }
    doc.set_holds(0, &[2, 2, 1, 1, 2]).expect("one per frame");
    doc.set_holds(1, &[0, 3, 3, 1, 1]).expect("one per frame");
    let before: Vec<Vec<u32>> = doc.layers().iter().map(|l| l.links().collect()).collect();
    let back = load(&save(&doc)).expect("opens");
    let after: Vec<Vec<u32>> = back.layers().iter().map(|l| l.links().collect()).collect();
    assert_eq!(after, before);
}

fn read_record(bytes: &[u8], spent: &mut usize) -> Result<Image, FormatError> {
    let mut input = In {
        data: bytes,
        pos: 0,
    };
    read_image(&mut input, spent, None).map(|(_, image)| image)
}

#[test]
fn a_table_as_wide_as_an_image_can_reach_is_read_and_charged_for_its_size() {
    let reach = pikslar_core::MAX_TILE_REACH as i16;
    let width = 2 * reach as u16;
    let mut solids = vec![0; width as usize];
    solids[width as usize - 1] = 0xff12_3456;
    let mut spent = 0;
    let record = image_record(0, -reach, 3, width, 1, &solids);
    let image = read_record(&record, &mut spent).expect("within reach");
    let table = width as usize * std::mem::size_of::<Cell>();
    assert!(spent >= IMAGE_RECORD_CHARGE + table, "charged {spent}");
    assert_eq!(image.get(32_767, 3 * 16 + 15), 0xff12_3456);
    assert_eq!(image.get(-32_768, 3 * 16), 0);
}

#[test]
fn a_table_beyond_what_is_left_of_the_budget_is_refused() {
    let mut spent = crate::piks::MAX_DECODED_BYTES - IMAGE_RECORD_CHARGE;
    let record = image_record(0, -40, 7, 64, 1, &[0; 64]);
    assert!(matches!(
        read_record(&record, &mut spent),
        Err(FormatError::FileTooLarge)
    ));
}
