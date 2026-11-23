//! Scrolling the timeline's layer rows: the wheel, the bar, and the keyboard bringing the
//! focused row into view.

use crate::app::App;
use crate::panel_matrix::{ID_MATRIX, ID_MATRIX_BAR, ID_NAMES};
use crate::test_app::*;
use pikslar_platform::{Button, Event, Handler, Key, Window};
use pikslar_render::Rect;

const SIZE: (i32, i32) = (1366, 705);

fn redraw(a: &mut App) {
    draw_whole_at(a, SIZE.0, SIZE.1);
    draw_whole_at(a, SIZE.0, SIZE.1);
}

/// An open timeline with `layers` layers; the top one is current.
fn open_app(layers: usize) -> App {
    let mut a = app(3);
    a.ui.record_names();
    a.resized(SIZE.0, SIZE.1, 1);
    for _ in 1..layers {
        a.layer_action(0);
    }
    a.toggle_matrix();
    redraw(&mut a);
    a
}

fn press(a: &mut App, key: Key) {
    a.key_down(key, PLAIN, false, Window::detached());
    redraw(a);
}

fn rect_of(a: &App, id: u32) -> Option<Rect> {
    a.ui.widgets()
        .iter()
        .find(|&&(_, w)| w == id)
        .map(|&(r, _)| r)
}

fn shows(a: &App, layer: usize) -> bool {
    let wanted = format!("\tCel {layer} 1\t");
    a.ui.layout().expect("names are kept").contains(&wanted)
}

fn wheel_over(a: &mut App, id: u32, ticks: i32) {
    let r = rect_of(a, id).expect("the widget is drawn");
    let event = Event::Wheel {
        x: r.x + 4,
        y: r.y + 4,
        ticks,
    };
    a.event(event, Window::detached());
    redraw(a);
}

#[test]
fn the_wheel_over_the_layer_names_scrolls_the_rows_too() {
    let mut a = open_app(20);
    assert!(shows(&a, 20) && !shows(&a, 1));
    wheel_over(&mut a, ID_NAMES, -30);
    assert!(shows(&a, 1), "the bottom layer comes into view");
    assert_eq!(a.editor.layer, 19, "scrolling does not change the layer");
}

#[test]
fn a_key_on_the_matrix_brings_the_current_row_back_into_view() {
    let mut a = open_app(20);
    wheel_over(&mut a, ID_MATRIX, -30);
    assert!(!shows(&a, 20), "the wheel took the current row out of view");
    a.focus_on(ID_MATRIX);
    redraw(&mut a);
    press(&mut a, Key::Right);
    assert!(shows(&a, 20), "an arrow shows the focused cel's row");
}

#[test]
fn a_key_on_the_names_brings_the_current_row_back_into_view() {
    let mut a = open_app(20);
    wheel_over(&mut a, ID_NAMES, -30);
    a.focus_on(ID_NAMES);
    redraw(&mut a);
    press(&mut a, Key::Up);
    assert!(shows(&a, 20), "Up at the top still shows the current row");
    wheel_over(&mut a, ID_NAMES, -30);
    press(&mut a, Key::Down);
    assert_eq!(a.editor.layer, 18);
    assert!(shows(&a, 19), "and Down follows to the new row");
}

#[test]
fn moving_down_through_the_rows_scrolls_the_list_along() {
    let mut a = open_app(20);
    a.focus_on(ID_MATRIX);
    redraw(&mut a);
    for _ in 0..19 {
        press(&mut a, Key::Down);
    }
    assert_eq!(a.editor.layer, 0);
    assert!(shows(&a, 1) && !shows(&a, 20));
}

#[test]
fn a_bar_sits_by_the_cels_only_when_rows_are_hidden_and_a_drag_scrolls() {
    let mut few = open_app(2);
    assert!(rect_of(&few, ID_MATRIX_BAR).is_none(), "every row fits");
    redraw(&mut few);
    let mut a = open_app(20);
    let (bar, cels) = (
        rect_of(&a, ID_MATRIX_BAR).expect("a bar"),
        rect_of(&a, ID_MATRIX).expect("cels"),
    );
    assert!(
        (0..=8).contains(&(bar.x - cels.right())),
        "the bar is next to the last cel column, not at the far end of the strip"
    );
    let at = (bar.x + bar.w / 2, bar.bottom() - 2);
    a.pointer_down(at.0, at.1, Button::Left, PLAIN);
    redraw(&mut a);
    a.pointer_up(at.0, at.1, Button::Left);
    redraw(&mut a);
    assert!(
        shows(&a, 1),
        "a click at the bottom of the bar reaches the last row"
    );
}

#[test]
fn an_unselected_frame_has_a_unit_of_edge_round_it_and_the_current_one_does_not() {
    use pikslar_render::Surface;
    use pikslar_ui::theme;
    let mut a = app(3);
    a.resized(WINDOW.0, WINDOW.1, 1);
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    for _ in 0..2 {
        let mut sf = Surface {
            px: &mut px,
            w: WINDOW.0,
            h: WINDOW.1,
        };
        a.frame(&mut sf, Window::detached(), &mut Vec::new());
    }
    let strip = a.timeline.strip;
    let top = |frame: i32| px[(strip.y * WINDOW.0 + strip.x + frame * 34 + 16) as usize];
    assert_eq!(a.editor.frame, 0);
    assert_ne!(top(0), theme::edge(), "the current frame is Accent");
    assert_eq!(top(1), theme::edge(), "frame 2 is outlined");
    assert_eq!(top(2), theme::edge(), "frame 3 is outlined");
}

#[test]
fn the_timing_buttons_are_icons_not_words() {
    let mut a = app(3);
    a.ui.record_names();
    a.resized(SIZE.0, SIZE.1, 1);
    redraw(&mut a);
    let layout = a.ui.layout().expect("names");
    for name in ["Shorter", "Longer"] {
        assert!(layout.contains(&format!("\t{name}\t")), "{name}");
    }
}

fn rows_shown(a: &App) -> usize {
    let layout = a.ui.layout().expect("names are kept");
    layout.matches("\tCel ").filter(|_| true).count() / 3
}

#[test]
fn deleting_layers_while_scrolled_keeps_the_rows_full() {
    let mut a = open_app(20);
    wheel_over(&mut a, ID_MATRIX, -30);
    assert!(shows(&a, 1) && !shows(&a, 20));
    for _ in 0..14 {
        a.layer_action(1);
        redraw(&mut a);
    }
    assert_eq!(a.editor.doc().layers().len(), 6);
    assert!(
        shows(&a, 1) && shows(&a, 6),
        "all six rows are in view again"
    );
}

#[test]
fn growing_the_window_shows_more_rows_from_where_it_was_scrolled_to() {
    let mut a = app(3);
    a.ui.record_names();
    a.resized(1000, 561, 1);
    for _ in 1..20 {
        a.layer_action(0);
    }
    a.toggle_matrix();
    draw_whole_at(&mut a, 1000, 561);
    draw_whole_at(&mut a, 1000, 561);
    wheel_over(&mut a, ID_MATRIX, -30);
    assert!(shows(&a, 1));
    a.resized(1366, 900, 1);
    draw_whole_at(&mut a, 1366, 900);
    draw_whole_at(&mut a, 1366, 900);
    assert!(
        a.layout.matrix_rows(1) > 2,
        "the larger window has more rows"
    );
    assert!(shows(&a, 1), "the bottom layer is still the last row");
    assert!(
        shows(&a, a.layout.matrix_rows(1) as usize),
        "and the rows above fill it"
    );
}

#[test]
fn one_layer_more_than_the_rows_scrolls_by_one_row() {
    let rows = crate::layout::MATRIX_ROWS as usize;
    let mut a = open_app(rows + 1);
    assert!(shows(&a, rows + 1) && !shows(&a, 1));
    a.focus_on(ID_MATRIX);
    redraw(&mut a);
    for _ in 0..rows {
        press(&mut a, Key::Down);
    }
    assert_eq!(a.editor.layer, 0);
    assert!(shows(&a, 1) && !shows(&a, rows + 1));
    assert_eq!(rows_shown(&a), rows);
}
