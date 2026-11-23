//! Menus used from the keyboard, through the app.

use crate::test_app::*;
use pikslar_core::BlendMode;
use pikslar_platform::{Handler, Key, Mods};

fn press(a: &mut crate::app::App, key: Key) {
    key_plain(a, key);
}

fn key_plain(a: &mut crate::app::App, k: Key) {
    key(a, k, Mods::default());
}

#[test]
fn down_down_enter_chooses_the_second_enabled_row() {
    let mut a = app(1);
    a.open_blend_menu((100, 100));
    draw(&mut a);
    let pushes = a.editor.history().pushes();
    press(&mut a, Key::Down);
    press(&mut a, Key::Down);
    assert!(a.menu.is_some(), "still open until Enter");
    press(&mut a, Key::Enter);
    assert!(a.menu.is_none(), "choosing closes it");
    let layer = &a.editor.doc().layers()[a.editor.layer];
    assert_eq!(layer.blend, BlendMode::Screen);
    assert_eq!(a.editor.history().pushes(), pushes + 1);
}

#[test]
fn end_goes_to_the_last_row() {
    let mut a = app(1);
    a.open_blend_menu((100, 100));
    draw(&mut a);
    press(&mut a, Key::End);
    press(&mut a, Key::Enter);
    let layer = &a.editor.doc().layers()[a.editor.layer];
    assert_eq!(layer.blend, BlendMode::Difference, "the last of nine rows");
}

#[test]
fn escape_still_closes_and_enter_with_no_highlight_does_nothing() {
    let mut a = app(1);
    a.open_blend_menu((100, 100));
    draw(&mut a);
    press(&mut a, Key::Enter);
    assert!(a.menu.is_some(), "nothing was highlighted");
    press(&mut a, Key::Escape);
    assert!(a.menu.is_none());
}

#[test]
fn a_letter_jumps_to_a_row_and_up_wraps_to_the_end() {
    let mut a = app(1);
    a.open_blend_menu((100, 100));
    draw(&mut a);
    press(&mut a, Key::Char(b'S'));
    press(&mut a, Key::Enter);
    let layer = &a.editor.doc().layers()[a.editor.layer];
    assert_eq!(layer.blend, BlendMode::Screen);
    a.open_blend_menu((100, 100));
    draw(&mut a);
    press(&mut a, Key::Up);
    press(&mut a, Key::Enter);
    let layer = &a.editor.doc().layers()[a.editor.layer];
    assert_eq!(
        layer.blend,
        BlendMode::Difference,
        "Up from nothing is the last row"
    );
}

fn short_window_with_blend_menu() -> crate::app::App {
    let mut a = app(1);
    a.resized(640, 130, 1);
    draw_whole_at(&mut a, 640, 130);
    a.open_blend_menu((100, 10));
    draw_whole_at(&mut a, 640, 130);
    a
}

#[test]
fn a_menu_taller_than_the_window_pages_and_its_last_row_is_chosen_by_key() {
    let mut a = short_window_with_blend_menu();
    let rect = a.menu_area(a.menu.expect("open"));
    assert!(rect.bottom() <= 130, "{rect:?}");
    a.key_down(
        Key::End,
        Mods::default(),
        false,
        pikslar_platform::Window::detached(),
    );
    draw_whole_at(&mut a, 640, 130);
    assert!(a.ui.menu_first > 0, "the list moved to show the last row");
    a.key_down(
        Key::Enter,
        Mods::default(),
        false,
        pikslar_platform::Window::detached(),
    );
    let layer = &a.editor.doc().layers()[a.editor.layer];
    assert_eq!(layer.blend, BlendMode::Difference);
}

#[test]
fn the_wheel_scrolls_an_open_menu_and_a_new_menu_starts_at_the_top() {
    let mut a = short_window_with_blend_menu();
    a.event(
        pikslar_platform::Event::Wheel {
            x: 110,
            y: 20,
            ticks: -2,
        },
        pikslar_platform::Window::detached(),
    );
    draw_whole_at(&mut a, 640, 130);
    assert!(a.ui.menu_first > 0);
    a.close_menu();
    a.open_blend_menu((100, 10));
    draw_whole_at(&mut a, 640, 130);
    assert_eq!(a.ui.menu_first, 0);
}

#[test]
fn the_wheel_after_a_keyboard_highlight_scrolls_and_does_not_snap_back() {
    let mut a = short_window_with_blend_menu();
    a.key_down(
        Key::End,
        Mods::default(),
        false,
        pikslar_platform::Window::detached(),
    );
    draw_whole_at(&mut a, 640, 130);
    let at_end = a.ui.menu_first;
    assert!(at_end > 0);
    a.event(
        pikslar_platform::Event::Wheel {
            x: 110,
            y: 20,
            ticks: 5,
        },
        pikslar_platform::Window::detached(),
    );
    draw_whole_at(&mut a, 640, 130);
    assert!(a.ui.menu_first < at_end, "{} vs {at_end}", a.ui.menu_first);
}
