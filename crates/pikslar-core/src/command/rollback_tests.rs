//! Steps that go wrong part way, and floating pixels on a layer that is moved or removed:
//! the document and history come out exactly as they were, or as one whole step.

use super::*;
use crate::color::rgba;
use crate::document::Document;
use crate::mask::Mask;
use crate::selection::SelectOp;
use crate::Tool;

const INK: u32 = rgba(200, 10, 10, 255);

/// Two layers, drawn on both, a held pair of frames with a copy, a cel moved past the top left
/// corner, and a dot undone so that there is something to redo.
fn session() -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("valid"));
    e.color = INK;
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    for layer in 0..2 {
        e.layer = layer;
        e.begin(Tool::Rect, 0, 0).expect("begin");
        e.drag(9, 9);
        e.commit();
    }
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
    e.apply(FrameCmd::Add { at: 2, copy: true }).expect("room");
    let layer = e.layer_id(0).expect("layer");
    e.apply(FrameCmd::HoldPrevious { layer, frame: 2 })
        .expect("hold");
    e.apply(CelCmd::Move {
        layer,
        frame: 0,
        dx: -6,
        dy: -5,
    })
    .expect("moved");
    e.apply(StepCmd::EndDrag).expect("end");
    e.layer = 1;
    e.begin(Tool::Pencil, 14, 14).expect("begin");
    e.commit();
    assert!(e.undo());
    e
}

fn lifted(mut e: Editor) -> Editor {
    e.layer = 1;
    e.frame = 0;
    e.select(Mask::rect(2, 2, 4, 4), SelectOp::Replace);
    assert!(e.lift(false), "something to lift");
    e.move_floating(-5, 3);
    e
}

#[test]
fn a_change_that_fails_part_way_keeps_redo_and_memory() {
    let mut e = session();
    let before = e.doc.clone();
    let (undo, redo, memory) = (
        e.history.undo_depth(),
        e.history.redo_depth(),
        e.history.memory_used(),
    );
    let result: Result<(), LimitError> = e.transact(|d| {
        d.set_layer_mode(0, ColorMode::Palette)?;
        // Frame 2 holds frame 1's drawing, so removing it tidies the holds.
        d.remove_frame(2)?;
        d.remove_frame(0)?;
        d.add_frame(1, true)?;
        d.layers_mut()[0].opacity = 9;
        d.remove_layer(1)?;
        Err(LimitError::NoSuchItem)
    });
    assert_eq!(result, Err(LimitError::NoSuchItem));
    assert_eq!(e.doc, before);
    assert_eq!(e.history.undo_depth(), undo);
    assert_eq!(e.history.redo_depth(), redo);
    assert_eq!(e.history.memory_used(), memory);
    assert!(e.redo(), "what could be redone still can be");
}

/// True if frames 1 and 2 of the first layer still show one drawing, as the hold says.
fn hold_kept(e: &Editor) -> bool {
    let layer = &e.doc.layers[0];
    layer.link(1) != 0 && layer.link(1) == layer.link(2) && layer.cel(1).same_image(layer.cel(2))
}

/// Applies a change over floating pixels, then undoes and redoes it twice, checking each against
/// copies.
fn one_step_over_floating(lift: fn(Editor) -> Editor, change: impl FnOnce(&mut Editor)) {
    let mut e = lift(session());
    let before = e.doc.clone();
    let depth = e.history.undo_depth();
    change(&mut e);
    assert!(!e.is_floating());
    assert_eq!(
        e.history.undo_depth(),
        depth + 1,
        "the pixels and the change are one step"
    );
    let after = e.doc.clone();
    for _ in 0..2 {
        assert!(e.undo());
        assert_eq!(e.doc, before);
        assert!(hold_kept(&e));
        assert!(e.redo());
        assert_eq!(e.doc, after);
    }
}

#[test]
fn floating_pixels_on_a_layer_that_is_moved_go_with_it_as_one_step() {
    one_step_over_floating(lifted, |e| {
        let layer = e.layer_id(1).expect("layer");
        e.apply(LayerCmd::Move { layer, to: 0 }).expect("moved");
        assert_eq!(e.color_at(0, 5), INK, "the pixels landed");
    });
}

#[test]
fn floating_pixels_on_a_layer_that_is_removed_are_removed_with_it_as_one_step() {
    one_step_over_floating(lifted, |e| {
        let layer = e.layer_id(1).expect("layer");
        e.apply(LayerCmd::Remove { layer }).expect("removed");
    });
}

/// Floating pixels lifted from a held frame of the first layer.
fn lifted_from_hold(mut e: Editor) -> Editor {
    e.layer = 0;
    e.frame = 2;
    e.select(Mask::rect(1, 1, 5, 5), SelectOp::Replace);
    assert!(e.lift(false), "something to lift");
    e.move_floating(3, 2);
    e
}

#[test]
fn floating_pixels_put_down_on_a_held_frame_keep_the_hold_through_undo_and_redo() {
    one_step_over_floating(lifted_from_hold, |e| {
        e.apply(FrameCmd::Add { at: 3, copy: false }).expect("room");
        assert!(hold_kept(e), "both frames show the pixels put down");
    });
}

#[test]
fn a_dragged_value_then_a_command_undo_and_redo_one_at_a_time() {
    let mut e = session();
    let layer = e.layer_id(0).expect("layer");
    let start = e.doc.clone();
    e.apply(LayerCmd::SetOpacity { layer, opacity: 30 })
        .expect("set");
    e.apply(LayerCmd::SetOpacity { layer, opacity: 60 })
        .expect("set");
    let dragged = e.doc.clone();
    // No end of the drag before the next command.
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
    let end = e.doc.clone();
    for _ in 0..2 {
        assert!(e.undo());
        assert_eq!(e.doc, dragged);
        assert!(e.undo());
        assert_eq!(e.doc, start);
        assert!(e.redo());
        assert_eq!(e.doc, dragged);
        assert!(e.redo());
        assert_eq!(e.doc, end);
    }
}
