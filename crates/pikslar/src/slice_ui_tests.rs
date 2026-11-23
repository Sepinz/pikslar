//! Slices in the app: made from the selection, drawn over the canvas, and changed by their menu.

use crate::app::{App, ToolChoice};
use crate::frame_menu::MenuKind;
use crate::test_app::*;
use pikslar_core::{Slice, SliceCmd, SliceRect, StepCmd};
use pikslar_platform::{Button, Key, Mods};
use pikslar_render::{Rect, Surface};

const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};

fn slice(name: &str, (x, y, w, h): (i32, i32, u32, u32)) -> Slice {
    Slice {
        name: name.to_string(),
        color: 0x00ff_00ff,
        bounds: SliceRect {
            x,
            y,
            width: w,
            height: h,
        },
        pivot: None,
        center: None,
    }
}

fn with_slice(a: &mut App, s: Slice) {
    a.run(SliceCmd::Add { slice: Box::new(s) }).expect("add");
    draw(a);
}

fn names(a: &App) -> Vec<String> {
    a.editor
        .doc()
        .slices
        .iter()
        .map(|s| s.name.clone())
        .collect()
}

/// What the canvas marks draw over a blank window, to compare.
fn marks(a: &App) -> Vec<u32> {
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    let mut sf = Surface {
        px: &mut px,
        w: WINDOW.0,
        h: WINDOW.1,
    };
    a.draw_slice_marks(&mut sf, Rect::new(0, 0, WINDOW.0, WINDOW.1));
    px
}

#[test]
fn a_slice_is_made_from_the_selection_with_the_first_unused_name() {
    let mut a = app(1);
    a.make_slice_from_selection();
    assert!(a.editor.doc().slices.is_empty());
    let said = a.message.as_ref().map(|(t, _)| t.clone());
    assert_eq!(said.as_deref(), Some("Select something to make a slice of"));
    select(&mut a);
    let depth = a.editor.history().undo_depth();
    a.make_slice_from_selection();
    a.make_slice_from_selection();
    assert_eq!(names(&a), ["Slice 1", "Slice 2"]);
    assert_eq!(a.editor.history().undo_depth(), depth + 2);
    let b = a.editor.doc().slices[0].bounds;
    assert_eq!((b.x, b.y, b.width, b.height), (2, 2, 4, 4));
    a.run(SliceCmd::Remove { index: 0 });
    a.make_slice_from_selection();
    assert_eq!(names(&a), ["Slice 2", "Slice 1"]);
    a.run(StepCmd::Undo);
    assert_eq!(names(&a), ["Slice 2"]);
}

#[test]
fn nothing_is_drawn_without_slices_or_when_they_are_hidden() {
    let mut a = app(1);
    assert!(marks(&a).iter().all(|&p| p == 0));
    assert_eq!(a.slices_stamp(), 0, "nothing to look at, nothing to cost");
    with_slice(&mut a, slice("Hero", (2, 2, 6, 6)));
    assert!(marks(&a).iter().any(|&p| p != 0));
    a.show_slices = false;
    assert!(marks(&a).iter().all(|&p| p == 0));
}

#[test]
fn the_outline_is_in_the_slices_color_at_its_corner() {
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (2, 2, 6, 6)));
    let (x, y) = (a.view.x + 2 * (a.view.num / a.view.den), a.view.y);
    let top = a.view.y + 2 * (a.view.num / a.view.den);
    let px = marks(&a);
    assert_eq!(px[(top * WINDOW.0 + x) as usize], 0xff00_ff00);
    assert_eq!(px[(y * WINDOW.0 + x) as usize], 0, "not outside");
}

#[test]
fn changing_slices_redraws_the_canvas_once() {
    let mut a = app(1);
    a.full_canvas = false;
    a.sync_slices();
    assert!(!a.full_canvas, "no slices, no redraw");
    with_slice(&mut a, slice("Hero", (2, 2, 6, 6)));
    a.sync_slices();
    assert!(a.full_canvas);
    a.full_canvas = false;
    a.sync_slices();
    assert!(!a.full_canvas, "unchanged since");
    a.show_slices = false;
    a.sync_slices();
    assert!(a.full_canvas);
}

#[test]
fn the_slice_menu_changes_one_slice_in_one_step_each() {
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (2, 2, 6, 6)));
    let labels: Vec<_> = a.slice_items(0, (4, 5)).iter().map(|i| i.label).collect();
    assert_eq!(
        labels,
        [
            "Rename slice",
            "Set pivot here",
            "Clear pivot",
            "Remove slice"
        ]
    );
    assert!(!a.slice_items(0, (4, 5))[2].enabled, "no pivot to clear");
    let pushes = a.editor.history().pushes();
    a.slice_menu_action(0, (4, 5), 1);
    assert_eq!(a.editor.doc().slices[0].pivot, Some((2, 3)));
    assert_eq!(a.editor.history().pushes(), pushes + 1);
    assert!(!a.slice_items(0, (4, 5))[1].enabled, "already there");
    assert!(a.slice_items(0, (4, 5))[2].enabled);
    a.slice_menu_action(0, (4, 5), 2);
    assert_eq!(a.editor.doc().slices[0].pivot, None);
    a.slice_menu_action(7, (4, 5), 1);
    a.slice_menu_action(0, (4, 5), 99);
    assert_eq!(a.editor.history().pushes(), pushes + 2, "nothing there");
    a.slice_menu_action(0, (4, 5), 3);
    assert!(a.editor.doc().slices.is_empty());
    assert_eq!(a.editor.history().pushes(), pushes + 3);
}

fn right_click(a: &mut App, at: (i32, i32), mods: Mods) {
    a.pointer_down(at.0, at.1, Button::Right, mods);
    draw(a);
    a.pointer_up(at.0, at.1, Button::Right);
}

#[test]
fn a_ctrl_right_click_in_a_slice_opens_its_menu_and_a_plain_one_still_erases() {
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (2, 2, 6, 6)));
    let inside = screen(&a, 4, 5);
    right_click(&mut a, inside, PLAIN);
    assert!(a.menu.is_none(), "a right click erases with a pencil");
    let outside = screen(&a, 20, 20);
    right_click(&mut a, outside, CTRL);
    assert!(a.menu.is_none(), "outside every slice");
    right_click(&mut a, inside, CTRL);
    let kind = a.menu.map(|m| m.kind);
    assert_eq!(
        kind,
        Some(MenuKind::Slice {
            index: 0,
            at: (4, 5)
        })
    );
    a.close_menu();
    a.show_slices = false;
    right_click(&mut a, inside, CTRL);
    assert!(a.menu.is_none(), "hidden slices are not there to click");
}

#[test]
fn a_plain_right_click_opens_the_menu_with_a_selection_tool() {
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (2, 2, 6, 6)));
    a.tool = ToolChoice::Marquee;
    let at = screen(&a, 3, 3);
    right_click(&mut a, at, PLAIN);
    assert!(matches!(
        a.menu.map(|m| m.kind),
        Some(MenuKind::Slice { index: 0, .. })
    ));
}

#[test]
fn the_smallest_slice_under_the_pointer_is_the_one_clicked() {
    let mut a = app(1);
    with_slice(&mut a, slice("Under", (0, 0, 10, 10)));
    with_slice(&mut a, slice("Over", (2, 2, 4, 4)));
    let (x, y) = screen(&a, 3, 3);
    assert_eq!(a.slice_at(x, y).map(|(i, _)| i), Some(1));
    let (x, y) = screen(&a, 8, 8);
    assert_eq!(a.slice_at(x, y).map(|(i, _)| i), Some(0));
}

#[test]
fn a_slice_inside_a_larger_later_one_can_still_be_reached() {
    let mut a = app(1);
    with_slice(&mut a, slice("Small", (2, 2, 3, 3)));
    with_slice(&mut a, slice("Mid", (1, 1, 6, 6)));
    with_slice(&mut a, slice("Big", (0, 0, 12, 12)));
    let at = |a: &App, x, y| {
        let (sx, sy) = screen(a, x, y);
        a.slice_at(sx, sy).map(|(i, _)| i)
    };
    assert_eq!(at(&a, 3, 3), Some(0));
    assert_eq!(at(&a, 1, 6), Some(1));
    assert_eq!(at(&a, 10, 10), Some(2));
    assert_eq!(at(&a, 20, 20), None);
    // Equal areas: the one drawn later is on top.
    with_slice(&mut a, slice("Twin", (2, 2, 3, 3)));
    assert_eq!(at(&a, 3, 3), Some(3));
}

fn type_text(a: &mut App, text: &str) {
    text.chars().for_each(|c| a.text_typed(c));
    draw(a);
}

#[test]
fn renaming_a_slice_keeps_the_name_as_one_step() {
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (2, 2, 6, 6)));
    let pushes = a.editor.history().pushes();
    a.slice_menu_action(0, (3, 3), 0);
    assert!(a.slice_naming.is_some());
    key(&mut a, Key::End, PLAIN);
    type_text(&mut a, "es");
    assert_eq!(names(&a), ["Hero"], "nothing changes while typing");
    let tool = a.tool;
    key(&mut a, Key::Char(b'B'), PLAIN);
    assert_eq!(a.tool, tool, "letters are not shortcuts");
    key(&mut a, Key::Enter, PLAIN);
    assert!(a.slice_naming.is_none());
    assert_eq!(names(&a), ["Heroesb"]);
    assert_eq!(a.editor.history().pushes(), pushes + 1);
    a.run(StepCmd::Undo);
    assert_eq!(names(&a), ["Hero"]);
}

#[test]
fn escape_and_an_empty_name_leave_the_name_as_it_was() {
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (2, 2, 6, 6)));
    let pushes = a.editor.history().pushes();
    a.slice_menu_action(0, (3, 3), 0);
    type_text(&mut a, "x");
    key(&mut a, Key::Escape, PLAIN);
    assert!(a.slice_naming.is_none());
    a.slice_menu_action(0, (3, 3), 0);
    a.pointer_down(1, 1, Button::Left, PLAIN);
    assert!(a.slice_naming.is_none(), "a press elsewhere ends it");
    a.slice_menu_action(0, (3, 3), 0);
    for _ in 0..8 {
        key(&mut a, Key::Backspace, PLAIN);
        key(&mut a, Key::Delete, PLAIN);
    }
    key(&mut a, Key::Enter, PLAIN);
    assert_eq!(names(&a), ["Hero"]);
    assert_eq!(a.editor.history().pushes(), pushes);
}

#[test]
fn the_view_menu_shows_and_hides_slices_without_touching_the_document() {
    let mut a = app(1);
    let last = a.view_items().len() - 5;
    assert_eq!(a.view_items()[last].label, "Hide slices");
    let revision = a.editor.revision();
    a.view_menu_action(last, pikslar_platform::Window::detached());
    assert!(!a.show_slices);
    assert_eq!(a.view_items()[last].label, "Show slices");
    assert_eq!(
        a.editor.revision(),
        revision,
        "a view setting, not a change"
    );
    a.view_menu_action(last, pikslar_platform::Window::detached());
    assert!(a.show_slices);
}

const SHADOW: u32 = 0xff00_0000;

/// Pans the view so the slice's top left corner is `by` pixels beyond the canvas's.
fn corner_off_screen(a: &mut App, by: i32) {
    let canvas = a.layout.canvas;
    a.view.x = canvas.x - by;
    a.view.y = canvas.y - by;
}

#[test]
fn a_name_is_drawn_when_the_slices_corner_is_off_screen() {
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (0, 0, 32, 32)));
    assert!(marks(&a).contains(&SHADOW), "the label shows on screen");
    corner_off_screen(&mut a, 40);
    let px = marks(&a);
    assert!(px.contains(&SHADOW), "the label stays on the canvas");
    let canvas = a.layout.canvas;
    let stray = |i: usize, p: &u32| {
        let (x, y) = (i as i32 % WINDOW.0, i as i32 / WINDOW.0);
        *p != 0 && !canvas.contains(x, y)
    };
    assert!(!px.iter().enumerate().any(|(i, p)| stray(i, p)));
}

#[test]
fn a_name_being_typed_is_drawn_when_the_corner_is_off_screen() {
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (0, 0, 32, 32)));
    a.slice_menu_action(0, (3, 3), 0);
    corner_off_screen(&mut a, 40);
    let raised = pikslar_ui::theme::raised();
    assert!(marks(&a).contains(&raised), "the box shows near the slice");
    // And one that is wholly out of view draws nothing and does not fail.
    a.view.x = a.layout.canvas.right() + 500;
    assert!(marks(&a).iter().all(|&p| p == 0));
}

#[test]
fn the_dashes_of_a_huge_centre_are_bounded_by_the_clip() {
    use crate::slice_marks::dashed_points;
    let clip = Rect::new(100, 50, 300, 200);
    let perimeter = 2 * (clip.w + clip.h) as usize;
    let huge = Rect::new(-1_000_000_000, -1_000_000_000, 2_000_000_000, 2_000_000_000);
    assert_eq!(
        dashed_points(clip, huge).count(),
        0,
        "every edge is far away"
    );
    let edge = Rect::new(clip.x + 5, -1_000_000_000, 1_000_000_000, 2_000_000_000);
    let n = dashed_points(clip, edge).count();
    assert!(n > 0 && n <= perimeter, "{n} plots");
    let inside = Rect::new(clip.x + 5, clip.y + 5, 20, 20);
    assert!(dashed_points(clip, inside).all(|(x, y)| clip.contains(x, y)));
    assert!(dashed_points(clip, inside).count() > 0);
}

#[test]
fn a_huge_centre_at_high_zoom_draws_without_walking_it() {
    let mut a = app(1);
    let mut big = slice("Hero", (0, 0, 32, 32));
    big.center = Some(SliceRect {
        x: 0,
        y: 0,
        width: 32,
        height: 32,
    });
    with_slice(&mut a, big);
    a.view.num = 64;
    a.view.den = 1;
    a.view.x = -50_000_000;
    a.view.y = -50_000_000;
    assert!(
        marks(&a).iter().all(|&p| p == 0),
        "all of it is out of view"
    );
}

#[test]
fn a_right_click_while_space_is_held_pans_and_opens_no_menu() {
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (2, 2, 6, 6)));
    let inside = screen(&a, 4, 5);
    a.space_held = true;
    a.pointer_down(inside.0, inside.1, Button::Right, CTRL);
    assert!(a.menu.is_none());
    assert!(matches!(a.gesture, crate::app::Gesture::Pan { .. }));
    a.pointer_up(inside.0, inside.1, Button::Right);
    a.tool = ToolChoice::Marquee;
    a.pointer_down(inside.0, inside.1, Button::Right, PLAIN);
    assert!(a.menu.is_none(), "a selection tool's click pans too");
    assert!(matches!(a.gesture, crate::app::Gesture::Pan { .. }));
}

#[test]
fn the_view_menu_makes_a_slice_from_the_selection_or_is_greyed_out() {
    let mut a = app(1);
    let make = a.view_items().len() - 4;
    assert_eq!(a.view_items()[make].label, "Make slice from selection");
    assert!(!a.view_items()[make].enabled, "nothing selected");
    select(&mut a);
    assert!(a.view_items()[make].enabled);
    a.view_menu_action(make, pikslar_platform::Window::detached());
    assert_eq!(names(&a), ["Slice 1"]);
}

#[test]
fn hovering_a_slice_with_a_paint_tool_says_how_to_reach_its_menu() {
    use pikslar_platform::{Event, Handler, Window};
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (2, 2, 6, 6)));
    let hint = Some("Ctrl right click for the slice menu");
    let hover = |a: &mut App, at: (i32, i32)| {
        draw(a);
        a.ui_dirty = false;
        a.event(Event::Move { x: at.0, y: at.1 }, Window::detached());
        let redraw = a.ui_dirty;
        draw(a);
        (a.status_text().map(str::to_owned), redraw)
    };
    let inside = screen(&a, 4, 5);
    assert_eq!(hover(&mut a, inside), (hint.map(str::to_owned), true));
    let outside = screen(&a, 20, 20);
    assert_eq!(hover(&mut a, outside), (None, true), "leaving redraws too");
    a.show_slices = false;
    assert_eq!(hover(&mut a, inside).0, None, "hidden slices say nothing");
    a.show_slices = true;
    a.tool = ToolChoice::Marquee;
    assert_eq!(hover(&mut a, inside).0, None, "the plain right click works");
}

#[test]
fn a_rename_pending_does_not_carry_into_a_newly_opened_document() {
    use pikslar_core::{ColorMode, Document};
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (2, 2, 6, 6)));
    a.slice_menu_action(0, (3, 3), 0);
    a.start_layer_naming(0);
    assert!(a.slice_naming.is_some() && a.layer_naming.is_some());
    let mut other = Document::new(16, 16, ColorMode::Rgba).expect("valid");
    other.add_slice(slice("Other", (1, 1, 4, 4))).expect("fits");
    a.replace_document(other, None);
    assert!(a.slice_naming.is_none());
    assert!(a.layer_naming.is_none());
    type_text(&mut a, "x");
    key(&mut a, Key::Enter, PLAIN);
    assert_eq!(names(&a), ["Other"]);
}

#[test]
fn cropping_to_the_selection_moves_slices_by_the_crop_offset() {
    let mut a = app(1);
    with_slice(&mut a, slice("Hero", (3, 4, 2, 2)));
    select(&mut a);
    a.crop_to_selection();
    assert_eq!(a.editor.doc().width, 4, "cropped to the selection");
    let b = a.editor.doc().slices[0].bounds;
    // The selection starts at (2, 2): the slice moves up and left by that.
    assert_eq!((b.x, b.y, b.width, b.height), (1, 2, 2, 2));
}
