//! Tests for saving and loading projects: round trips, damaged files, sections and recovery copies.

use super::*;
use pikslar_core::PaletteCmd;
use pikslar_core::{DrawCmd, FloatingCmd, SelectionCmd};

#[test]
fn memory_a_project_needs_is_refused_before_it_is_allocated() {
    let mut spent = 0;
    assert!(charge(&mut spent, MAX_DECODED_BYTES).is_ok());
    assert!(matches!(
        charge(&mut spent, 1),
        Err(FormatError::FileTooLarge)
    ));
    let mut huge = usize::MAX - 1;
    assert!(charge(&mut huge, 16).is_err(), "must not wrap around");
}
use pikslar_core::{rgba, Clip, Editor, Mask, SelectOp, Tool};

fn sample() -> Document {
    let mut e = Editor::new(Document::new(40, 24, ColorMode::Palette).expect("valid"));
    e.color = rgba(200, 30, 10, 255);
    e.apply(DrawCmd::Begin {
        tool: Tool::Line,
        x: 0,
        y: 0,
    })
    .expect("begin");
    e.apply(DrawCmd::To { x: 39, y: 23 }).expect("drag");
    e.apply(DrawCmd::Commit).expect("commit");
    e.apply(PaletteCmd::AddSwatch {
        color: rgba(1, 2, 3, 255),
    })
    .expect("room");
    e.apply(pikslar_core::LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    e.layer = 1;
    e.color = rgba(9, 8, 7, 128);
    e.apply(DrawCmd::Begin {
        tool: Tool::Fill,
        x: 3,
        y: 3,
    })
    .expect("begin");
    e.apply(pikslar_core::FrameCmd::Add { at: 1, copy: true })
        .expect("room");
    e.apply(pikslar_core::FrameCmd::Retime {
        from: 1,
        durations: vec![250],
    })
    .expect("timing");
    e.apply(pikslar_core::LayerCmd::SetBlend {
        layer: e.layer_id(1).expect("layer"),
        blend: BlendMode::Multiply,
    })
    .expect("set");
    e.apply(pikslar_core::LayerCmd::SetOpacity {
        layer: e.layer_id(1).expect("layer"),
        opacity: 90,
    })
    .expect("set");
    e.apply(pikslar_core::LayerCmd::SetVisible {
        layer: e.layer_id(1).expect("layer"),
        visible: false,
    })
    .expect("set");
    let mut doc = e.doc().clone();
    doc.layers_mut()[1].name = "Shading é".into();
    doc
}

/// A version 2 file, as earlier builds wrote.
fn save_v2(doc: &Document) -> Vec<u8> {
    super::v2_writer::encode_v2(doc, &[]).expect("a document version 2 can hold")
}

fn assert_same(a: &Document, b: &Document) {
    assert_eq!((a.width, a.height), (b.width, b.height));
    assert_eq!(a.frames, b.frames);
    assert_eq!(a.colors.colors(), b.colors.colors());
    assert_eq!(a.colors.swatches(), b.colors.swatches());
    assert_eq!(a.layers(), b.layers());
}

#[test]
fn projects_round_trip_exactly() {
    let doc = sample();
    assert_same(&doc, &load(&save(&doc)).expect("valid"));
    assert_same(&doc, &load(&save_v2(&doc)).expect("version 2"));
}

#[test]
fn shared_frames_stay_shared() {
    let doc = load(&save(&sample())).expect("valid");
    assert!(doc.layers()[0].cel(0).same_image(doc.layers()[0].cel(1)));
}

#[test]
fn every_truncation_is_rejected() {
    let bytes = save(&sample());
    for cut in (0..bytes.len()).step_by(7) {
        assert!(load(&bytes[..cut]).is_err(), "cut at {cut}");
    }
}

#[test]
fn any_flipped_bit_is_rejected() {
    let bytes = save(&sample());
    for i in (0..bytes.len()).step_by(11) {
        let mut bad = bytes.clone();
        bad[i] ^= 0x10;
        assert!(load(&bad).is_err(), "flip at {i}");
    }
}

/// Recomputes the checksum, so parsing itself is what gets tested.
fn resealed(mut bytes: Vec<u8>) -> Vec<u8> {
    let end = bytes.len() - 4;
    let crc = crc32(&[&bytes[..end]]);
    bytes[end..].copy_from_slice(&crc.to_le_bytes());
    bytes
}

#[test]
fn hostile_contents_with_a_valid_checksum_never_panic() {
    let bytes = save(&sample());
    let mut x = 7u32;
    for _ in 0..3_000 {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut bad = bytes.clone();
        let at = (x >> 8) as usize % (bytes.len() - 4);
        bad[at] = (x >> 3) as u8;
        let _ = load(&resealed(bad));
    }
}

fn floating_sample() -> (Document, FloatingSnapshot) {
    let mut e = Editor::new(sample());
    e.frame = 1;
    e.layer = 0;
    e.apply(SelectionCmd::Select {
        shape: Box::new(Mask::rect(1, 1, 12, 9)),
        op: SelectOp::Replace,
    })
    .expect("select");
    e.apply(FloatingCmd::Lift { copy: false }).expect("lift");
    e.apply(FloatingCmd::Move {
        dx: -3,
        dy: 40,
        join: true,
    })
    .expect("move");
    let snapshot = e.floating_snapshot().expect("floating");
    (e.doc().clone(), snapshot)
}

fn pasted_sample() -> (Document, FloatingSnapshot) {
    let mut e = Editor::new(sample());
    let color = rgba(77, 66, 55, 255);
    e.apply(SelectionCmd::Select {
        shape: Box::new(Mask::rect(0, 0, 3, 3)),
        op: SelectOp::Replace,
    })
    .expect("select");
    e.apply(FloatingCmd::Paste {
        clip: std::sync::Arc::new(Clip::from_image(70, 2, vec![color; 140])),
        x: 4,
        y: 5,
    })
    .expect("fits");
    let snapshot = e.floating_snapshot().expect("floating");
    (e.doc().clone(), snapshot)
}

#[test]
fn floating_pixels_round_trip_beside_the_document() {
    for (doc, snapshot) in [floating_sample(), pasted_sample()] {
        let bytes = save_recovery(&doc, Some(&snapshot));
        let back = load_recovery(&bytes).expect("valid");
        assert_same(&doc, &back.doc);
        let found = back.floating.expect("floating");
        let table = |s: &FloatingSnapshot| {
            s.colors_before
                .as_ref()
                .map(|c| (c.colors().to_vec(), c.swatches().to_vec()))
        };
        assert_eq!(table(&found), table(&snapshot));
        let bare = |s: FloatingSnapshot| FloatingSnapshot {
            colors_before: None,
            ..s
        };
        assert_eq!(bare(found), bare(snapshot));
        assert!(!back.floating_damaged);
    }
}

#[test]
fn floating_pixels_are_charged_to_the_decode_limit_after_the_document() {
    for (doc, snapshot) in [floating_sample(), pasted_sample()] {
        let bytes = save_recovery(&doc, Some(&snapshot));
        let document = charges(&bytes).expect("valid");
        let mut spent = 0;
        recover(&bytes, &mut spent).expect("valid");
        let floating = spent - document;
        let (_, _, w, h) = snapshot.source.bounds();
        let least = snapshot.pixels.as_ref().map_or(0, |_| (w * h * 4) as usize);
        assert!(
            floating > least,
            "{floating} bytes charged for {least} of pixels"
        );
        let mut whole = MAX_DECODED_BYTES - spent;
        let back = recover(&bytes, &mut whole).expect("valid");
        assert!(back.floating.is_some() && !back.floating_damaged);
        assert_eq!(whole, MAX_DECODED_BYTES);
        // A byte short, the floating pixels are given up and the document kept.
        let mut short = MAX_DECODED_BYTES - spent + 1;
        let back = recover(&bytes, &mut short).expect("the document still opens");
        assert!(back.floating.is_none() && back.floating_damaged);
        assert_same(&doc, &back.doc);
    }
}

#[test]
fn a_recovery_copy_opens_as_a_plain_project_too() {
    let (doc, snapshot) = floating_sample();
    assert_same(
        &doc,
        &load(&save_recovery(&doc, Some(&snapshot))).expect("valid"),
    );
}

#[test]
fn saved_projects_never_carry_floating_pixels() {
    let (doc, _) = floating_sample();
    assert!(load_recovery(&save(&doc))
        .expect("valid")
        .floating
        .is_none());
}

#[test]
fn every_truncated_recovery_copy_is_rejected() {
    let (doc, snapshot) = floating_sample();
    let bytes = save_recovery(&doc, Some(&snapshot));
    for cut in (0..bytes.len()).step_by(5) {
        assert!(load_recovery(&bytes[..cut]).is_err(), "cut at {cut}");
    }
}

#[test]
fn damaged_floating_pixels_leave_the_document_readable() {
    let (doc, snapshot) = floating_sample();
    let plain = save(&doc).len();
    let mut bytes = save_recovery(&doc, Some(&snapshot));
    // The section's fixed fields, then the mask's position and size: its packed length.
    let inside = plain + 4 + 26 + 16;
    bytes[inside..inside + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let back = load_recovery(&resealed(bytes)).expect("document survives");
    assert_same(&doc, &back.doc);
    assert!(back.floating.is_none());
    assert!(back.floating_damaged);
}

#[test]
fn masks_larger_than_any_image_are_refused() {
    let (doc, mut snapshot) = pasted_sample();
    snapshot.pixels = None;
    snapshot.source = Arc::new(Mask::blank(0, 0, pikslar_core::MAX_DIMENSION as i32 + 1, 1));
    let back = load_recovery(&save_recovery(&doc, Some(&snapshot))).expect("document");
    assert!(back.floating.is_none() && back.floating_damaged);
}

fn animated() -> Document {
    let mut doc = sample();
    doc.add_frame(1, true).expect("room");
    doc.add_frame(2, false).expect("room");
    doc.add_tag("walk", 0, 1).expect("valid");
    doc.add_tag("idle", 2, 2).expect("valid");
    doc.tags[1].direction = pikslar_core::Direction::PingPong;
    doc.hold_across(0, 0, 0, 2).expect("frames");
    doc
}

#[test]
fn tags_and_holds_round_trip() {
    let doc = animated();
    let back = load(&save_v2(&doc)).expect("version 2");
    assert_eq!(back.tags, doc.tags);
    assert!(back.layers()[0].links().eq(doc.layers()[0].links()));
    let back = load(&save(&doc)).expect("valid");
    assert_eq!(back.tags, doc.tags);
    assert!(back.layers()[0].links().eq(doc.layers()[0].links()));
    assert!(back.layers()[0].cel(0).same_image(back.layers()[0].cel(2)));
    assert_same(&doc, &back);
}

#[test]
fn holds_on_different_layers_stay_separate_after_loading() {
    let mut doc = animated();
    doc.hold_across(1, 0, 0, 1).expect("frames");
    let bytes = save(&doc);
    let back = load(&bytes).expect("valid");
    let first: Vec<u32> = back.layers()[0].links().filter(|id| *id != 0).collect();
    let second: Vec<u32> = back.layers()[1].links().filter(|id| *id != 0).collect();
    assert!(!first.is_empty() && !second.is_empty());
    assert!(
        first.iter().all(|id| !second.contains(id)),
        "{first:?} {second:?}"
    );
    assert_eq!(
        save(&back),
        bytes,
        "the file does not change for being opened"
    );
}

#[test]
fn layers_opened_from_a_file_are_numbered_in_order() {
    let back = load(&save(&animated())).expect("valid");
    let ids: Vec<_> = back.layers().iter().map(|l| l.persisted_id()).collect();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn an_image_is_charged_for_the_table_it_keeps_and_not_the_canvas() {
    let blank = [0u8; 4];
    let mut spent = MAX_DECODED_BYTES - IMAGE_RECORD_CHARGE;
    let mut input = In {
        data: &blank,
        pos: 0,
    };
    let cel = read_grid(&mut input, 32, 32, &mut spent).expect("a blank image keeps nothing");
    assert!(cel.is_blank());
    assert_eq!(spent, MAX_DECODED_BYTES, "its record only");
}

#[test]
fn a_kept_table_still_counts_against_the_limit() {
    let mut drawn = vec![1u8];
    drawn.extend_from_slice(&7u32.to_le_bytes());
    drawn.extend_from_slice(&[0; 3]);
    let mut input = In {
        data: &drawn,
        pos: 0,
    };
    let mut spent = MAX_DECODED_BYTES;
    assert!(matches!(
        read_grid(&mut input, 32, 32, &mut spent),
        Err(FormatError::FileTooLarge)
    ));
}

#[test]
fn a_plain_drawing_gains_no_sections() {
    let doc = sample();
    let bytes = save(&doc);
    assert!(!bytes.windows(4).any(|w| w == b"TAGS" || w == b"LINK"));
}

#[test]
fn hold_ids_from_a_file_are_made_small() {
    let mut doc = animated();
    let ids: Vec<u32> = doc.layers()[0]
        .links()
        .map(|id| if id != 0 { u32::MAX } else { 0 })
        .collect();
    doc.set_holds(0, &ids).expect("one per frame");
    let back = load(&save(&doc)).expect("valid");
    let frames = back.frames.len() as u32;
    assert!(back.layers()[0].links().any(|id| id != 0));
    assert!(back.layers()[0].links().all(|id| id <= frames));
}

#[test]
fn holds_are_made_true_when_a_file_disagrees_with_itself() {
    // A document cannot hold frames showing different drawings, so the file is made by hand:
    // the holds of one document with the drawings of another.
    let held = save_v2(&animated());
    let mut doc = animated();
    let cel = doc.layers()[1].cel(2).clone();
    doc.put_cel(0, 2, cel).expect("frame exists");
    assert_eq!(
        doc.layers()[0].link(2),
        0,
        "a drawing of its own leaves the hold"
    );
    let mut bytes = save_v2(&doc);
    let section = |b: &[u8]| b.windows(4).rposition(|w| w == b"LINK").expect("section");
    let (from, to) = (section(&held), section(&bytes));
    let length = held.len() - 4 - from;
    assert_eq!(length, bytes.len() - 4 - to, "the same frames");
    bytes[to..to + length].copy_from_slice(&held[from..from + length]);
    let back = load(&resealed(bytes)).expect("valid");
    assert!(back.layers()[0].cel(0).same_image(back.layers()[0].cel(2)));
}

#[test]
fn damaged_animation_data_never_costs_the_art() {
    let doc = animated();
    let bytes = save_v2(&doc);
    let header = |tag: &[u8; 4]| {
        bytes
            .windows(4)
            .rposition(|w| w == tag)
            .expect("section written")
    };
    let (tags, links) = (header(b"TAGS"), header(b"LINK"));
    let payload: Vec<usize> = (tags + 8..links)
        .chain(links + 8..bytes.len() - 4)
        .collect();
    let mut x = 7u32;
    for _ in 0..3_000 {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut bad = bytes.clone();
        bad[payload[(x >> 8) as usize % payload.len()]] = (x >> 3) as u8;
        let back = load(&resealed(bad)).expect("the art still loads");
        assert_eq!(back.frames.len(), doc.frames.len());
        assert!(back
            .tags
            .iter()
            .all(|t| t.from <= t.to && t.to < back.frames.len()));
        for (a, b) in back.layers().iter().zip(doc.layers()) {
            assert_eq!(a.links().count(), back.frames.len());
            assert_eq!(a.frame_count(), back.frames.len());
            let size = (doc.width, doc.height);
            assert_eq!(
                a.cel(1).to_cells(size.0, size.1),
                b.cel(1).to_cells(size.0, size.1),
                "a frame keeps its picture when the holds are damaged"
            );
        }
    }
}

#[test]
fn hostile_floating_sections_never_panic() {
    let (doc, snapshot) = pasted_sample();
    let plain = save(&doc).len();
    let bytes = save_recovery(&doc, Some(&snapshot));
    let mut x = 11u32;
    for _ in 0..3_000 {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut bad = bytes.clone();
        let at = plain + (x >> 8) as usize % (bytes.len() - plain - 4);
        bad[at] = (x >> 3) as u8;
        let _ = load_recovery(&resealed(bad));
    }
}

#[test]
fn version_one_files_still_open() {
    let doc = sample();
    let mut bytes = save_v2(&doc);
    bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
    let without_sections = bytes.len() - 8;
    bytes.drain(without_sections..without_sections + 4);
    assert_same(&doc, &load(&resealed(bytes)).expect("valid"));
}

#[test]
fn a_version_two_name_too_long_to_keep_is_cut_and_noted() {
    let mut doc = sample();
    doc.layers_mut()[0].name = "é".repeat(3000);
    let (back, notes) = load_noted(&save_v2(&doc)).expect("kept");
    assert_eq!(back.layers()[0].name.len(), MAX_NAME_BYTES);
    assert!(
        notes.damaged.is_empty(),
        "a long name is shortened, not damaged"
    );
    assert!(notes.names_shortened && !notes.can_save_over());
    let said = notes.message().expect("told");
    assert!(said.starts_with("Some layer names were too long and were shortened. "));
}

#[test]
fn a_layer_name_that_is_not_text_is_damage() {
    let mut doc = sample();
    doc.layers_mut()[0].name = "abc".into();
    let mut bytes = save_v2(&doc);
    let at = bytes
        .windows(3)
        .position(|w| w == b"abc")
        .expect("the name is stored");
    bytes[at] = 0xff;
    let (_, notes) = load_noted(&resealed(bytes)).expect("kept");
    assert_eq!(notes.damaged, ["layer names"]);
    assert!(!notes.names_shortened);
}

#[test]
fn unknown_sections_are_skipped() {
    let doc = sample();
    let bytes = crate::piks_v3::encode(&doc, &[(b"NEXT", vec![1, 2, 3])]);
    assert_same(&doc, &load(&bytes).expect("valid"));
}

#[test]
fn files_from_a_newer_version_are_refused() {
    let mut bytes = save(&sample());
    bytes[4..8].copy_from_slice(&(NEWEST + 1).to_le_bytes());
    assert!(matches!(
        load(&resealed(bytes)),
        Err(FormatError::Unsupported(_))
    ));
}

#[test]
fn huge_counts_fail_cleanly() {
    let mut bytes = save(&sample());
    bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(load(&resealed(bytes)).is_err());
}

#[test]
fn timeline_drags_round_trip_holds_that_skip_frames() {
    use pikslar_core::{CelDrag, DragKind, Landing};
    let mut doc = animated();
    doc.add_frame(3, false).expect("room");
    // Frame 1's drawing linked into frame 4, past a frame of its own, then a block inserted.
    doc.release_hold(0, 1).expect("frame");
    let link = CelDrag {
        layers: (0, 0),
        frames: (0, 0),
        to_layer: 0,
        to_frame: 3,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    };
    doc.drag_cels(&link).expect("link");
    let insert = CelDrag {
        frames: (2, 3),
        to_frame: 1,
        kind: DragKind::Copy,
        landing: Landing::Insert,
        ..link
    };
    doc.drag_cels(&insert).expect("insert");
    let back = load(&save(&doc)).expect("valid");
    assert_same(&doc, &back);
    assert_eq!(back.tags, doc.tags, "tags follow the inserted frames");
    assert!(back.layers()[0].links().eq(doc.layers()[0].links()));
    let held: Vec<usize> = (0..back.frames.len())
        .filter(|&f| {
            back.layers()[0].link(f) != 0 && back.layers()[0].link(f) == back.layers()[0].link(0)
        })
        .collect();
    assert!(
        held.len() >= 2 && held.windows(2).any(|w| w[1] > w[0] + 1),
        "{held:?}"
    );
    for &f in &held {
        assert!(back.layers()[0].cel(f).same_image(back.layers()[0].cel(0)));
    }
}

#[test]
fn a_document_with_tilesets_is_written_and_nothing_is_refused() {
    let mut doc = sample();
    assert_eq!(
        try_save(&doc).expect("no tilesets"),
        save(&doc),
        "a document without tilesets is written as it always was"
    );
    doc.add_tileset("Walls", ColorMode::Rgba, (8, 8))
        .expect("room");
    check_writable(&doc).expect("writable");
    let bytes = try_save(&doc).expect("tilesets are written");
    assert_eq!(*load(&bytes).expect("opens").tilesets, *doc.tilesets);
    let copy = try_save_recovery(&doc, None).expect("a recovery copy too");
    assert_eq!(copy, bytes);
}

#[test]
fn a_layer_name_read_from_a_file_has_hidden_characters_turned_to_spaces() {
    let mut doc = sample();
    doc.layers_mut()[0].name = "a\nb\u{202E}c\u{200B}d".into();
    let (back, notes) = load_noted(&save_v2(&doc)).expect("kept");
    assert_eq!(back.layers()[0].name, "a b c d");
    assert!(notes.damaged.is_empty() && !notes.names_shortened);
    // A name already clean comes back as it was.
    doc.layers_mut()[0].name = "Sky é".into();
    let (back, _) = load_noted(&save_v2(&doc)).expect("kept");
    assert_eq!(back.layers()[0].name, "Sky é");
}
