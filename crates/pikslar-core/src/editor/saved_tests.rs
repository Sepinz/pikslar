//! Whether the document is where it was saved, by the place in its history rather than by the
//! revision, which undo and redo advance. Anything unclear must read as changed.

use super::*;
use crate::color::rgba;
use crate::document::{ColorMode, Document};
use crate::{LayerCmd, StepCmd};

fn editor() -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("valid"));
    e.color = rgba(255, 0, 0, 255);
    e.pixel_perfect = false;
    e
}

fn dot(e: &mut Editor, x: i32) {
    e.begin(Tool::Pencil, x, 1).expect("begin");
    e.commit();
}

fn saved(e: &mut Editor) {
    let copy = e.begin_saved_copy();
    e.finish_saved_copy(copy);
}

#[test]
fn nothing_is_saved_until_it_is_said_so() {
    let mut e = editor();
    assert!(!e.is_at_saved());
    e.mark_saved();
    assert!(e.is_at_saved());
}

#[test]
fn an_undone_stroke_is_back_at_the_saved_place() {
    let mut e = editor();
    e.mark_saved();
    dot(&mut e, 1);
    assert!(!e.is_at_saved());
    e.undo();
    assert!(e.is_at_saved());
    e.redo();
    assert!(!e.is_at_saved());
}

#[test]
fn a_save_of_a_copy_is_the_place_of_the_copy() {
    let mut e = editor();
    dot(&mut e, 1);
    let copy = e.begin_saved_copy();
    dot(&mut e, 2);
    e.finish_saved_copy(copy);
    assert!(!e.is_at_saved());
    e.undo();
    assert!(e.is_at_saved());
    e.undo();
    assert!(!e.is_at_saved());
}

#[test]
fn a_copy_whose_place_was_dropped_before_the_save_ended_is_never_saved() {
    let mut e = editor();
    dot(&mut e, 1);
    let copy = e.begin_saved_copy();
    e.undo();
    dot(&mut e, 2);
    e.finish_saved_copy(copy);
    assert!(!e.is_at_saved());
    e.undo();
    e.redo();
    assert!(!e.is_at_saved());
}

#[test]
fn a_copy_that_is_not_the_newest_saves_nothing() {
    let mut e = editor();
    let old = e.begin_saved_copy();
    dot(&mut e, 1);
    let new = e.begin_saved_copy();
    e.finish_saved_copy(old);
    assert!(!e.is_at_saved());
    e.finish_saved_copy(new);
    assert!(e.is_at_saved());
}

#[test]
fn a_new_edit_after_undoing_past_the_saved_place_drops_it_for_good() {
    let mut e = editor();
    dot(&mut e, 1);
    saved(&mut e);
    e.undo();
    dot(&mut e, 2);
    assert!(!e.is_at_saved());
    e.undo();
    assert!(!e.is_at_saved(), "the picture before the saved one");
    e.redo();
    dot(&mut e, 3);
    assert!(!e.is_at_saved());
}

#[test]
fn edits_beyond_the_saved_place_do_not_disturb_it() {
    let mut e = editor();
    dot(&mut e, 1);
    saved(&mut e);
    dot(&mut e, 2);
    dot(&mut e, 3);
    e.undo();
    e.undo();
    assert!(e.is_at_saved());
}

#[test]
fn a_change_outside_the_history_drops_the_saved_place() {
    let mut e = editor();
    dot(&mut e, 1);
    saved(&mut e);
    let layer = e.layer_id(0).expect("layer");
    e.apply(LayerCmd::SetVisible {
        layer,
        visible: false,
    })
    .expect("hide");
    assert!(!e.is_at_saved());
    e.undo();
    e.redo();
    assert!(!e.is_at_saved());
}

#[test]
fn a_value_that_goes_on_in_its_step_drops_the_saved_place_of_that_step() {
    let mut e = editor();
    let layer = e.layer_id(0).expect("layer");
    let opacity = |e: &mut Editor, opacity| {
        e.apply(LayerCmd::SetOpacity { layer, opacity })
            .expect("opacity")
    };
    opacity(&mut e, 40);
    saved(&mut e);
    opacity(&mut e, 90);
    assert!(!e.is_at_saved());
    e.undo();
    e.redo();
    assert!(!e.is_at_saved());
}

#[test]
fn a_step_that_came_to_nothing_leaves_the_place_before_it_saved() {
    let mut e = editor();
    let layer = e.layer_id(0).expect("layer");
    let opacity = |e: &mut Editor, opacity| {
        e.apply(LayerCmd::SetOpacity { layer, opacity })
            .expect("opacity")
    };
    opacity(&mut e, 40);
    e.apply(StepCmd::EndDrag).expect("end");
    saved(&mut e);
    opacity(&mut e, 90);
    opacity(&mut e, 40);
    e.apply(StepCmd::EndDrag).expect("end");
    assert!(e.is_at_saved());
}

#[test]
fn trimming_the_oldest_steps_drops_a_place_beneath_what_is_kept() {
    let mut e = Editor::with_undo_budget(Document::new(16, 16, ColorMode::Rgba).expect("ok"), 200);
    e.color = rgba(255, 0, 0, 255);
    e.pixel_perfect = false;
    e.mark_saved();
    for x in 0..12 {
        dot(&mut e, x);
    }
    while e.history().can_undo() {
        e.undo();
    }
    assert!(e.history().rewind_marks().below > 0, "steps were trimmed");
    assert!(!e.is_at_saved());
}

#[test]
fn pixels_floating_while_copying_leave_nothing_saved() {
    let mut e = editor();
    dot(&mut e, 1);
    e.apply(crate::SelectionCmd::Select {
        shape: Box::new(crate::Mask::rect(1, 1, 1, 1)),
        op: crate::SelectOp::Replace,
    })
    .expect("select");
    e.apply(crate::FloatingCmd::Lift { copy: true })
        .expect("lift");
    e.apply(crate::FloatingCmd::Move {
        dx: 3,
        dy: 0,
        join: false,
    })
    .expect("move");
    assert!(e.is_floating());
    let copy = e.begin_saved_copy();
    e.finish_saved_copy(copy);
    assert!(!e.is_at_saved());
}

#[test]
fn forgetting_drops_the_place() {
    let mut e = editor();
    e.mark_saved();
    e.forget_saved();
    assert!(!e.is_at_saved());
}

#[test]
fn a_write_that_ends_replaces_the_place_the_file_held_before_even_if_its_own_was_dropped() {
    let mut e = editor();
    dot(&mut e, 1);
    saved(&mut e);
    dot(&mut e, 2);
    let copy = e.begin_saved_copy();
    e.undo();
    dot(&mut e, 3);
    e.finish_saved_copy(copy);
    assert!(
        !e.is_at_saved(),
        "the file holds the copy, not the first save"
    );
    e.undo();
    assert!(!e.is_at_saved());
}

#[test]
fn a_write_of_a_state_that_could_not_be_named_leaves_nothing_saved() {
    let mut e = editor();
    saved(&mut e);
    e.apply(crate::SelectionCmd::Select {
        shape: Box::new(crate::Mask::rect(1, 1, 1, 1)),
        op: crate::SelectOp::Replace,
    })
    .expect("select");
    e.apply(crate::FloatingCmd::Lift { copy: true })
        .expect("lift");
    let copy = e.begin_saved_copy();
    e.finish_saved_copy(copy);
    e.apply(crate::FloatingCmd::Revert).expect("put back");
    assert!(!e.is_at_saved());
}
