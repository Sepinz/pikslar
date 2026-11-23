//! Moving a cel: one small step, every pixel kept, holds moved together, the range respected.

use super::{CelCmd, FloatingCmd, FrameCmd, SelectionCmd, StepCmd};
use crate::color::rgba;
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::{Editor, Tool};
use crate::layer::{Cel, OFFSET_LIMIT};
use crate::mask::Mask;
use crate::offset_tests::{image_of, world};
use crate::selection::SelectOp;
use crate::DrawCmd;
use std::collections::{BTreeMap, HashMap};

const RED: u32 = rgba(200, 10, 10, 255);

/// A 32x32 editor whose one cel has pixels on the canvas and past every edge.
fn editor() -> Editor {
    let mut e = Editor::new(Document::new(32, 32, ColorMode::Rgba).expect("valid"));
    let pixels = HashMap::from([
        ((3, 4), RED),
        ((-5, 10), RED),
        ((40, -2), RED),
        ((31, 31), RED),
    ]);
    e.doc.layers[0]
        .put_cel(0, Cel::from_image(image_of(&pixels)))
        .expect("frame");
    e.color = RED;
    e
}

fn moved(e: &mut Editor, frame: usize, dx: i32, dy: i32) -> bool {
    let layer = e.layer_id(0).expect("layer");
    let command = CelCmd::Move {
        layer,
        frame,
        dx,
        dy,
    };
    e.apply(command).expect("move").changed
}

fn shifted(pixels: &BTreeMap<(i32, i32), u32>, dx: i32, dy: i32) -> BTreeMap<(i32, i32), u32> {
    pixels
        .iter()
        .map(|(&(x, y), &p)| ((x + dx, y + dy), p))
        .collect()
}

fn pixels(e: &Editor, frame: usize) -> BTreeMap<(i32, i32), u32> {
    world(e.doc.layers[0].cel(frame))
}

#[test]
fn a_move_keeps_every_pixel_and_undoes_and_redoes() {
    let mut e = editor();
    let start = pixels(&e, 0);
    let image = e.doc.layers[0].cel(0).image() as *const _;
    assert!(moved(&mut e, 0, -7, 50));
    assert_eq!(pixels(&e, 0), shifted(&start, -7, 50));
    assert_eq!(e.doc.layers[0].cel(0).position(), (-7, 50));
    assert!(
        std::ptr::eq(image, e.doc.layers[0].cel(0).image()),
        "no pixel was copied"
    );
    e.apply(StepCmd::EndDrag).expect("end");
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(pixels(&e, 0), start);
    e.apply(StepCmd::Redo).expect("redo");
    assert_eq!(pixels(&e, 0), shifted(&start, -7, 50));
}

#[test]
fn a_drag_is_one_step_and_a_drag_back_to_the_start_is_none() {
    let mut e = editor();
    let start = pixels(&e, 0);
    for (dx, dy) in [(1, 0), (3, -2), (-1, 9)] {
        moved(&mut e, 0, dx, dy);
    }
    e.apply(StepCmd::EndDrag).expect("end");
    assert_eq!(e.history().undo_depth(), 1);
    assert_eq!(pixels(&e, 0), shifted(&start, 3, 7));
    let revision = e.revision();
    moved(&mut e, 0, 5, 5);
    moved(&mut e, 0, -5, -5);
    e.apply(StepCmd::EndDrag).expect("end");
    assert_eq!(
        e.history().undo_depth(),
        1,
        "a drag that went nowhere leaves no step"
    );
    assert_eq!(e.revision(), revision, "and counts as no change");
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(pixels(&e, 0), start);
}

#[test]
fn escape_puts_the_cel_back_and_leaves_nothing_to_redo() {
    let mut e = editor();
    let start = pixels(&e, 0);
    moved(&mut e, 0, 10, 0);
    moved(&mut e, 0, 10, 4);
    e.apply(StepCmd::AbortJoin).expect("abort");
    assert_eq!(pixels(&e, 0), start);
    assert!(!e.history().can_undo() && !e.history().can_redo());
}

#[test]
fn a_held_frame_moves_its_whole_hold_and_nothing_else() {
    let mut e = editor();
    let layer = e.layer_id(0).expect("layer");
    for at in 1..4 {
        e.apply(FrameCmd::Add { at, copy: true }).expect("frame");
    }
    e.apply(FrameCmd::HoldAcross {
        layer,
        source: 0,
        from: 0,
        to: 2,
    })
    .expect("hold");
    let start = pixels(&e, 0);
    moved(&mut e, 1, 4, -4);
    for frame in 0..3 {
        assert_eq!(pixels(&e, frame), shifted(&start, 4, -4), "frame {frame}");
    }
    assert_eq!(pixels(&e, 3), start, "a frame outside the hold stays");
    assert!(
        e.doc.layers[0].cel(0).same_image(e.doc.layers[0].cel(2)),
        "still one drawing"
    );
    e.apply(StepCmd::EndDrag).expect("end");
    e.apply(StepCmd::Undo).expect("undo");
    for frame in 0..4 {
        assert_eq!(pixels(&e, frame), start, "frame {frame} after undo");
    }
}

#[test]
fn a_move_stops_at_the_range() {
    let mut e = editor();
    assert!(moved(&mut e, 0, i32::MAX, i32::MIN));
    assert_eq!(
        e.doc.layers[0].cel(0).position(),
        (OFFSET_LIMIT as i16, -OFFSET_LIMIT as i16)
    );
    assert!(!moved(&mut e, 0, 1, -1), "no further");
    assert!(moved(&mut e, 0, -2 * OFFSET_LIMIT - 5, 0));
    assert_eq!(e.doc.layers[0].cel(0).position().0, -OFFSET_LIMIT as i16);
}

#[test]
fn a_move_names_a_layer_and_frame_that_exist() {
    let mut e = editor();
    let layer = e.layer_id(0).expect("layer");
    let far = CelCmd::Move {
        layer,
        frame: 9,
        dx: 1,
        dy: 1,
    };
    assert_eq!(e.apply(far), Err(LimitError::NoSuchItem));
    let gone = CelCmd::Move {
        layer: crate::LayerId(u32::MAX),
        frame: 0,
        dx: 1,
        dy: 1,
    };
    assert_eq!(e.apply(gone), Err(LimitError::NoSuchItem));
}

#[test]
fn a_move_step_is_small() {
    let mut e = editor();
    let before = e.history().steps_used();
    moved(&mut e, 0, 3, 3);
    // Its room in history's list, and the frame it names.
    let slot = size_of::<(crate::history::Edit, usize)>();
    assert_eq!(e.history().steps_used() - before, slot + 4);
}

#[test]
fn moving_while_pixels_float_is_one_step_with_them() {
    let mut e = editor();
    let start = pixels(&e, 0);
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
    moved(&mut e, 0, 2, 2);
    moved(&mut e, 0, 1, 1);
    e.apply(StepCmd::EndDrag).expect("end");
    assert!(e.floating.is_none());
    assert_eq!(e.history().undo_depth(), depth + 1);
    let mut expected = shifted(&start, 3, 3);
    expected.remove(&(6, 7));
    expected.insert((11, 7), RED);
    assert_eq!(
        pixels(&e, 0),
        expected,
        "the lifted pixel went down, then moved"
    );
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(pixels(&e, 0), start);
}

#[test]
fn moving_off_and_back_keeps_the_picture_through_strokes() {
    let mut e = editor();
    moved(&mut e, 0, -20, 0);
    e.apply(StepCmd::EndDrag).expect("end");
    e.apply(DrawCmd::Stroke {
        tool: Tool::Eraser,
        path: vec![(-40, 0), (40, 31)],
    })
    .expect("erase");
    e.apply(DrawCmd::Stroke {
        tool: Tool::Fill,
        path: vec![(20, 20)],
    })
    .expect("fill");
    let painted = pixels(&e, 0);
    let off_canvas = |p: &BTreeMap<(i32, i32), u32>| {
        p.iter()
            .filter(|((x, y), _)| !(0..32).contains(x) || !(0..32).contains(y))
            .map(|(&k, &v)| (k, v))
            .collect::<Vec<_>>()
    };
    assert!(
        off_canvas(&painted).contains(&((-25, 10), RED)),
        "the far pixel survived"
    );
    moved(&mut e, 0, 20, 0);
    e.apply(StepCmd::EndDrag).expect("end");
    assert_eq!(pixels(&e, 0), shifted(&painted, 20, 0));
    e.apply(StepCmd::ScrubTo { depth: 0 }).expect("scrub");
    assert_eq!(pixels(&e, 0), pixels(&editor(), 0));
}

#[test]
fn a_hold_at_unequal_offsets_moves_as_far_as_its_nearest_member_allows() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("frame");
    let near_edge = OFFSET_LIMIT - 10;
    let image = image_of(&HashMap::from([((0, 0), RED)]));
    let layer = &mut e.doc.layers[0];
    layer
        .put_cel(0, Cel::at(image.clone(), 0, 0).expect("in range"))
        .expect("frame");
    layer
        .put_cel(1, Cel::at(image, near_edge, 3).expect("in range"))
        .expect("frame");
    layer.set_holds(&[1, 1], 0).expect("hold");
    assert!(moved(&mut e, 0, 25, 0));
    assert_eq!(e.doc.layers[0].cel(0).position(), (10, 0));
    assert_eq!(e.doc.layers[0].cel(1).position(), (OFFSET_LIMIT as i16, 3));
}
