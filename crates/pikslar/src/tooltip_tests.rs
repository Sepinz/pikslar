//! Tooltips and list scroll bars, through the window's own entry points.

use crate::app::App;
use crate::panel_right::ID_SCROLL;
use crate::test_app::{app, key, PLAIN, WINDOW};
use crate::tooltip::TOOLTIP_DELAY;
use pikslar_platform::{Button, Event, Handler, Key, Window};
use pikslar_render::{Rect, Surface};
use std::time::{Duration, Instant};

fn frame(a: &mut App) -> Vec<Rect> {
    frame_pixels(a).1
}

fn frame_pixels(a: &mut App) -> (Vec<u32>, Vec<Rect>) {
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    let mut surface = Surface {
        px: &mut px,
        w: WINDOW.0,
        h: WINDOW.1,
    };
    let mut dirty = Vec::new();
    a.frame(&mut surface, Window::detached(), &mut dirty);
    (px, dirty)
}

#[test]
fn the_tooltip_is_painted_over_the_window_and_sent_to_the_screen() {
    let mut a = app(2);
    let at = strip(&a);
    a.event(Event::Move { x: at.0, y: at.1 }, Window::detached());
    frame(&mut a);
    a.rest_moved(at.0 + 100, at.1, Instant::now());
    a.rest_moved(at.0, at.1, Instant::now() - 2 * TOOLTIP_DELAY);
    let (px, dirty) = frame_pixels(&mut a);
    let (rect, _) = a.tooltip_area().expect("a tooltip");
    let inside = (rect.y + 1) * WINDOW.0 + rect.x + 1;
    assert_eq!(px[inside as usize], pikslar_ui::theme::header());
    assert!(
        dirty.iter().any(|d| d.contains(rect.x, rect.y)),
        "{dirty:?}"
    );
}

fn strip(a: &App) -> (i32, i32) {
    (a.timeline.strip.x + 10, a.timeline.strip.y + 5)
}

/// The pointer comes to rest at `at`, long enough ago for its tooltip to be due.
fn rest_long(a: &mut App, at: (i32, i32)) {
    a.event(Event::Move { x: at.0, y: at.1 }, Window::detached());
    frame(a);
    a.rest_moved(at.0 + 100, at.1, Instant::now());
    a.rest_moved(
        at.0,
        at.1,
        Instant::now() - TOOLTIP_DELAY - Duration::from_millis(1),
    );
    frame(a);
}

#[test]
fn a_resting_pointer_shows_its_controls_hint_beside_it() {
    let mut a = app(2);
    let at = strip(&a);
    rest_long(&mut a, at);
    assert!(a.rest.shown);
    let (rect, lines) = a.tooltip_area().expect("a tooltip");
    assert_eq!(
        lines.join(" "),
        a.hint
            .expect("hint")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    );
    assert!(
        !rect.contains(at.0, at.1),
        "beside the pointer, not over it: {rect:?}"
    );
    assert!(rect.bottom() <= WINDOW.1 && rect.right() <= WINDOW.0);
}

#[test]
fn it_waits_until_the_pointer_has_rested() {
    let mut a = app(2);
    let at = strip(&a);
    a.event(Event::Move { x: at.0, y: at.1 }, Window::detached());
    frame(&mut a);
    assert!(!a.rest.shown);
    let wait = a.tooltip_wait().expect("a wake-up is asked for");
    assert!(
        wait <= TOOLTIP_DELAY && wait > TOOLTIP_DELAY / 2,
        "{wait:?}"
    );
}

#[test]
fn moving_away_hides_it_and_repaints_what_it_covered_but_a_small_drift_does_not() {
    let mut a = app(2);
    let at = strip(&a);
    rest_long(&mut a, at);
    a.event(
        Event::Move {
            x: at.0 + 2,
            y: at.1 + 2,
        },
        Window::detached(),
    );
    assert!(a.rest.shown);
    a.full_canvas = false;
    a.event(
        Event::Move {
            x: at.0 + 40,
            y: at.1,
        },
        Window::detached(),
    );
    assert!(!a.rest.shown);
    assert!(a.full_canvas, "the canvas under it is drawn again");
}

#[test]
fn a_press_or_key_hides_it_until_the_pointer_moves_again() {
    let mut a = app(2);
    let at = strip(&a);
    rest_long(&mut a, at);
    key(&mut a, Key::Char(b'Q'), PLAIN);
    assert!(!a.rest.shown);
    assert_eq!(a.tooltip_wait(), None);
    rest_long(&mut a, at);
    a.pointer_down(at.0, at.1, Button::Left, PLAIN);
    assert!(!a.rest.shown);
    a.pointer_up(at.0, at.1, Button::Left);
    a.tick_tooltip(Instant::now() + 2 * TOOLTIP_DELAY);
    assert!(!a.rest.shown);
}

#[test]
fn where_there_is_no_hint_there_is_no_tooltip_and_no_more_waking() {
    let mut a = app(2);
    let c = a.layout.canvas;
    rest_long(&mut a, (c.x + c.w / 2, c.y + c.h / 2));
    assert!(!a.rest.shown);
    assert_eq!(a.tooltip_wait(), None);
}

#[test]
fn no_tooltip_shows_while_a_menu_is_open() {
    let mut a = app(2);
    let at = strip(&a);
    a.pointer_down(at.0, at.1, Button::Right, PLAIN);
    a.pointer_up(at.0, at.1, Button::Right);
    frame(&mut a);
    assert!(a.menu.is_some());
    rest_long(&mut a, (at.0 + 300, at.1));
    assert!(!a.rest.shown);
}

fn bar(a: &App) -> Rect {
    a.ui.widgets()
        .iter()
        .find(|(_, id)| *id == ID_SCROLL)
        .map(|&(r, _)| r)
        .expect("the layer list has a scroll bar")
}

#[test]
fn dragging_the_layer_bar_scrolls_the_list_without_changing_the_layer() {
    let mut a = app(1);
    for _ in 0..40 {
        a.layer_action(0);
    }
    a.editor.layer = 0;
    a.ui_dirty = true;
    frame(&mut a);
    let rows = |a: &App| -> Vec<u32> {
        let mut ids: Vec<u32> =
            a.ui.widgets()
                .iter()
                .map(|&(_, id)| id)
                .filter(|id| (80_000..90_000).contains(id))
                .collect();
        ids.sort();
        ids
    };
    let before = rows(&a);
    let r = bar(&a);
    let (x, top) = (r.x + r.w / 2, r.y + 1);
    a.event(
        Event::Move {
            x,
            y: r.bottom() - 1,
        },
        Window::detached(),
    );
    a.pointer_down(x, r.bottom() - 1, Button::Left, PLAIN);
    frame(&mut a);
    a.event(Event::Move { x, y: top }, Window::detached());
    frame(&mut a);
    a.pointer_up(x, top, Button::Left);
    frame(&mut a);
    let after = rows(&a);
    assert_ne!(before, after, "other rows are shown");
    assert_eq!(a.editor.layer, 0);
    assert!(
        after.contains(&(80_000 + 40)),
        "the top layer is in view: {after:?}"
    );
}

#[test]
fn a_different_control_under_a_resting_pointer_replaces_the_tooltip() {
    let mut a = app(2);
    let at = strip(&a);
    rest_long(&mut a, at);
    assert!(a.rest.shown);
    // The pointer drifts onto the canvas without leaving its rest.
    let c = a.layout.canvas;
    a.ui.track(c.x + c.w / 2, c.y + c.h / 2);
    a.ui_dirty = true;
    let dirty = frame(&mut a);
    assert!(!a.rest.shown, "the tooltip for another control goes");
    let whole = |d: &Rect| d.contains(c.x, c.y) && d.contains(c.right() - 1, c.bottom() - 1);
    assert!(
        dirty.iter().any(whole),
        "and the canvas it covered is drawn again: {dirty:?}"
    );
    assert!(
        a.tooltip_wait().is_some(),
        "the pointer rests again from here"
    );
}

#[test]
fn the_wheel_changes_nothing_under_an_open_menu() {
    let mut a = app(2);
    let at = strip(&a);
    a.pointer_down(at.0, at.1, Button::Right, PLAIN);
    a.pointer_up(at.0, at.1, Button::Right);
    frame(&mut a);
    assert!(a.menu.is_some());
    let view = (a.view.num, a.view.den, a.view.x, a.view.y);
    let c = a.layout.canvas;
    let wheel = Event::Wheel {
        x: c.x + c.w / 2,
        y: c.y + c.h / 2,
        ticks: 3,
    };
    a.event(wheel, Window::detached());
    assert_eq!((a.view.num, a.view.den, a.view.x, a.view.y), view);
}
