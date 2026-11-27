//! Layer groups in version 3: the kind and depth bytes, the folded flag, the required
//! feature bit, and the order a file must keep.

use crate::piks::{load, load_noted, load_recovery, save, save_recovery};
use crate::{crc32, FormatError};
use pikslar_core::{
    rgba, Cel, ColorMode, Document, Editor, FloatingCmd, Grid, LayerCmd, LayerKind, Mask, SelectOp,
    SelectionCmd,
};

const INK: u32 = rgba(200, 30, 60, 255);

/// Top first: a folded, locked group holding a hidden group holding a drawn layer, over a
/// plain layer.
fn with_groups() -> Document {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    let mut cells = vec![0; 16 * 16];
    cells[3 * 16 + 2] = INK;
    doc.put_cel(1, 0, Cel::new(Grid::from_cells(16, 16, &cells)))
        .expect("drawn");
    let mut e = Editor::new(doc);
    let layer = e.layer_id(1).expect("layer");
    e.apply(LayerCmd::Group { layer }).expect("grouped");
    let inner = e.layer_id(2).expect("group");
    e.apply(LayerCmd::Group { layer: inner }).expect("grouped");
    let mut doc = e.doc().clone();
    doc.layers_mut()[2].visible = false;
    doc.layers_mut()[3].folded = true;
    doc.layers_mut()[3].locked = true;
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
fn groups_are_saved_and_loaded_with_their_depth_fold_and_lock() {
    let doc = with_groups();
    let depths: Vec<u8> = doc.layers().iter().map(|l| l.depth).collect();
    assert_eq!(depths, [0, 2, 1, 0]);
    let bytes = save(&doc);
    assert_eq!(features(&bytes), 2, "an older version must refuse it");
    let back = load(&bytes).expect("loads");
    assert_eq!(back.layers(), doc.layers());
    assert_eq!(back.layers()[3].kind, LayerKind::Group);
    assert!(back.layers()[3].folded && back.layers()[3].locked);
    assert!(!back.layers()[2].visible);
    assert_eq!(back.layers()[1].cel(0).get(2, 3), INK);
    assert_eq!(back.picture_layers(), [true, false, false, false]);
}

#[test]
fn a_file_without_groups_needs_no_feature_and_no_extra_bytes() {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    let plain = save(&doc);
    assert_eq!(features(&plain), 0);
    let mut grouped = Editor::new(doc);
    let layer = grouped.layer_id(1).expect("layer");
    grouped.apply(LayerCmd::Group { layer }).expect("grouped");
    let bytes = save(grouped.doc());
    assert_eq!(features(&bytes), 2);
    assert!(bytes.len() > plain.len());
}

fn reloaded(doc: &Document) -> Result<Document, FormatError> {
    load(&save(doc))
}

#[test]
fn groups_out_of_order_are_refused_when_loaded() {
    let mut doc = with_groups();
    doc.layers_mut()[3].depth = 1;
    assert!(
        matches!(reloaded(&doc), Err(FormatError::Corrupt(_))),
        "top"
    );
    let mut doc = with_groups();
    doc.layers_mut()[1].depth = 3;
    assert!(
        matches!(reloaded(&doc), Err(FormatError::Corrupt(_))),
        "gap"
    );
    let mut doc = with_groups();
    for layer in doc.layers_mut() {
        layer.depth = 9;
    }
    assert!(
        matches!(reloaded(&doc), Err(FormatError::Corrupt(_))),
        "deep"
    );
}

#[test]
fn a_group_with_pixels_is_refused_when_loaded() {
    let mut doc = with_groups();
    let mut cells = vec![0; 4];
    cells[1] = INK;
    doc.put_cel(3, 0, Cel::new(Grid::from_cells(2, 2, &cells)))
        .expect("cel");
    assert!(matches!(reloaded(&doc), Err(FormatError::Corrupt(_))));
}

#[test]
fn a_fold_on_a_layer_that_is_no_group_is_not_kept() {
    let mut doc = with_groups();
    doc.layers_mut()[0].folded = true;
    let back = reloaded(&doc).expect("loads");
    assert!(!back.layers()[0].folded);
    // Written by hand, since the writer never writes one: the fold flag on a drawn layer.
    let name = doc.layers()[1].name.clone();
    let mut bytes = save(&doc);
    let marker = [name.as_bytes(), &[0, 0, 2]].concat();
    let at = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .expect("the drawn layer's name, flags, kind and depth");
    bytes[at + name.len()] = 2;
    let back = load(&resealed(bytes)).expect("loads");
    assert!(!back.layers()[1].folded);
}

#[test]
fn an_unknown_kind_in_a_group_keeps_its_depth_and_protects_the_file() {
    let doc = with_groups();
    let name = doc.layers()[1].name.clone();
    let mut bytes = save(&doc);
    let marker = [name.as_bytes(), &[0, 0, 2]].concat();
    let at = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .expect("the drawn layer's name, flags, kind and depth");
    bytes[at + name.len() + 1] = 9;
    let (back, notes) = load_noted(&resealed(bytes)).expect("kept");
    assert_eq!(back.layers()[1].kind, LayerKind::Paint);
    assert_eq!(back.layers()[1].depth, 2);
    assert!(!notes.can_save_over());
}

#[test]
fn a_recovery_copy_keeps_the_groups_and_pixels_lifted_inside_them() {
    let mut doc = with_groups();
    doc.layers_mut()[2].visible = true;
    doc.layers_mut()[3].locked = false;
    let mut e = Editor::new(doc);
    e.layer = 1;
    let shape = Box::new(Mask::rect(0, 0, 8, 8));
    let op = SelectOp::Replace;
    e.apply(SelectionCmd::Select { shape, op }).expect("select");
    e.apply(FloatingCmd::Lift { copy: false }).expect("lift");
    let (dx, dy, join) = (3, 1, false);
    e.apply(FloatingCmd::Move { dx, dy, join }).expect("float");
    let snapshot = e.floating_snapshot().expect("floating");
    let back = load_recovery(&save_recovery(e.doc(), Some(&snapshot))).expect("recovers");
    assert!(!back.floating_damaged);
    let shape = |d: &Document| -> Vec<_> {
        let row = |l: &pikslar_core::Layer| (l.kind, l.depth, l.folded, l.visible, l.locked);
        d.layers().iter().map(row).collect()
    };
    assert_eq!(shape(&back.doc), shape(e.doc()));
    let mut restored = Editor::new(back.doc);
    restored.layer = 1;
    let floating = back.floating.expect("floating");
    restored.restore_floating(floating).expect("fits");
    for editor in [&mut e, &mut restored] {
        editor.apply(FloatingCmd::Anchor).expect("anchor");
    }
    assert_eq!(restored.doc().layers()[1].cel(0).get(5, 4), INK);
    assert_eq!(restored.doc().layers(), e.doc().layers());
}

#[test]
fn a_group_without_its_feature_is_refused() {
    let mut bytes = save(&with_groups());
    assert_eq!(features(&bytes), 2);
    bytes[8..12].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(
        load(&resealed(bytes)),
        Err(FormatError::Corrupt("a layer group without its feature"))
    ));
}
