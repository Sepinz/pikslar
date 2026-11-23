//! The frame menu as an artist uses it: right click a frame, choose, or leave it.

use crate::app::App;
use crate::test_app::{app, click, draw, key, PLAIN, SHIFT};
use pikslar_platform::{Button, Key};

fn on_frame(a: &App, frame: usize) -> (i32, i32) {
    (
        a.timeline.strip.x + frame as i32 * 34 + 10,
        a.timeline.strip.y + 5,
    )
}

fn right_click_frame(a: &mut App, frame: usize) {
    let at = on_frame(a, frame);
    right_click(a, at);
}

fn click_frame(a: &mut App, frame: usize, mods: pikslar_platform::Mods) {
    let at = on_frame(a, frame);
    click(a, at, mods);
}

fn right_click(a: &mut App, at: (i32, i32)) {
    a.pointer_down(at.0, at.1, Button::Right, PLAIN);
    draw(a);
    a.pointer_up(at.0, at.1, Button::Right);
    draw(a);
}

/// The middle of item `n` of the open menu.
fn item(a: &App, n: i32) -> (i32, i32) {
    let open = a.menu.expect("a menu is open");
    let r = a.menu_area(open);
    let row = r.h / 5;
    (r.x + r.w / 2, r.y + n * row + row / 2)
}

#[test]
fn a_right_click_on_a_frame_opens_the_menu_there_and_goes_to_it() {
    let mut a = app(3);
    right_click_frame(&mut a, 2);
    assert_eq!(a.editor.frame, 2);
    let open = a.menu.expect("menu");
    assert_eq!(open.at, on_frame(&a, 2));
    assert!(
        a.panel_open(),
        "the canvas and keys are held while it is open"
    );
}

#[test]
fn a_right_click_away_from_the_frames_opens_nothing() {
    let mut a = app(3);
    let below = (a.timeline.strip.x + 10, a.timeline.strip.bottom() + 40);
    right_click(&mut a, below);
    let past = (a.timeline.strip.x + 3 * 34 + 10, a.timeline.strip.y + 5);
    right_click(&mut a, past);
    assert_eq!(a.menu, None);
    assert_eq!(a.editor.frame, 0);
}

#[test]
fn choosing_duplicate_copies_the_frame_as_one_undo_step_and_closes() {
    let mut a = app(2);
    right_click_frame(&mut a, 1);
    let at = item(&a, 1);
    click(&mut a, at, PLAIN);
    assert_eq!(a.menu, None);
    assert_eq!(a.editor.doc().frames.len(), 3);
    a.undo();
    assert_eq!(a.editor.doc().frames.len(), 2);
}

#[test]
fn delete_cannot_be_chosen_when_only_one_frame_is_left() {
    let mut a = app(1);
    right_click_frame(&mut a, 0);
    let at = item(&a, 2);
    click(&mut a, at, PLAIN);
    assert_eq!(a.editor.doc().frames.len(), 1);
    assert!(a.menu.is_some(), "a disabled item leaves the menu open");
}

#[test]
fn the_panel_under_the_menu_does_not_answer_and_a_click_outside_only_closes() {
    let mut a = app(2);
    right_click_frame(&mut a, 0);
    // The first frame cell is under the open menu's cover but outside the menu itself.
    let other = on_frame(&a, 1);
    let r = a.menu_area(a.menu.expect("menu"));
    assert!(!r.contains(other.0, other.1));
    click(&mut a, other, PLAIN);
    assert_eq!(a.menu, None);
    assert_eq!(
        a.editor.frame, 0,
        "the click that closed the menu did nothing else"
    );
    assert_eq!(a.editor.doc().frames.len(), 2);
}

#[test]
fn escape_or_another_right_click_closes_it() {
    let mut a = app(2);
    right_click_frame(&mut a, 1);
    key(&mut a, Key::Escape, PLAIN);
    assert_eq!(a.menu, None);
    right_click_frame(&mut a, 1);
    right_click_frame(&mut a, 0);
    assert_eq!(a.menu, None);
    assert_eq!(a.editor.frame, 1);
}

#[test]
fn a_right_click_inside_the_selection_keeps_it_for_the_command() {
    let mut a = app(4);
    click_frame(&mut a, 1, PLAIN);
    click_frame(&mut a, 2, SHIFT);
    assert_eq!(a.timeline.selected(), Some((1, 2)));
    right_click_frame(&mut a, 1);
    assert_eq!(a.timeline.selected(), Some((1, 2)));
    let at = item(&a, 2);
    click(&mut a, at, PLAIN);
    assert_eq!(
        a.editor.doc().frames.len(),
        2,
        "both selected frames were deleted"
    );
}

#[test]
fn a_right_click_outside_the_selection_drops_it() {
    let mut a = app(4);
    click_frame(&mut a, 1, PLAIN);
    click_frame(&mut a, 2, SHIFT);
    right_click_frame(&mut a, 3);
    assert_eq!(a.timeline.selected(), None);
    assert_eq!(a.editor.frame, 3);
}
