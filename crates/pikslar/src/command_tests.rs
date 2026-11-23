//! How the window drives commands: when a dragged value ends, and what a refused change leaves.

use crate::test_app::{app, draw};
use pikslar_core::{FloatingCmd, LayerCmd, Mask, SelectOp, SelectionCmd, StepCmd};
use pikslar_platform::Button;

fn set_opacity(a: &mut crate::app::App, opacity: u8) {
    let layer = a.editor.layer_id(0).expect("layer");
    a.run(LayerCmd::SetOpacity { layer, opacity });
}

#[test]
fn letting_go_of_a_slider_ends_its_step_even_at_the_same_value() {
    let mut a = app(1);
    let depth = a.editor.history().undo_depth();
    a.ui.down = true;
    set_opacity(&mut a, 40);
    a.pointer_up(5, 5, Button::Left);
    a.ui.down = true;
    set_opacity(&mut a, 90);
    a.pointer_up(5, 5, Button::Left);
    assert_eq!(a.editor.history().undo_depth(), depth + 2);
}

#[test]
fn letting_go_while_naming_a_tag_keeps_the_name_joined() {
    let mut a = app(1);
    a.tag_key();
    let depth = a.editor.history().undo_depth();
    assert!(a.timeline.naming.is_some());
    a.ui.down = true;
    a.pointer_up(5, 5, Button::Left);
    a.cancel_naming();
    assert_eq!(a.editor.history().undo_depth(), depth - 1, "no tag is left");
}

fn with_pixels_that_cannot_be_applied(frames: usize) -> crate::app::App {
    let mut a = app(frames);
    a.run(SelectionCmd::Select {
        shape: Box::new(Mask::rect(0, 0, 4, 4)),
        op: SelectOp::Replace,
    });
    a.run(FloatingCmd::Paste {
        clip: std::sync::Arc::new(pikslar_core::Clip::from_image(1, 1, vec![0xff00_00ff])),
        x: 1,
        y: 1,
    });
    assert!(a.editor.is_floating());
    a.editor.set_memory_check(|_| false);
    a
}

#[test]
fn a_frame_change_waits_for_floating_pixels_that_could_not_be_applied() {
    let mut a = with_pixels_that_cannot_be_applied(2);
    a.set_frame(1);
    assert_eq!(a.editor.frame, 0);
    assert!(a.editor.is_floating());
    draw(&mut a);
}

#[test]
fn playback_waits_for_floating_pixels_that_could_not_be_applied() {
    let mut a = with_pixels_that_cannot_be_applied(2);
    a.toggle_play();
    assert!(!a.playing);
    a.editor.set_memory_check(|_| true);
    a.toggle_play();
    assert!(a.playing);
    assert!(!a.editor.is_floating(), "playing applies them first");
}

#[test]
fn pixels_that_could_not_be_applied_count_as_unsaved_work() {
    let mut a = with_pixels_that_cannot_be_applied(1);
    a.saved_revision = a.editor.revision();
    assert!(!a.settle());
    assert!(
        a.is_modified(),
        "moved floating pixels differ from what was saved"
    );
    assert!(a.has_unsaved_work());
}

#[test]
fn a_recovery_copy_is_due_again_after_a_cancelled_tag_and_a_stroke() {
    let mut a = app(1);
    a.tag_key();
    let during = a.recovery_stamp();
    a.cancel_naming();
    a.run(pikslar_core::DrawCmd::Stroke {
        tool: pikslar_core::Tool::Pencil,
        path: vec![(3, 3)],
    });
    assert_ne!(a.recovery_stamp(), during);
}

#[test]
fn a_value_dragged_after_a_save_cannot_rewind_past_it() {
    let mut a = app(1);
    let opacity = a.editor.doc().layers()[0].opacity;
    set_opacity(&mut a, 40);
    assert!(a.ready_to_save());
    a.saved_revision = a.editor.revision();
    set_opacity(&mut a, 90);
    set_opacity(&mut a, 40);
    a.run(StepCmd::EndDrag);
    assert!(!a.is_modified(), "back where it was saved");
    set_opacity(&mut a, opacity);
    a.run(StepCmd::EndDrag);
    assert!(a.is_modified());
}

#[test]
fn something_queued_by_a_queued_action_still_runs() {
    let mut a = app(1);
    a.defer(|a| a.defer(|a| a.playing = true));
    draw(&mut a);
    assert!(a.playing);
}
