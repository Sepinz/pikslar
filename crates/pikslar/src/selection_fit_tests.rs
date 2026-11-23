//! The selected frames shown are the frames acted on: when a change leaves fewer frames, the
//! selection is clipped at once, and a selection with nothing left is never acted on.

use crate::test_app::{app, draw};
use pikslar_core::FrameCmd;

#[test]
fn a_change_removing_selected_frames_clips_the_selection_at_once() {
    let mut a = app(8);
    a.timeline.select(3, 7);
    a.run(FrameCmd::Remove { from: 5, to: 7 });
    assert_eq!(a.timeline.selected(), Some((3, 4)), "what is shown");
    assert_eq!(a.selected_frames(), Some((3, 4)), "what is acted on");
    a.remove_frames();
    assert_eq!(a.editor.doc().frames.len(), 3);
}

#[test]
fn undo_clips_the_selection_too() {
    let mut a = app(4);
    a.new_frame(false);
    a.timeline.select(2, 4);
    a.undo();
    assert_eq!(a.editor.doc().frames.len(), 4);
    assert_eq!(a.timeline.selected(), Some((2, 3)));
}

#[test]
fn a_selection_with_nothing_left_is_dropped() {
    let mut a = app(8);
    a.timeline.select(6, 7);
    a.run(FrameCmd::Remove { from: 5, to: 7 });
    assert_eq!(a.timeline.selected(), None);
}

#[test]
fn removing_frames_refuses_a_selection_entirely_past_the_end() {
    let mut a = app(5);
    a.set_frame(1);
    draw(&mut a);
    // As if the frames went by a way that did not clip it.
    a.timeline.select(6, 7);
    a.remove_frames();
    assert_eq!(
        a.editor.doc().frames.len(),
        5,
        "not the current frame instead"
    );
    assert_eq!(a.timeline.selected(), None);
    assert!(a.message.is_some(), "it says why");
}
