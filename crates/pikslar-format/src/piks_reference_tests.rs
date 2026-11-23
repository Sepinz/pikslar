//! Reference layers in version 3: the kind byte, the required feature bit, and the checks a
//! file passes before its references are kept.

use crate::piks::{load, load_noted, load_recovery, save, save_recovery};
use crate::{crc32, FormatError};
use pikslar_core::{rgba, Cel, ColorMode, Document, Editor, Grid, LayerCmd, LayerKind};
use std::sync::Arc;

const PHOTO: u32 = rgba(30, 60, 200, 255);
const NAME: &str = "RefName";

fn strip(width: usize) -> Cel {
    let mut cells = vec![0; width];
    cells[0] = PHOTO;
    cells[width - 1] = PHOTO;
    Cel::new(Grid::from_cells(width, 1, &cells))
}

/// One paint layer and a locked reference over three frames.
fn with_reference(width: usize) -> Document {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("valid");
    doc.add_frame(1, false).expect("frame");
    doc.add_frame(2, false).expect("frame");
    let mut e = Editor::new(doc);
    let place = LayerCmd::PlaceReference {
        below: 0,
        image: Arc::new(strip(width).image().clone()),
        name: NAME.into(),
    };
    e.apply(place).expect("placed");
    let mut doc = e.doc().clone();
    doc.layers_mut()[1].locked = true;
    doc
}

fn resealed(mut bytes: Vec<u8>) -> Vec<u8> {
    let end = bytes.len() - 4;
    let crc = crc32(&[&bytes[..end]]);
    bytes[end..].copy_from_slice(&crc.to_le_bytes());
    bytes
}

fn features(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[8..12].try_into().expect("four bytes"))
}

#[test]
fn a_reference_is_saved_and_loaded_with_its_kind_lock_and_hold() {
    let doc = with_reference(40);
    let bytes = save(&doc);
    assert_eq!(features(&bytes), 1, "an older version must refuse it");
    let back = load(&bytes).expect("loads");
    assert_eq!(back.layers(), doc.layers());
    let reference = &back.layers()[1];
    assert_eq!(reference.kind, LayerKind::Reference);
    assert!(reference.locked);
    assert_eq!(reference.cel(2).get(39, 0), PHOTO);
    let link = reference.link(0);
    assert!(link != 0 && reference.links().all(|l| l == link));
}

#[test]
fn a_file_without_references_needs_no_feature() {
    let mut doc = with_reference(8);
    doc.layers_mut()[1].kind = LayerKind::Paint;
    assert_eq!(features(&save(&doc)), 0);
}

#[test]
fn a_recovery_copy_keeps_the_reference() {
    let doc = with_reference(8);
    let back = load_recovery(&save_recovery(&doc, None)).expect("recovers");
    assert_eq!(back.doc.layers()[1].kind, LayerKind::Reference);
}

#[test]
fn an_unknown_feature_is_refused() {
    let mut bytes = save(&with_reference(8));
    bytes[8..12].copy_from_slice(&16u32.to_le_bytes());
    assert!(matches!(
        load(&resealed(bytes)),
        Err(FormatError::Unsupported(_))
    ));
}

#[test]
fn an_unknown_layer_kind_is_read_as_paint_and_protects_the_file() {
    let mut bytes = save(&with_reference(8));
    let marker = [NAME.as_bytes(), &[1, 1]].concat();
    let at = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .expect("the reference's name, flags and kind");
    bytes[at + marker.len() - 1] = 9;
    let (doc, notes) = load_noted(&resealed(bytes)).expect("kept");
    assert_eq!(doc.layers()[1].kind, LayerKind::Paint);
    assert!(!notes.can_save_over());
}

#[test]
fn a_reference_too_wide_is_refused_when_loaded() {
    let mut doc = with_reference(8);
    let wide = pikslar_core::reference::MAX_REFERENCE_SIDE + 64;
    for frame in 0..3 {
        doc.put_cel(1, frame, strip(wide)).expect("cel");
    }
    assert!(matches!(load(&save(&doc)), Err(FormatError::Corrupt(_))));
}

#[test]
fn a_reference_whose_frames_differ_is_shown_held_on_every_frame() {
    let mut doc = with_reference(8);
    doc.set_holds(1, &[0, 0, 0]).expect("holds");
    doc.put_cel(1, 2, Cel::blank()).expect("cel");
    let (back, notes) = load_noted(&save(&doc)).expect("loads");
    assert_eq!(
        notes.damaged,
        ["reference pictures"],
        "told, so it is not saved over"
    );
    let reference = &back.layers()[1];
    assert_eq!(reference.cel(2).get(7, 0), PHOTO);
    let link = reference.link(0);
    assert!(link != 0 && reference.links().all(|l| l == link));
}

/// A reference of the largest size, every tile drawn, all sharing one tile so the test is cheap.
fn largest(color: u32) -> Arc<pikslar_core::Image> {
    use pikslar_core::tile::{Tile, TILE_AREA};
    let tile = Arc::new(Tile::pack(&[color; TILE_AREA]).expect("drawn"));
    let side = pikslar_core::reference::MAX_REFERENCE_SIDE / pikslar_core::TILE;
    let cells = vec![Some(tile); side * side];
    Arc::new(pikslar_core::Image::from_tiles(0, 0, side as u16, cells).expect("fits"))
}

#[test]
fn references_together_past_the_memory_limit_are_refused_when_loaded() {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    let mut e = Editor::new(doc.clone());
    let place = LayerCmd::PlaceReference {
        below: 0,
        image: largest(PHOTO),
        name: "Wall".into(),
    };
    e.apply(place).expect("placed");
    doc = e.doc().clone();
    assert!(load(&save(&doc)).is_ok(), "one is exactly the limit");
    doc.add_layer(2, ColorMode::Rgba).expect("layer");
    doc.layers_mut()[2].kind = LayerKind::Reference;
    let other = Cel::from_image((*largest(rgba(1, 2, 3, 255))).clone());
    doc.put_cel(2, 0, other).expect("cel");
    assert!(
        matches!(load(&save(&doc)), Err(FormatError::Corrupt(_))),
        "two are past 64 MiB"
    );
}

#[test]
fn a_reference_without_its_feature_is_refused() {
    let mut bytes = save(&with_reference(8));
    bytes[8..12].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(
        load(&resealed(bytes)),
        Err(FormatError::Corrupt(
            "a reference layer without its feature"
        ))
    ));
}
