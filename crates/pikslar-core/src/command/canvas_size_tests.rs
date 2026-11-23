//! Canvas size and crop: every cel moves, no pixel is lost, and undo brings back the canvas.

use super::{anchor_shift, crop_to, CanvasCmd, FloatingCmd, FrameCmd, SelectionCmd, StepCmd};
use crate::color::rgba;
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::Editor;
use crate::layer::{Cel, OFFSET_LIMIT};
use crate::mask::Mask;
use crate::offset_tests::{image_of, world};
use crate::selection::SelectOp;
use std::collections::{BTreeMap, HashMap};

const RED: u32 = rgba(200, 10, 10, 255);

/// A 32x32 editor with two layers and two frames held together on the first.
fn editor() -> Editor {
    let mut e = Editor::new(Document::new(32, 32, ColorMode::Rgba).expect("valid"));
    let pixels = HashMap::from([((3, 4), RED), ((31, 31), RED), ((-5, 10), RED)]);
    e.doc.layers[0]
        .put_cel(0, Cel::from_image(image_of(&pixels)))
        .expect("frame");
    e.doc.add_layer(1, ColorMode::Rgba).expect("layer");
    e.doc.layers[1]
        .put_cel(
            0,
            Cel::from_image(image_of(&HashMap::from([((8, 8), RED)]))),
        )
        .expect("frame");
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("frame");
    let layer = e.layer_id(0).expect("layer");
    e.apply(FrameCmd::HoldPrevious { layer, frame: 1 })
        .expect("hold");
    e
}

fn everything(e: &Editor) -> Vec<BTreeMap<(i32, i32), u32>> {
    e.doc
        .layers
        .iter()
        .flat_map(|l| l.cels().iter().map(world))
        .collect()
}

fn shifted(all: &[BTreeMap<(i32, i32), u32>], dx: i32, dy: i32) -> Vec<BTreeMap<(i32, i32), u32>> {
    all.iter()
        .map(|m| {
            m.iter()
                .map(|(&(x, y), &p)| ((x + dx, y + dy), p))
                .collect()
        })
        .collect()
}

fn resize(e: &mut Editor, width: usize, height: usize, dx: i32, dy: i32) -> bool {
    let command = CanvasCmd::Resize {
        width,
        height,
        dx,
        dy,
    };
    e.apply(command).expect("resize").changed
}

#[test]
fn every_cel_moves_with_the_canvas_and_undo_brings_back_both() {
    let mut e = editor();
    let start = everything(&e);
    let before = e.doc.clone();
    assert!(resize(&mut e, 50, 40, 9, 4));
    assert_eq!((e.doc.width, e.doc.height), (50, 40));
    assert_eq!(everything(&e), shifted(&start, 9, 4));
    assert_eq!(e.overlay.size(), (50, 40), "the overlay fits the canvas");
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(e.doc, before);
    assert_eq!(e.overlay.size(), (32, 32));
    e.apply(StepCmd::Redo).expect("redo");
    assert_eq!((e.doc.width, e.doc.height), (50, 40));
    assert_eq!(everything(&e), shifted(&start, 9, 4));
    assert_eq!(e.overlay.size(), (50, 40));
}

#[test]
fn a_smaller_canvas_keeps_the_pixels_outside_it_for_a_larger_one_later() {
    let mut e = editor();
    let start = everything(&e);
    assert!(resize(&mut e, 10, 10, -2, -2));
    assert_eq!(everything(&e), shifted(&start, -2, -2), "nothing removed");
    assert!(resize(&mut e, 32, 32, 2, 2));
    assert_eq!(everything(&e), start);
}

#[test]
fn holds_and_shared_drawings_stay_as_they_were() {
    let mut e = editor();
    let links: Vec<u32> = e.doc.layers[0].links().collect();
    assert!(resize(&mut e, 40, 40, 3, 3));
    let layer = &e.doc.layers[0];
    assert_eq!(layer.links().collect::<Vec<_>>(), links);
    assert!(layer.cel(0).same_image(layer.cel(1)));
}

#[test]
fn a_size_out_of_range_or_a_cel_past_the_offsets_changes_nothing() {
    let mut e = editor();
    let before = e.doc.clone();
    let depth = e.history().undo_depth();
    for (w, h) in [(0, 10), (10, 0), (8193, 10), (10, 8193)] {
        let refused = e.apply(CanvasCmd::Resize {
            width: w,
            height: h,
            dx: 0,
            dy: 0,
        });
        assert_eq!(refused, Err(LimitError::Dimensions), "{w} by {h}");
    }
    let far = CanvasCmd::Resize {
        width: 32,
        height: 32,
        dx: OFFSET_LIMIT + 1,
        dy: 0,
    };
    assert_eq!(e.apply(far), Err(LimitError::CanvasReach));
    assert_eq!(e.doc, before);
    assert_eq!(e.history().undo_depth(), depth);
}

#[test]
fn the_same_canvas_is_no_step() {
    let mut e = editor();
    let depth = e.history().undo_depth();
    assert!(!resize(&mut e, 32, 32, 0, 0));
    assert_eq!(e.history().undo_depth(), depth);
}

#[test]
fn the_selection_is_cleared_and_comes_back_with_undo() {
    let mut e = editor();
    let shape = Box::new(Mask::rect(2, 2, 6, 6));
    e.apply(SelectionCmd::Select {
        shape,
        op: SelectOp::Replace,
    })
    .expect("select");
    let selected = e.selection().cloned();
    assert!(resize(&mut e, 20, 20, 0, 0));
    assert!(e.selection().is_none());
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(e.selection().cloned(), selected);
}

#[test]
fn floating_pixels_are_put_down_first_in_the_same_step() {
    let mut e = editor();
    let shape = Box::new(Mask::rect(0, 0, 8, 8));
    e.apply(SelectionCmd::Select {
        shape,
        op: SelectOp::Replace,
    })
    .expect("select");
    e.apply(FloatingCmd::Lift { copy: false }).expect("lift");
    e.apply(FloatingCmd::Move {
        dx: 5,
        dy: 0,
        join: false,
    })
    .expect("float");
    let depth = e.history().undo_depth();
    assert!(resize(&mut e, 40, 40, 1, 1));
    assert!(e.floating.is_none());
    assert_eq!(e.history().undo_depth(), depth + 1);
    assert_eq!(world(e.doc.layers[0].cel(0)).get(&(9, 5)), Some(&RED));
}

fn set_blending(e: &mut Editor, blending: crate::Blending) -> bool {
    e.apply(CanvasCmd::SetBlending { blending })
        .expect("blending")
        .changed
}

#[test]
fn blending_is_one_step_that_undo_and_redo_bring_back() {
    use crate::Blending;
    let mut e = editor();
    let (before, depth) = (e.doc.clone(), e.history().undo_depth());
    let pixels = everything(&e);
    assert!(set_blending(&mut e, Blending::Aseprite));
    assert_eq!(e.doc.blending, Blending::Aseprite);
    assert_eq!(e.history().undo_depth(), depth + 1);
    assert_eq!(everything(&e), pixels, "no pixel changes");
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(e.doc, before);
    e.apply(StepCmd::Redo).expect("redo");
    assert_eq!(e.doc.blending, Blending::Aseprite);
}

#[test]
fn the_same_blending_is_no_step() {
    let mut e = editor();
    let depth = e.history().undo_depth();
    assert!(!set_blending(&mut e, crate::Blending::Pikslar));
    assert_eq!(e.history().undo_depth(), depth);
}

#[test]
fn anchors_place_the_old_canvas_at_start_middle_or_end() {
    assert_eq!(anchor_shift((32, 32), (50, 40), (0, 0)), (0, 0));
    assert_eq!(anchor_shift((32, 32), (50, 40), (1, 1)), (9, 4));
    assert_eq!(anchor_shift((32, 32), (50, 40), (2, 2)), (18, 8));
    assert_eq!(
        anchor_shift((32, 32), (21, 32), (1, 2)),
        (-6, 0),
        "rounds down"
    );
    assert_eq!(anchor_shift((32, 32), (16, 8), (2, 1)), (-16, -12));
}

#[test]
fn a_crop_keeps_the_rectangle_and_moves_it_to_the_corner() {
    let mut e = editor();
    let start = everything(&e);
    e.apply(crop_to(3, 4, 6, 5)).expect("crop");
    assert_eq!((e.doc.width, e.doc.height), (6, 5));
    assert_eq!(everything(&e), shifted(&start, -3, -4));
    assert_eq!(world(e.doc.layers[0].cel(0)).get(&(0, 0)), Some(&RED));
}
