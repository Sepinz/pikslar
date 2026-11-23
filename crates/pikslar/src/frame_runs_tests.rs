//! Several frames at once, through the strip, the keys and the tag row.

use crate::app::App;
use crate::test_app::{app, click, draw, durations, range, PLAIN, SHIFT};
use pikslar_core::{FrameCmd, StepCmd, TagCmd};
use pikslar_platform::{Button, Event, Handler, Window};

/// Five frames timed 10 to 50 ms so each can be followed, with frames 2 and 3 selected.
fn timed() -> App {
    let mut a = app(5);
    a.editor
        .apply(FrameCmd::Retime {
            from: 0,
            durations: vec![10, 20, 30, 40, 50],
        })
        .expect("retime");
    range(&mut a, 1, 2);
    assert_eq!(a.timeline.selected(), Some((1, 2)));
    a
}

fn strip_at(a: &App, frame: usize) -> (i32, i32) {
    let strip = a.timeline.strip;
    (strip.x + frame as i32 * 34 + 10, strip.y + 5)
}

/// Presses at one point, moves to another and lets go, as a hand would.
fn drag(a: &mut App, from: (i32, i32), to: (i32, i32)) {
    a.pointer_down(from.0, from.1, Button::Left, PLAIN);
    draw(a);
    a.event(Event::Move { x: to.0, y: to.1 }, Window::detached());
    draw(a);
    a.pointer_up(to.0, to.1, Button::Left);
    draw(a);
    draw(a);
}

#[test]
fn dragging_selected_frames_moves_them_as_one_step() {
    let mut a = timed();
    let depth = a.editor.history().undo_depth();
    let (from, to) = (strip_at(&a, 2), strip_at(&a, 4));
    drag(&mut a, from, to);
    assert_eq!(durations(&a), [10, 40, 50, 20, 30]);
    assert_eq!(a.timeline.selected(), Some((3, 4)), "they stay selected");
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    a.editor.apply(StepCmd::Undo).expect("undo");
    assert_eq!(durations(&a), [10, 20, 30, 40, 50]);
}

#[test]
fn a_click_on_a_selected_frame_goes_there_and_forgets_the_selection() {
    let mut a = timed();
    let at = strip_at(&a, 1);
    click(&mut a, at, PLAIN);
    assert_eq!(a.editor.frame, 1);
    assert_eq!(a.timeline.selected(), None);
    assert_eq!(durations(&a), [10, 20, 30, 40, 50]);
}

#[test]
fn shift_on_a_selected_frame_still_changes_the_selection() {
    let mut a = timed();
    let at = strip_at(&a, 4);
    click(&mut a, at, SHIFT);
    assert_eq!(a.timeline.selected(), Some((1, 4)));
    // Shift on a frame already selected moves the selection's end there; it does not drag.
    let at = strip_at(&a, 2);
    click(&mut a, at, SHIFT);
    assert_eq!(a.timeline.selected(), Some((1, 2)));
}

#[test]
fn duplicating_copies_every_selected_frame_and_selects_the_copies() {
    let mut a = timed();
    a.new_frame(true);
    assert_eq!(durations(&a), [10, 20, 30, 20, 30, 40, 50]);
    assert_eq!(a.timeline.selected(), Some((3, 4)));
    a.editor.apply(StepCmd::Undo).expect("undo");
    assert_eq!(durations(&a), [10, 20, 30, 40, 50]);
}

#[test]
fn the_frame_keys_move_the_selected_frames_and_stop_at_the_ends() {
    let mut a = timed();
    a.move_frame(true);
    a.move_frame(true);
    assert_eq!(durations(&a), [10, 40, 50, 20, 30]);
    assert_eq!(a.timeline.selected(), Some((3, 4)));
    a.move_frame(true);
    assert_eq!(durations(&a), [10, 40, 50, 20, 30], "already at the end");
    let said = a.message.as_ref().map(|(text, _)| text.clone());
    assert!(
        said.is_some_and(|t| t.contains("already at the end")),
        "{:?}",
        a.message
    );
    for _ in 0..4 {
        a.move_frame(false);
    }
    assert_eq!(durations(&a), [20, 30, 10, 40, 50]);
    assert_eq!(a.timeline.selected(), Some((0, 1)));
}

/// A tag over frames 2 to 3 of five, drawn.
fn tagged() -> App {
    let mut a = app(5);
    a.editor
        .apply(TagCmd::Add {
            name: "walk".into(),
            from: 1,
            to: 2,
        })
        .expect("tag");
    draw(&mut a);
    a
}

fn tag_range(a: &App) -> (usize, usize) {
    let tag = &a.editor.doc().tags[0];
    (tag.from, tag.to)
}

fn row_at(a: &App, x: i32) -> (i32, i32) {
    let row = a.timeline.tag_row;
    (row.x + x, row.y + row.h / 2)
}

#[test]
fn dragging_a_tag_end_gives_it_other_frames_in_one_step() {
    let mut a = tagged();
    let depth = a.editor.history().undo_depth();
    // The tag's right end is at the end of frame 3's cell, less the gap.
    let (end, past) = (row_at(&a, 3 * 34 - 3), row_at(&a, 4 * 34 + 10));
    drag(&mut a, end, past);
    assert_eq!(tag_range(&a), (1, 4));
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    assert_eq!(a.timeline.tag, None, "an edge drag is not a click");
    let (start, before) = (row_at(&a, 34 + 1), row_at(&a, 10));
    drag(&mut a, start, before);
    assert_eq!(tag_range(&a), (0, 4));
    a.editor.apply(StepCmd::Undo).expect("undo");
    assert_eq!(tag_range(&a), (1, 4));
}

#[test]
fn an_end_never_passes_the_other_end() {
    let mut a = tagged();
    let (start, far) = (row_at(&a, 34 + 1), row_at(&a, 4 * 34 + 10));
    drag(&mut a, start, far);
    assert_eq!(tag_range(&a), (2, 2));
}

#[test]
fn an_end_dragged_away_and_back_is_not_a_click() {
    let mut a = tagged();
    let (end, out) = (row_at(&a, 3 * 34 - 3), row_at(&a, 4 * 34 + 10));
    a.pointer_down(end.0, end.1, Button::Left, PLAIN);
    draw(&mut a);
    for at in [out, end] {
        a.event(Event::Move { x: at.0, y: at.1 }, Window::detached());
        draw(&mut a);
    }
    a.pointer_up(end.0, end.1, Button::Left);
    draw(&mut a);
    draw(&mut a);
    assert_eq!(tag_range(&a), (1, 2));
    assert_eq!(a.timeline.tag, None, "it does not start looping");
}

#[test]
fn a_tag_replaced_during_an_edge_drag_is_left_alone() {
    let mut a = tagged();
    let (end, past) = (row_at(&a, 3 * 34 - 3), row_at(&a, 4 * 34 + 10));
    a.pointer_down(end.0, end.1, Button::Left, PLAIN);
    draw(&mut a);
    a.event(
        Event::Move {
            x: past.0,
            y: past.1,
        },
        Window::detached(),
    );
    draw(&mut a);
    // Another tag takes the dragged one's place in the list before the button comes up.
    a.editor.apply(TagCmd::Remove { tag: 0 }).expect("remove");
    let add = TagCmd::Add {
        name: "run".into(),
        from: 3,
        to: 3,
    };
    a.editor.apply(add).expect("add");
    a.pointer_up(past.0, past.1, Button::Left);
    draw(&mut a);
    draw(&mut a);
    assert_eq!(tag_range(&a), (3, 3), "the other tag keeps its frames");
}

#[test]
fn a_selection_changed_during_a_drag_is_not_moved() {
    let mut a = timed();
    let (from, to) = (strip_at(&a, 2), strip_at(&a, 4));
    a.pointer_down(from.0, from.1, Button::Left, PLAIN);
    draw(&mut a);
    a.event(Event::Move { x: to.0, y: to.1 }, Window::detached());
    draw(&mut a);
    a.timeline.select(0, 1);
    a.pointer_up(to.0, to.1, Button::Left);
    draw(&mut a);
    draw(&mut a);
    assert_eq!(durations(&a), [10, 20, 30, 40, 50]);
}

#[test]
fn a_click_in_the_middle_of_a_tag_still_loops_it() {
    let mut a = tagged();
    let middle = row_at(&a, 2 * 34);
    click(&mut a, middle, PLAIN);
    assert_eq!(a.timeline.tag, Some(0));
    let depth = a.editor.history().undo_depth();
    let end = row_at(&a, 3 * 34 - 3);
    click(&mut a, end, PLAIN);
    assert_eq!(
        a.timeline.tag, None,
        "a click on an end that does not move loops too"
    );
    assert_eq!(a.editor.history().undo_depth(), depth);
}
