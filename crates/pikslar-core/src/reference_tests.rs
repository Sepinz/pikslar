//! Reference layers: placing one, what refuses to draw on it, and how it stays one drawing
//! held across every frame.

use crate::cel_drag::{CelDrag, DragKind, Landing};
use crate::color::rgba;
use crate::command::{CelCmd, DrawCmd, FrameCmd, LayerCmd, StepCmd};
use crate::document::{ColorMode, Document, LimitError, MAX_LAYERS};
use crate::editor::Editor;
use crate::image::Image;
use crate::layer::LayerKind;
use crate::offset_tests::image_of;
use crate::reference::{MAX_REFERENCE_BYTES, MAX_REFERENCE_SIDE};
use std::collections::HashMap;
use std::sync::Arc;

const GREEN: u32 = rgba(20, 200, 40, 255);

fn picture() -> Arc<Image> {
    let pixels: HashMap<(i32, i32), u32> = [((1, 1), GREEN), ((40, 3), GREEN)].into();
    Arc::new(image_of(&pixels))
}

/// One paint layer over three frames, with a reference placed above it.
fn editor() -> Editor {
    let mut e = Editor::new(Document::new(32, 32, ColorMode::Rgba).expect("valid"));
    for _ in 0..2 {
        e.doc.add_frame(e.doc.frames.len(), false).expect("frame");
    }
    place(&mut e).expect("placed");
    e
}

fn place(e: &mut Editor) -> Result<(), LimitError> {
    let command = LayerCmd::PlaceReference {
        below: e.layer,
        image: picture(),
        name: "Sketch".into(),
    };
    e.apply(command).map(|_| ())
}

/// True when every frame of the layer shows one drawing, held together.
fn held_everywhere(e: &Editor, index: usize) -> bool {
    let layer = &e.doc.layers[index];
    let first = layer.cel(0).image_id();
    let link = layer.link(0);
    (0..layer.frame_count()).all(|f| layer.cel(f).image_id() == first && layer.link(f) == link)
        && link != 0
}

#[test]
fn placing_puts_a_held_reference_above_the_active_layer_and_makes_it_active() {
    let e = editor();
    assert_eq!(e.doc.layers.len(), 2);
    assert_eq!(e.layer, 1);
    let reference = &e.doc.layers[1];
    assert_eq!(reference.kind, LayerKind::Reference);
    assert_eq!(reference.name, "Sketch");
    assert_eq!(reference.cel(2).get(40, 3), GREEN, "kept past the canvas");
    assert!(held_everywhere(&e, 1));
    assert!(!reference.in_picture());
    assert!(e.doc.layers[0].in_picture());
    assert_ne!(reference.persisted_id(), e.doc.layers[0].persisted_id());
}

#[test]
fn placing_is_one_step_undone_and_redone() {
    let mut e = editor();
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(e.doc.layers.len(), 1);
    e.apply(StepCmd::Redo).expect("redo");
    assert_eq!(e.doc.layers[1].kind, LayerKind::Reference);
}

#[test]
fn drawing_on_a_reference_is_refused_and_changes_nothing() {
    let mut e = editor();
    let before = e.doc.layers[1].clone();
    let refused = e.apply(DrawCmd::EraseCell { x: 1, y: 1 });
    assert_eq!(refused.map(|_| ()), Err(LimitError::Reference));
    let layer = e.layer_id(1).expect("layer");
    let turn = CelCmd::Turn {
        layer,
        frame: 0,
        turn: crate::layer::Turn::FlipHorizontal,
    };
    assert_eq!(e.apply(turn).map(|_| ()), Err(LimitError::Reference));
    let mode = LayerCmd::SetMode {
        layer,
        mode: ColorMode::Palette,
    };
    assert_eq!(e.apply(mode).map(|_| ()), Err(LimitError::Reference));
    assert_eq!(e.doc.layers[1].cel(0).image_id(), before.cel(0).image_id());
}

#[test]
fn a_reference_moves_on_every_frame_at_once_unless_locked() {
    let mut e = editor();
    let layer = e.layer_id(1).expect("layer");
    let step = CelCmd::Move {
        layer,
        frame: 1,
        dx: 3,
        dy: 2,
    };
    e.apply(step).expect("moved");
    for frame in 0..3 {
        assert_eq!(e.doc.layers[1].cel(frame).get(4, 3), GREEN, "frame {frame}");
    }
    e.doc.layers[1].locked = true;
    assert_eq!(e.apply(step).map(|_| ()), Err(LimitError::Locked));
}

#[test]
fn a_reference_cannot_be_released_merged_or_dragged_onto() {
    let mut e = editor();
    let layer = e.layer_id(1).expect("layer");
    let release = FrameCmd::Release {
        layer,
        from: 0,
        to: 2,
    };
    assert_eq!(e.apply(release).map(|_| ()), Err(LimitError::Reference));
    let merge = LayerCmd::MergeDown { layer };
    assert_eq!(e.apply(merge).map(|_| ()), Err(LimitError::Reference));
    let onto = CelDrag {
        layers: (0, 0),
        frames: (0, 0),
        to_layer: 1,
        to_frame: 1,
        kind: DragKind::Copy,
        landing: Landing::Overwrite,
    };
    assert_eq!(
        e.apply(CelCmd::Drag(onto)).map(|_| ()),
        Err(LimitError::Reference)
    );
    assert!(held_everywhere(&e, 1));
}

#[test]
fn a_paint_layer_can_still_be_released_while_locked() {
    let mut e = editor();
    let layer = e.layer_id(0).expect("layer");
    e.doc.set_holds(0, &[1, 1, 1]).expect("holds");
    e.doc.layers[0].locked = true;
    let release = FrameCmd::Release {
        layer,
        from: 0,
        to: 2,
    };
    e.apply(release).expect("released");
}

#[test]
fn new_frames_show_the_reference_held_with_the_rest() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 3, copy: false })
        .expect("blank frame");
    e.apply(FrameCmd::Add { at: 0, copy: true })
        .expect("copied frame");
    e.apply(FrameCmd::Duplicate { from: 1, to: 2 })
        .expect("duplicated");
    assert_eq!(e.doc.frames.len(), 7);
    assert!(held_everywhere(&e, 1));
    assert_eq!(e.doc.layers[1].cel(6).get(1, 1), GREEN);
}

#[test]
fn frames_put_in_by_a_drag_show_the_reference_too() {
    let mut e = editor();
    let drag = CelDrag {
        layers: (0, 0),
        frames: (0, 1),
        to_layer: 0,
        to_frame: 3,
        kind: DragKind::Copy,
        landing: Landing::Insert,
    };
    e.apply(CelCmd::Drag(drag)).expect("dragged");
    assert_eq!(e.doc.frames.len(), 5);
    assert!(held_everywhere(&e, 1));
}

#[test]
fn a_hold_split_in_two_is_joined_again() {
    let mut e = editor();
    e.doc.set_holds(1, &[1, 1, 2]).expect("holds");
    e.doc.hold_references().expect("held");
    assert!(held_everywhere(&e, 1));
}

#[test]
fn a_one_frame_document_holds_nothing() {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
    place(&mut e).expect("placed");
    assert_eq!(e.doc.layers[1].link(0), 0);
}

#[test]
fn references_past_the_size_limits_are_refused() {
    let doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    assert!(doc
        .check_reference(MAX_REFERENCE_SIDE, MAX_REFERENCE_SIDE)
        .is_ok());
    for (w, h) in [
        (MAX_REFERENCE_SIDE + 1, 1),
        (1, MAX_REFERENCE_SIDE + 1),
        (0, 5),
    ] {
        assert_eq!(
            doc.check_reference(w, h),
            Err(LimitError::ReferenceTooLarge),
            "{w}x{h}"
        );
    }
}

#[test]
fn references_together_stay_within_the_memory_limit_and_shared_ones_count_once() {
    let mut e = editor();
    let one = e.doc.reference_bytes();
    assert!(one > 0);
    let layer = e.layer_id(1).expect("layer");
    e.apply(LayerCmd::Duplicate { layer }).expect("duplicated");
    assert_eq!(e.doc.reference_bytes(), one, "a copy shares the drawing");
    let room = MAX_REFERENCE_BYTES - one;
    let side = |bytes: usize| ((bytes / 4) as f64).sqrt() as usize;
    assert!(e
        .doc
        .check_reference(side(room) - 16, side(room) - 16)
        .is_ok());
    assert_eq!(
        e.doc.check_reference(side(room) + 32, side(room) + 32),
        Err(LimitError::ReferenceTooLarge)
    );
}

#[test]
fn placing_is_refused_at_the_layer_limit() {
    let mut e = editor();
    while e.doc.layers.len() < MAX_LAYERS {
        e.doc.add_layer(0, ColorMode::Rgba).expect("layer");
    }
    assert_eq!(place(&mut e), Err(LimitError::TooManyLayers));
}

/// A reference of the largest size, every tile drawn, all sharing one tile so the test is cheap.
pub(crate) fn largest(color: u32) -> Arc<Image> {
    use crate::tile::{Tile, TILE, TILE_AREA};
    let tile = Arc::new(Tile::pack(&[color; TILE_AREA]).expect("drawn"));
    let side = MAX_REFERENCE_SIDE / TILE;
    let cells = vec![Some(tile); side * side];
    Arc::new(Image::from_tiles(0, 0, side as u16, cells).expect("fits"))
}

#[test]
fn a_largest_reference_fills_the_memory_limit_so_no_other_fits() {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
    let command = LayerCmd::PlaceReference {
        below: 0,
        image: largest(GREEN),
        name: "Wall".into(),
    };
    e.apply(command).expect("exactly the limit");
    assert_eq!(e.doc.reference_bytes(), MAX_REFERENCE_BYTES);
    assert_eq!(
        e.doc.check_reference(16, 16),
        Err(LimitError::ReferenceTooLarge)
    );
    assert_eq!(place(&mut e), Err(LimitError::ReferenceTooLarge));
}

#[test]
fn frames_added_to_a_one_frame_document_show_the_reference_held() {
    for add in [
        Box::new(|e: &mut Editor| e.apply(FrameCmd::Add { at: 1, copy: false }))
            as Box<dyn Fn(&mut Editor) -> Result<crate::command::Outcome, LimitError>>,
        Box::new(|e: &mut Editor| e.apply(FrameCmd::Add { at: 0, copy: false })),
        Box::new(|e: &mut Editor| e.apply(FrameCmd::Duplicate { from: 0, to: 0 })),
    ] {
        let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
        place(&mut e).expect("placed");
        add(&mut e).expect("added");
        assert!(held_everywhere(&e, 1));
        assert_eq!(e.doc.layers[1].cel(1).get(1, 1), GREEN);
    }
}

#[test]
fn two_holds_on_a_reference_are_joined_into_one() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 3, copy: false })
        .expect("frame");
    e.doc.set_holds(1, &[1, 1, 2, 2]).expect("holds");
    e.doc.hold_references().expect("held");
    assert!(held_everywhere(&e, 1));
}

#[test]
fn a_frame_showing_something_else_is_given_the_drawing_back() {
    let mut e = editor();
    e.doc.set_holds(1, &[0, 0, 0]).expect("holds");
    e.doc
        .put_cel(1, 2, crate::layer::Cel::blank())
        .expect("cel");
    e.doc.hold_references().expect("held");
    assert_eq!(e.doc.layers[1].cel(2).get(1, 1), GREEN);
    assert!(held_everywhere(&e, 1));
}
