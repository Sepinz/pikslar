//! What undo and redo show, and what they must leave alone.

use crate::color::rgba;
use crate::document::{ColorMode, Document};
use crate::editor::{Editor, Tool};
use crate::mask::Mask;
use crate::selection::SelectOp;
use crate::{FrameCmd, LayerCmd, SelectionCmd, StepCmd};

const INK: u32 = rgba(10, 200, 30, 255);

fn editor(frames: usize, layers: usize) -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("valid"));
    for at in 1..frames {
        e.transact(|d| d.add_frame(at, false)).expect("room");
    }
    for at in 1..layers {
        e.transact(|d| d.add_layer(at, ColorMode::Rgba))
            .expect("room");
    }
    e.color = INK;
    e
}

fn dot(e: &mut Editor, layer: usize, frame: usize, x: i32) {
    e.layer = layer;
    e.frame = frame;
    e.begin(Tool::Pencil, x, 1).expect("begin");
    e.commit();
}

#[test]
fn an_undo_that_might_regrow_a_table_is_refused_without_memory() {
    let mut e = Editor::new(Document::new(8192, 8192, ColorMode::Rgba).expect("valid"));
    e.color = INK;
    dot(&mut e, 0, 0, 3);
    let before = e.doc.clone();
    e.set_memory_check(|_| false);
    assert_eq!(e.apply(StepCmd::Undo), Err(crate::LimitError::NoMemory));
    assert!(e.doc == before, "the document changed");
    e.set_memory_check(|_| true);
    assert!(e.apply(StepCmd::Undo).expect("room").changed);
    assert!(e.doc.layers[0].cel(0).is_blank());
    e.set_memory_check(|_| false);
    assert_eq!(e.apply(StepCmd::Redo), Err(crate::LimitError::NoMemory));
    assert!(e.doc.layers[0].cel(0).is_blank());
}

#[test]
fn a_long_scrub_of_one_image_asks_for_one_table() {
    fn at_most_one_table(bytes: usize) -> bool {
        bytes <= 3 << 20
    }
    let mut e = Editor::new(Document::new(8192, 8192, ColorMode::Rgba).expect("valid"));
    e.color = INK;
    (0..3).for_each(|x| dot(&mut e, 0, 0, x * 40));
    e.set_memory_check(at_most_one_table);
    e.apply(StepCmd::ScrubTo { depth: 0 })
        .expect("the steps share one image");
    assert_eq!(e.history.undo_depth(), 0);
    assert!(e.doc.layers[0].cel(0).is_blank());
}

/// An editor on a canvas of `side`, with two frames added as undoable steps.
fn three_frames(side: usize) -> Editor {
    let mut e = Editor::new(Document::new(side, side, ColorMode::Rgba).expect("valid"));
    for at in 1..3 {
        e.transact(|d| d.add_frame(at, false)).expect("room");
    }
    e.color = INK;
    e
}

fn at_most_one_large_table(bytes: usize) -> bool {
    bytes <= 3 << 20
}

#[test]
fn a_scrub_that_brings_frames_back_asks_for_the_images_it_redraws() {
    let mut e = three_frames(8192);
    (0..3).for_each(|frame| dot(&mut e, 0, frame, 40));
    let top = e.history.undo_depth();
    e.apply(StepCmd::ScrubTo { depth: 0 }).expect("room");
    assert_eq!(e.doc.frames.len(), 1);
    e.set_memory_check(at_most_one_large_table);
    let refused = e.apply(StepCmd::ScrubTo { depth: top });
    assert_eq!(refused, Err(crate::LimitError::NoMemory));
}

#[test]
fn a_scrub_counts_only_the_images_it_edits() {
    let mut e = three_frames(8192);
    (0..3).for_each(|x| dot(&mut e, 0, 0, x * 40));
    let keep = e.history.undo_depth() - 3;
    e.set_memory_check(at_most_one_large_table);
    e.apply(StepCmd::ScrubTo { depth: keep })
        .expect("one image is drawn on");
    assert_eq!(e.history.undo_depth(), keep);
}

#[test]
fn a_scrub_on_an_ordinary_canvas_never_asks_for_memory() {
    let mut e = three_frames(4096);
    (0..3).for_each(|frame| dot(&mut e, 0, frame, 40));
    e.set_memory_check(|_| false);
    assert!(
        e.apply(StepCmd::ScrubTo { depth: 0 })
            .expect("no check")
            .changed
    );
}

#[test]
fn a_scrub_asks_for_a_table_for_each_image_it_can_grow() {
    fn at_most_one_table(bytes: usize) -> bool {
        bytes <= 3 << 20
    }
    let mut doc = Document::new(8192, 8192, ColorMode::Rgba).expect("valid");
    (1..3).for_each(|at| doc.add_frame(at, false).expect("room"));
    let mut e = Editor::new(doc);
    e.color = INK;
    (0..3).for_each(|frame| dot(&mut e, 0, frame, 40));
    let before = e.doc.clone();
    e.set_memory_check(at_most_one_table);
    let refused = e.apply(StepCmd::ScrubTo { depth: 0 });
    assert_eq!(refused, Err(crate::LimitError::NoMemory));
    assert!(e.doc == before, "the document changed");
    assert!(
        e.apply(StepCmd::ScrubTo { depth: 2 })
            .expect("one table")
            .changed
    );
}

#[test]
fn undo_shows_the_frame_it_changed() {
    let mut e = editor(4, 1);
    dot(&mut e, 0, 1, 3);
    e.frame = 3;
    assert!(e.undo());
    assert_eq!(e.frame, 1, "the change must be on screen");
    assert_eq!(e.doc.layers[0].cel(1).get(3, 1), 0);
}

#[test]
fn undo_shows_the_layer_it_changed() {
    let mut e = editor(1, 3);
    dot(&mut e, 1, 0, 3);
    e.layer = 2;
    assert!(e.undo());
    assert_eq!(e.layer, 1);
}

#[test]
fn redo_shows_the_frame_it_changed() {
    let mut e = editor(4, 1);
    dot(&mut e, 0, 1, 3);
    assert!(e.undo());
    e.frame = 3;
    assert!(e.redo());
    assert_eq!(e.frame, 1);
}

#[test]
fn undo_stays_put_on_another_frame_of_the_same_held_drawing() {
    let mut e = editor(4, 1);
    e.transact(|d| d.hold_across(0, 1, 0, 3)).expect("room");
    dot(&mut e, 0, 1, 3);
    e.frame = 3;
    assert!(e.undo());
    assert_eq!(e.frame, 3, "this frame already shows the change");
}

#[test]
fn undoing_a_structural_change_goes_back_to_where_it_was_made() {
    let mut e = editor(4, 1);
    e.frame = 1;
    e.transact(|d| d.add_frame(2, false)).expect("room");
    e.frame = 4;
    assert!(e.undo());
    assert_eq!(e.frame, 1);
}

#[test]
fn undo_cannot_land_on_a_frame_that_no_longer_exists() {
    let mut e = editor(2, 1);
    e.frame = 1;
    e.transact(|d| d.add_frame(2, false)).expect("room");
    e.transact(|d| d.remove_frames(0, 1)).expect("room");
    e.frame = 0;
    assert!(e.undo());
    assert!(e.frame < e.doc.frames.len());
    assert!(e.undo());
    assert!(e.frame < e.doc.frames.len());
}

#[test]
fn undoing_something_else_leaves_layer_visibility_alone() {
    let mut e = editor(2, 2);
    e.transact(|d| d.add_frame(2, false)).expect("room");
    e.doc.layers[1].visible = false;
    assert!(e.undo());
    assert!(!e.doc.layers[1].visible, "undo revealed a hidden layer");
    e.doc.layers[0].visible = false;
    assert!(e.redo());
    assert!(!e.doc.layers[0].visible, "redo revealed a hidden layer");
    assert!(!e.doc.layers[1].visible);
}

#[test]
fn visibility_follows_its_layer_when_undo_reorders_layers() {
    let mut e = editor(1, 3);
    e.transact(|d| d.remove_layer(0)).expect("room");
    e.doc.layers[1].visible = false;
    assert!(e.undo());
    assert_eq!(e.doc.layers.len(), 3);
    assert!(e.doc.layers[0].visible, "the restored layer keeps its own");
    assert!(e.doc.layers[1].visible);
    assert!(!e.doc.layers[2].visible);
}

#[test]
fn opacity_is_one_undo_step_per_drag_and_comes_back() {
    let mut e = editor(1, 1);
    let depth = e.history.undo_depth();
    for value in [200, 150, 90] {
        e.apply(LayerCmd::SetOpacity {
            layer: e.layer_id(0).expect("layer"),
            opacity: value,
        })
        .expect("opacity");
    }
    assert_eq!(e.history.undo_depth(), depth + 1);
    assert!(e.undo());
    assert_eq!(e.doc.layers[0].opacity, 255);
    assert!(e.redo());
    assert_eq!(e.doc.layers[0].opacity, 90);
}

#[test]
fn a_second_opacity_drag_is_its_own_step() {
    let mut e = editor(1, 1);
    e.apply(LayerCmd::SetOpacity {
        layer: e.layer_id(0).expect("layer"),
        opacity: 100,
    })
    .expect("opacity");
    e.apply(StepCmd::EndDrag).expect("end");
    e.apply(LayerCmd::SetOpacity {
        layer: e.layer_id(0).expect("layer"),
        opacity: 200,
    })
    .expect("opacity");
    assert!(e.undo());
    assert_eq!(e.doc.layers[0].opacity, 100);
}

#[test]
fn undoing_something_else_leaves_opacity_where_it_was_set() {
    let mut e = editor(1, 1);
    e.apply(LayerCmd::SetOpacity {
        layer: e.layer_id(0).expect("layer"),
        opacity: 90,
    })
    .expect("opacity");
    dot(&mut e, 0, 0, 3);
    assert!(e.undo());
    assert_eq!(e.doc.layers[0].opacity, 90);
}

fn lifted(e: &mut Editor) {
    e.begin(Tool::Pencil, 5, 5).expect("begin");
    e.commit();
    e.select(Mask::rect(4, 4, 4, 4), SelectOp::Replace);
    e.lift(false);
}

fn left(e: &Editor) -> i32 {
    e.selection().map_or(-1, |m| m.bounds().0)
}

#[test]
fn undo_while_floating_takes_back_one_step_at_a_time() {
    let mut e = editor(1, 1);
    lifted(&mut e);
    e.mark_floating();
    e.move_floating(1, 0);
    e.mark_floating();
    e.move_floating(2, 0);
    assert_eq!(left(&e), 7);
    assert!(e.undo());
    assert_eq!(left(&e), 5);
    assert!(e.is_floating());
    assert!(e.redo());
    assert_eq!(left(&e), 7);
    assert!(e.undo());
    assert!(e.undo());
    assert_eq!(left(&e), 4);
    assert!(e.is_floating());
    assert!(e.undo());
    assert!(!e.is_floating(), "the last step cancels the lift");
}

#[test]
fn undo_while_floating_restores_the_shape_after_a_rotation() {
    let mut e = editor(1, 1);
    e.begin(Tool::Pencil, 4, 4).expect("begin");
    e.commit();
    e.select(Mask::rect(4, 4, 4, 2), SelectOp::Replace);
    e.lift(false);
    e.mark_floating();
    e.rotate(true);
    let turned = e.selection().map(|m| m.bounds());
    assert_ne!(turned, Some((4, 4, 4, 2)));
    assert!(e.undo());
    assert_eq!(e.selection().map(|m| m.bounds()), Some((4, 4, 4, 2)));
    e.anchor();
    assert_eq!(
        e.doc.layers[0].cel(0).get(4, 4),
        INK,
        "not rotated any more"
    );
}

#[test]
fn abandoning_a_change_keeps_what_redo_held() {
    let mut e = editor(1, 1);
    lifted(&mut e);
    e.mark_floating();
    e.move_floating(2, 0);
    assert!(e.undo());
    e.gesture_begin();
    e.gesture_end();
    assert!(e.redo());
    assert_eq!(left(&e), 6);
}

fn set_opacity(e: &mut Editor, opacity: u8) {
    let layer = e.layer_id(0).expect("layer");
    e.apply(LayerCmd::SetOpacity { layer, opacity })
        .expect("opacity");
}

#[test]
fn a_value_goes_on_in_its_step_across_a_command_that_changes_nothing() {
    let mut e = editor(2, 1);
    let depth = e.history.undo_depth();
    // Back where it began, the drag's step alters nothing, so finishing it keeps nothing.
    set_opacity(&mut e, 100);
    set_opacity(&mut e, 255);
    e.apply(FrameCmd::Move { from: 0, to: 0 }).expect("move");
    set_opacity(&mut e, 90);
    assert_eq!(e.history.undo_depth(), depth + 1, "one step for the drag");
    assert!(e.undo());
    assert_eq!(e.doc.layers[0].opacity, 255);
    assert!(e.redo());
    assert_eq!(e.doc.layers[0].opacity, 90);
}

#[test]
fn a_value_after_undo_starts_its_own_step_even_if_undo_did_nothing() {
    let mut e = editor(1, 1);
    set_opacity(&mut e, 100);
    e.apply(StepCmd::Redo).expect("redo");
    set_opacity(&mut e, 200);
    assert!(e.undo());
    assert_eq!(e.doc.layers[0].opacity, 100);
}

#[test]
fn a_value_after_a_change_to_something_else_starts_its_own_step() {
    let mut e = editor(2, 1);
    dot(&mut e, 0, 1, 3);
    set_opacity(&mut e, 100);
    e.apply(FrameCmd::Move { from: 0, to: 1 }).expect("move");
    set_opacity(&mut e, 200);
    assert!(e.undo());
    assert_eq!(e.doc.layers[0].opacity, 100);
    assert!(e.undo());
    assert_eq!(e.doc.layers[0].cel(1).get(3, 1), INK, "the move is undone");
    assert!(e.undo());
    assert_eq!(e.doc.layers[0].opacity, 255);
}

#[test]
fn a_timing_goes_on_in_its_step_across_a_command_that_changes_nothing() {
    let mut e = editor(3, 1);
    let before: Vec<u32> = e.doc.frames.iter().map(|f| f.duration_ms).collect();
    let retime = |e: &mut Editor, ms: u32| {
        e.apply(FrameCmd::Retime {
            from: 1,
            durations: vec![ms],
        })
        .expect("retime");
    };
    retime(&mut e, 300);
    retime(&mut e, before[1]);
    e.apply(FrameCmd::Move { from: 2, to: 2 }).expect("move");
    retime(&mut e, 500);
    assert!(e.undo());
    let after: Vec<u32> = e.doc.frames.iter().map(|f| f.duration_ms).collect();
    assert_eq!(after, before);
}

#[test]
fn a_layer_changed_and_removed_in_one_step_comes_back_as_it_was() {
    let mut e = editor(1, 2);
    let before = e.doc.layers[1].clone();
    e.transact(|d| {
        d.layers_mut()[1].name = "gone".into();
        d.layers_mut()[1].opacity = 7;
        d.remove_layer(1)
    })
    .expect("room");
    for _ in 0..2 {
        assert!(e.undo());
        assert_eq!(e.doc.layers[1].name, before.name);
        assert_eq!(e.doc.layers[1].opacity, before.opacity);
        assert!(e.redo());
        assert_eq!(e.doc.layers.len(), 1);
    }
}

#[test]
fn a_step_on_top_of_a_value_ends_the_value_and_keeps_less() {
    let mut e = editor(300, 1);
    dot(&mut e, 0, 0, 3);
    set_opacity(&mut e, 100);
    let open = e.history.memory_used();
    e.apply(SelectionCmd::All).expect("select");
    assert!(
        e.history.memory_used() < open,
        "the value's step was finished"
    );
    assert!(e.undo());
    assert!(e.undo());
    assert_eq!(e.doc.layers[0].opacity, 255);
}
