//! Every file an earlier Pikslar wrote must still open and look exactly the same.

use pikslar_core::Editor;
use pikslar_format::piks;
use pikslar_safety::corpus::{entries, Entry};
use pikslar_safety::picture::{frame_hashes, stored_hash};

fn all() -> Vec<Entry> {
    let found = entries().expect("the corpus is readable");
    assert!(found.len() >= 20, "the corpus lost files");
    found
}

fn opened(entry: &Entry) -> (Editor, Vec<u8>) {
    let bytes = entry.bytes().expect("frozen file");
    let recovered = piks::load_recovery(&bytes)
        .unwrap_or_else(|e| panic!("{} no longer opens: {e}", entry.name));
    assert!(!recovered.floating_damaged, "{}", entry.name);
    assert_eq!(
        recovered.floating.is_some(),
        entry.floating,
        "{}",
        entry.name
    );
    let mut editor = Editor::new(recovered.doc);
    if let Some(snapshot) = recovered.floating {
        editor.restore_floating(snapshot).expect("fits");
    }
    (editor, bytes)
}

#[test]
fn the_corpus_holds_every_format_version() {
    let versions: Vec<u32> = all().iter().map(|e| e.version).collect();
    assert!(versions.contains(&1) && versions.contains(&2) && versions.contains(&3));
}

#[test]
fn saved_layer_numbers_are_kept() {
    let entry = all()
        .into_iter()
        .find(|e| e.name == "v3-numbered.piks")
        .expect("in the corpus");
    let doc = piks::load(&entry.bytes().expect("frozen")).expect("opens");
    let ids: Vec<u32> = doc
        .layers()
        .iter()
        .map(|l| l.persisted_id().get())
        .collect();
    assert_eq!(ids, [7, 3, 12]);
}

#[test]
fn every_frozen_file_has_its_shape() {
    for entry in all() {
        let (editor, bytes) = opened(&entry);
        let doc = editor.doc();
        assert_eq!(
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            entry.version
        );
        assert_eq!(
            (doc.width, doc.height),
            (entry.width, entry.height),
            "{}",
            entry.name
        );
        assert_eq!(doc.layers().len(), entry.layers, "{}", entry.name);
        assert_eq!(doc.frames.len(), entry.frames, "{}", entry.name);
        assert_eq!(doc.tags.len(), entry.tags, "{}", entry.name);
    }
}

#[test]
fn every_frozen_file_looks_the_same() {
    for entry in all() {
        let (mut editor, _) = opened(&entry);
        assert_eq!(frame_hashes(&mut editor), entry.hashes, "{}", entry.name);
    }
}

#[test]
fn saving_a_frozen_file_keeps_its_picture() {
    for entry in all().iter().filter(|e| !e.floating) {
        let (editor, _) = opened(entry);
        let again = piks::load(&piks::save(editor.doc())).expect("own output opens");
        assert_eq!(
            frame_hashes(&mut Editor::new(again)),
            entry.hashes,
            "{}",
            entry.name
        );
    }
}

#[test]
fn saving_a_current_file_changes_no_byte() {
    for entry in all().iter().filter(|e| !e.floating) {
        let (editor, bytes) = opened(entry);
        let saved = piks::save(editor.doc());
        if saved[4..8] == bytes[4..8] {
            assert!(saved == bytes, "{} changed when saved", entry.name);
        }
    }
}

#[test]
fn recovery_copies_open_as_plain_projects() {
    for entry in all().iter().filter(|e| e.floating) {
        let bytes = entry.bytes().expect("frozen file");
        let doc = piks::load(&bytes).expect("opens");
        assert_eq!(
            (doc.width, doc.height),
            (entry.width, entry.height),
            "{}",
            entry.name
        );
    }
}

#[test]
fn offsets_holds_and_pixels_past_the_canvas_are_kept_exactly() {
    let entry = all()
        .into_iter()
        .find(|e| e.name == "v3-offsets.piks")
        .expect("in the corpus");
    let bytes = entry.bytes().expect("frozen");
    let doc = piks::load(&bytes).expect("opens");
    assert_eq!(
        stored_hash(&doc),
        0x4f64_b95c,
        "every stored pixel and place"
    );
    let places: Vec<Vec<(i16, i16)>> = doc
        .layers()
        .iter()
        .map(|l| l.cels().iter().map(|c| c.position()).collect())
        .collect();
    assert_eq!(
        places,
        [
            vec![(-20, 9), (-20, 9), (30, -30)],
            vec![(5, 5), (0, 0), (12, -7)]
        ]
    );
    let (held, apart) = (&doc.layers()[0], &doc.layers()[1]);
    assert!(held.link(0) != 0 && held.link(0) == held.link(1));
    assert!(
        held.cel(0).looks_the_same(held.cel(1)),
        "a hold moved together"
    );
    assert!(apart.link(1) != 0 && apart.link(1) == apart.link(2));
    assert!(apart.cel(1).same_image(apart.cel(2)), "one drawing");
    assert!(
        !apart.cel(1).looks_the_same(apart.cel(2)),
        "shown from two places"
    );
    let again = piks::load(&piks::save(&doc)).expect("own save opens");
    assert_eq!(stored_hash(&again), stored_hash(&doc));
    assert_eq!(piks::save(&again), bytes, "saved again, byte for byte");
}

#[test]
fn moving_a_cel_back_shows_what_lay_past_the_canvas() {
    let entry = all()
        .into_iter()
        .find(|e| e.name == "v3-offsets.piks")
        .expect("in the corpus");
    let doc = piks::load(&entry.bytes().expect("frozen")).expect("opens");
    let mut editor = Editor::new(doc);
    let layer = editor.layer_id(0).expect("layer");
    let hidden = editor.doc().layers()[0].cel(0).get(-10, 20);
    assert_ne!(hidden, 0, "a pixel past the left edge");
    let step = pikslar_core::CelCmd::Move {
        layer,
        frame: 0,
        dx: 20,
        dy: -9,
    };
    editor.apply(step).expect("move back");
    assert_eq!(editor.doc().layers()[0].cel(0).get(10, 11), hidden);
    assert_eq!(
        editor.doc().layers()[0].cel(1).position(),
        (0, 0),
        "with its hold"
    );
}

fn frozen(name: &str) -> pikslar_core::Document {
    let entry = all()
        .into_iter()
        .find(|e| e.name == name)
        .expect("in the corpus");
    let (doc, notes) = piks::load_noted(&entry.bytes().expect("frozen")).expect("opens");
    assert!(notes.can_save_over(), "{name}: {notes:?}");
    doc
}

#[test]
fn m6_layer_settings_are_kept() {
    use pikslar_core::BlendMode::*;
    let doc = frozen("v3-layers.piks");
    assert_eq!((doc.width, doc.height), (40, 36), "the resized canvas");
    let blends: Vec<_> = doc.layers().iter().map(|l| l.blend).collect();
    let want = [
        Normal, Multiply, Screen, Screen, Overlay, Darken, Lighten, Addition, Subtract, Difference,
        Normal,
    ];
    assert_eq!(blends, want);
    let opacity: Vec<u8> = doc.layers().iter().map(|l| l.opacity).collect();
    assert_eq!(
        opacity,
        [255, 243, 231, 231, 219, 207, 195, 183, 171, 159, 255]
    );
    let locked: Vec<bool> = doc.layers().iter().map(|l| l.locked).collect();
    assert_eq!(locked.iter().filter(|&&l| l).count(), 1);
    assert!(locked[5]);
    assert_eq!(doc.layers()[5].name, "Ombre ü 影");
    assert_eq!(doc.layers()[3].name, "Layer 3 copy");
}

#[test]
fn m6_reference_layers_are_kept_and_left_out_of_the_picture() {
    use pikslar_core::LayerKind;
    let doc = frozen("v3-reference.piks");
    let kinds: Vec<_> = doc.layers().iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        [LayerKind::Paint, LayerKind::Reference, LayerKind::Paint]
    );
    assert_eq!(doc.picture_layers(), [true, false, true]);
    let reference = &doc.layers()[1];
    let first = reference.cel(0).image_id();
    assert!((0..3).all(|f| reference.cel(f).image_id() == first && reference.link(f) != 0));
    assert_ne!(reference.cel(0).get(36, 0), 0, "a pixel past the canvas");
}

#[test]
fn m6_groups_are_kept_with_their_depth_fold_eye_and_lock() {
    use pikslar_core::LayerKind::{Group, Paint, Reference};
    let doc = frozen("v3-groups.piks");
    doc.check_groups().expect("the list keeps its rule");
    let shape: Vec<_> = doc.layers().iter().map(|l| (l.kind, l.depth)).collect();
    let want = [
        (Paint, 0),
        (Paint, 2),
        (Paint, 2),
        (Group, 1),
        (Group, 0),
        (Paint, 0),
        (Paint, 1),
        (Group, 0),
        (Paint, 1),
        (Reference, 1),
        (Group, 0),
    ];
    assert_eq!(shape, want);
    let flags = |i: usize| {
        let l = &doc.layers()[i];
        (l.locked, l.visible, l.folded)
    };
    assert_eq!(flags(4), (true, true, false), "the locked group");
    assert_eq!(flags(7), (false, false, false), "the hidden group");
    assert_eq!(flags(10), (false, true, true), "the folded group");
    assert!(doc.locked_through(1) && doc.locked_through(2) && !doc.locked_through(5));
    assert_eq!(doc.listed_layers(), [10, 7, 6, 5, 4, 3, 2, 1, 0]);
    let picture = doc.picture_layers();
    assert_eq!(picture.iter().filter(|&&p| p).count(), 5, "{picture:?}");
    assert!(!picture[6], "inside the hidden group");
}

#[test]
fn the_rate_and_slices_file_keeps_its_timing_and_every_slice_field() {
    let doc = frozen("v3-rate-slices.piks");
    assert_eq!(doc.frame_rate, Some(24));
    let times: Vec<u32> = doc.frames.iter().map(|f| f.duration_ms).collect();
    assert_eq!(times, [83, 42, 250], "whole ticks: two, one and six");
    let units: Vec<u32> = (0..3).map(|f| doc.frame_units(f)).collect();
    assert_eq!(units, [2, 1, 6]);
    let rect = |x, y, width, height| pikslar_core::SliceRect {
        x,
        y,
        width,
        height,
    };
    assert_eq!(doc.slices.len(), 2);
    let (plain, full) = (&doc.slices[0], &doc.slices[1]);
    assert_eq!(
        (plain.name.as_str(), plain.color),
        ("hit box", 0xff_00_80_ff)
    );
    assert_eq!(plain.bounds, rect(-2, 3, 8, 5));
    assert_eq!((plain.pivot, plain.center), (None, None));
    assert_eq!((full.name.as_str(), full.color), ("panel ü", 0x01_02_03_04));
    assert_eq!(full.bounds, rect(1, 1, 18, 14));
    assert_eq!(full.pivot, Some((4, -1)));
    assert_eq!(full.center, Some(rect(5, 5, 8, 4)));
}

#[test]
fn the_links_file_shares_one_drawing_across_layers_at_its_own_places() {
    let doc = frozen("v3-links.piks");
    let (bottom, held, top) = (&doc.layers()[0], &doc.layers()[1], &doc.layers()[2]);
    let id = bottom.link(0);
    assert!(
        id != 0 && top.link(1) == id && top.link(2) == id,
        "one link"
    );
    assert!(bottom.cel(0).same_image(top.cel(1)) && bottom.cel(0).same_image(top.cel(2)));
    assert_eq!(top.cel(2).position(), (7, -4), "moved alone");
    assert_eq!(top.cel(1).position(), (0, 0));
    assert!(held.link(0) != 0 && held.link(0) == held.link(1) && held.link(0) != id);
    assert_eq!(doc.check_links(), Ok(()));
}

#[test]
fn the_tilemap_file_keeps_its_tilesets_counters_maps_and_required_bit() {
    use pikslar_core::tileset::{TileId, TilesetId};
    use pikslar_core::LayerKind;
    let entry = all()
        .into_iter()
        .find(|e| e.name == "v3-tilemap.piks")
        .expect("in the corpus");
    let bytes = entry.bytes().expect("frozen");
    let features = u32::from_le_bytes(bytes[8..12].try_into().expect("four bytes"));
    assert_eq!(
        features, 4,
        "required feature bit 2 only: an M7 build refuses it"
    );
    let doc = frozen("v3-tilemap.piks");
    doc.check_tilemaps().expect("the rules hold");
    assert_eq!(doc.tilesets.len(), 2);
    assert_eq!(
        doc.tilesets.next_id(),
        4,
        "the removed tileset's number is not reused"
    );
    let walls = doc.tilesets.get(TilesetId::new(1)).expect("walls");
    assert_eq!((walls.name(), walls.tile_size()), ("Walls ü", (8, 8)));
    let ids: Vec<u32> = walls.tiles().iter().map(|t| t.id.get()).collect();
    assert_eq!(ids, [1, 3]);
    assert_eq!(
        walls.next_tile_id(),
        4,
        "the removed tile's number is not reused"
    );
    assert!(TileId::new(2).is_some_and(|gone| walls.tile(gone).is_none()));
    let kinds: Vec<LayerKind> = doc.layers().iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        [
            LayerKind::Paint,
            LayerKind::Tilemap(TilesetId::new(1)),
            LayerKind::Tilemap(TilesetId::new(2))
        ]
    );
    assert!(doc.layers()[1..].iter().all(|l| !l.visible));
    let again = piks::load(&piks::save(&doc)).expect("own save opens");
    assert_eq!(piks::save(&again), bytes, "saved again, byte for byte");
    let copy = piks::load_recovery(&piks::save_recovery(&doc, None)).expect("recovery opens");
    assert_eq!(*copy.doc.tilesets, *doc.tilesets);
}

#[test]
fn the_aseprite_blending_file_keeps_its_setting_modes_and_required_bit() {
    use pikslar_core::{BlendMode, Blending};
    let entry = all()
        .into_iter()
        .find(|e| e.name == "v3-aseprite-blending.piks")
        .expect("in the corpus");
    let bytes = entry.bytes().expect("frozen");
    let features = u32::from_le_bytes(bytes[8..12].try_into().expect("four bytes"));
    assert_eq!(
        features, 8,
        "required feature bit 3 only: a build before it refuses the file"
    );
    let doc = frozen("v3-aseprite-blending.piks");
    assert_eq!(doc.blending, Blending::Aseprite);
    let blends: Vec<BlendMode> = doc.layers().iter().map(|l| l.blend).collect();
    assert_eq!(
        blends,
        [
            BlendMode::Normal,
            BlendMode::Hue,
            BlendMode::ColorBurn,
            BlendMode::SoftLight
        ]
    );
    assert_eq!(doc.layers()[2].opacity, 170);
    let again = piks::load(&piks::save(&doc)).expect("own save opens");
    assert_eq!(again.blending, Blending::Aseprite);
    assert_eq!(piks::save(&again), bytes, "saved again, byte for byte");
}
