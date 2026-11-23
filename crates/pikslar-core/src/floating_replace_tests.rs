//! `FloatingCmd::ReplacePasted`: text typed after a paste swaps the floating pixels in place.

use super::*;
use crate::color::rgba;
use crate::command::{FloatingCmd, LayerCmd, SelectionCmd, TileCmd};
use crate::document::{ColorMode, Document, LimitError};
use crate::layer::LayerKind;
use crate::selection::SelectOp;

const RED: u32 = rgba(255, 0, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);

fn editor(mode: ColorMode) -> Editor {
    Editor::new(Document::new(32, 32, mode).expect("valid"))
}

fn clip(w: usize, h: usize, color: u32) -> Arc<Clip> {
    Arc::new(Clip::from_image(w, h, vec![color; w * h]))
}

fn select(e: &mut Editor, x: i32, y: i32, w: i32, h: i32) {
    let shape = Box::new(Mask::rect(x, y, w, h));
    let op = SelectOp::Replace;
    e.apply(SelectionCmd::Select { shape, op }).expect("select");
}

fn paste(e: &mut Editor, clip: &Arc<Clip>, x: i32, y: i32) {
    let clip = clip.clone();
    e.apply(FloatingCmd::Paste { clip, x, y }).expect("paste");
}

fn replace(e: &mut Editor, clip: &Arc<Clip>) -> Result<bool, LimitError> {
    let clip = clip.clone();
    e.apply(FloatingCmd::ReplacePasted { clip })
        .map(|outcome| outcome.changed)
}

fn mv(e: &mut Editor, dx: i32, dy: i32) {
    let join = false;
    e.apply(FloatingCmd::Move { dx, dy, join }).expect("move");
}

fn cells(e: &Editor) -> Vec<u32> {
    e.doc.layers[e.layer]
        .cel(0)
        .to_cells(e.doc.width, e.doc.height)
}

fn bounds(e: &Editor) -> Option<(i32, i32, i32, i32)> {
    e.selection().map(|m| m.bounds())
}

#[test]
fn paste_replaces_a_move_and_anchor_are_one_undo_step() {
    let mut e = editor(ColorMode::Rgba);
    select(&mut e, 1, 1, 2, 2);
    let (depth, before) = (e.history.undo_depth(), cells(&e));
    paste(&mut e, &clip(3, 1, RED), 5, 5);
    for width in 1..=4 {
        assert!(replace(&mut e, &clip(width, 2, BLUE)).expect("replace"));
    }
    mv(&mut e, 3, 2);
    assert!(replace(&mut e, &clip(2, 2, RED)).expect("replace"));
    assert_eq!(bounds(&e), Some((8, 7, 2, 2)));
    e.apply(FloatingCmd::Anchor).expect("anchor");
    assert_eq!(e.history.undo_depth(), depth + 1);
    assert_eq!(e.color_at(8, 7), RED);
    assert_eq!(e.color_at(9, 8), RED);
    assert_eq!(e.color_at(10, 7), 0, "only the last text is left");
    assert_eq!(e.color_at(5, 5), 0);
    assert!(e.undo());
    assert_eq!(cells(&e), before);
    assert_eq!(bounds(&e), Some((1, 1, 2, 2)));
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn it_is_refused_unless_a_paste_is_floating() {
    let mut e = editor(ColorMode::Rgba);
    assert!(!replace(&mut e, &clip(2, 2, RED)).expect("nothing to do"));
    assert!(!e.is_floating());
    select(&mut e, 1, 1, 2, 2);
    e.apply(FloatingCmd::Lift { copy: false }).expect("lift");
    assert!(!replace(&mut e, &clip(2, 2, RED)).expect("a lift is not a paste"));
    assert_eq!(bounds(&e), Some((1, 1, 2, 2)));
    assert!(e.floating.as_ref().is_some_and(|f| !f.pasted));
    paste(&mut e, &clip(1, 1, RED), 9, 9);
    e.apply(FloatingCmd::Anchor).expect("anchor");
    assert!(!replace(&mut e, &clip(2, 2, BLUE)).expect("anchored"));
    assert_eq!(e.color_at(9, 9), RED);
}

#[test]
fn a_malformed_clip_is_refused_and_changes_nothing() {
    let mut e = editor(ColorMode::Rgba);
    paste(&mut e, &clip(2, 2, RED), 4, 4);
    let bad = Arc::new(Clip::from_image(3, 3, vec![RED; 2]));
    assert_eq!(replace(&mut e, &bad), Err(LimitError::Dimensions));
    assert_eq!(bounds(&e), Some((4, 4, 2, 2)));
}

#[test]
fn reverting_leaves_the_document_and_history_as_they_were() {
    for mode in [ColorMode::Rgba, ColorMode::Palette] {
        let mut e = editor(mode);
        select(&mut e, 1, 1, 2, 2);
        let (doc, depth, colors) = (e.doc.clone(), e.history.undo_depth(), e.doc.colors.clone());
        paste(&mut e, &clip(2, 2, RED), 5, 5);
        replace(&mut e, &clip(3, 2, BLUE)).expect("replace");
        mv(&mut e, 1, 1);
        replace(&mut e, &clip(1, 1, RED)).expect("replace");
        e.apply(FloatingCmd::Revert).expect("revert");
        assert_eq!(e.doc, doc, "{mode:?}");
        assert_eq!(e.history.undo_depth(), depth, "{mode:?}");
        assert!(Arc::ptr_eq(&e.doc.colors, &colors), "{mode:?}");
        assert_eq!(bounds(&e), Some((1, 1, 2, 2)), "{mode:?}");
        assert!(!e.is_floating());
    }
}

#[test]
fn replacing_forgets_the_colors_of_the_text_it_replaced() {
    let mut e = editor(ColorMode::Palette);
    paste(&mut e, &clip(2, 2, RED), 0, 0);
    replace(&mut e, &clip(2, 2, BLUE)).expect("replace");
    e.apply(FloatingCmd::Anchor).expect("anchor");
    assert!(e.doc.colors.id_for_existing(BLUE).is_some());
    assert!(e.doc.colors.id_for_existing(RED).is_none());
    assert_eq!(e.color_at(1, 1), BLUE);
    assert!(e.undo());
    assert!(e.doc.colors.id_for_existing(BLUE).is_none());
}

#[test]
fn text_running_off_the_canvas_is_clipped_when_anchored() {
    let mut e = editor(ColorMode::Rgba);
    paste(&mut e, &clip(1, 1, RED), 30, 30);
    assert!(replace(&mut e, &clip(6, 4, BLUE)).expect("replace"));
    e.apply(FloatingCmd::Anchor).expect("anchor");
    assert_eq!(e.color_at(30, 30), BLUE);
    assert_eq!(e.color_at(31, 31), BLUE);
    assert_eq!(bounds(&e), Some((30, 30, 2, 2)));
    assert!(e.undo());
    assert_eq!(e.color_at(31, 31), 0);
}

#[test]
fn a_replacement_that_does_not_fit_the_palette_changes_nothing() {
    let mut e = editor(ColorMode::Palette);
    paste(&mut e, &clip(2, 2, RED), 3, 3);
    let colors = (1..=90_000u32)
        .map(|i| rgba((i >> 16) as u8, (i >> 8) as u8, i as u8, 255))
        .collect();
    let big = Arc::new(Clip::from_image(300, 300, colors));
    assert!(replace(&mut e, &big).is_err());
    assert_eq!(bounds(&e), Some((3, 3, 2, 2)));
    e.apply(FloatingCmd::Anchor).expect("anchor");
    assert_eq!(e.color_at(3, 3), RED);
}

type Setup = fn(&mut Editor);

/// The layers a paste is refused on refuse a replacement just as much.
#[test]
fn it_is_refused_on_tilemap_reference_group_and_locked_layers() {
    let cases: [(&str, Setup, LimitError); 4] = [
        ("tilemap", tilemap, LimitError::Tilemap),
        ("reference", reference, LimitError::Reference),
        ("group", group, LimitError::Group),
        ("locked", locked, LimitError::Locked),
    ];
    for (name, setup, refusal) in cases {
        let mut e = editor(ColorMode::Rgba);
        setup(&mut e);
        let (doc, depth) = (e.doc.clone(), e.history.undo_depth());
        assert_eq!(replace(&mut e, &clip(2, 2, RED)), Err(refusal), "{name}");
        let clip = clip(2, 2, RED);
        let paste = e.apply(FloatingCmd::Paste { clip, x: 1, y: 1 });
        assert_eq!(paste.map(|_| ()), Err(refusal), "{name}");
        assert_eq!((&e.doc, e.history.undo_depth()), (&doc, depth), "{name}");
        assert!(!e.is_floating(), "{name}");
    }
}

fn tilemap(e: &mut Editor) {
    e.apply(TileCmd::NewMapLayer {
        at: 1,
        name: "Map".into(),
        mode: ColorMode::Rgba,
        tile_w: 8,
        tile_h: 8,
    })
    .expect("map");
    assert!(matches!(e.doc.layers[e.layer].kind, LayerKind::Tilemap(_)));
}

fn reference(e: &mut Editor) {
    let pixels = [((1, 1), RED)].into_iter().collect();
    let image = Arc::new(crate::offset_tests::image_of(&pixels));
    let name = "Sketch".into();
    let below = 0;
    e.apply(LayerCmd::PlaceReference { below, image, name })
        .expect("placed");
    assert_eq!(e.doc.layers[e.layer].kind, LayerKind::Reference);
}

fn group(e: &mut Editor) {
    let layer = e.layer_id(0).expect("layer");
    e.apply(LayerCmd::Group { layer }).expect("grouped");
    assert_eq!(e.doc.layers[e.layer].kind, LayerKind::Group);
}

fn locked(e: &mut Editor) {
    let layer = e.layer_id(0).expect("layer");
    let locked = true;
    e.apply(LayerCmd::SetLocked { layer, locked })
        .expect("lock");
}
