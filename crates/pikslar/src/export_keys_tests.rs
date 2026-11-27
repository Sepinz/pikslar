//! The Export panel used from the keyboard: kinds, steppers and layer choices.

use crate::test_app::*;
use pikslar_export::Kind;
use pikslar_platform::{Key, Mods, Window};

const ID: u32 = 500;

fn tap(a: &mut crate::app::App, key: Key, mods: Mods) {
    a.key_down(key, mods, false, Window::detached());
    draw_whole(a);
}

fn open(sheet: bool) -> crate::app::App {
    let mut a = app(3);
    a.open_export_dialog(sheet);
    draw_whole(&mut a);
    a
}

fn focused(a: &crate::app::App) -> Option<u32> {
    a.ui.focus.focused()
}

#[test]
fn tab_walks_the_kinds_then_each_row_then_the_buttons() {
    let mut a = open(false);
    let mut seen = Vec::new();
    for _ in 0..14 {
        tap(&mut a, Key::Tab, PLAIN);
        seen.push(focused(&a).expect("focused"));
    }
    let expected = [0, 1, 2, 3, 4, 10, 11, 20, 21, 22, 23, 36, 30, 50].map(|n| ID + n);
    assert_eq!(seen, expected);
    tap(&mut a, Key::Tab, PLAIN);
    assert_eq!(focused(&a), Some(ID + 51), "Cancel");
    tap(&mut a, Key::Tab, PLAIN);
    assert_eq!(focused(&a), Some(ID), "wraps");
}

#[test]
fn arrows_and_space_choose_a_kind() {
    let mut a = open(false);
    tap(&mut a, Key::Tab, PLAIN);
    tap(&mut a, Key::Right, PLAIN);
    assert_eq!(focused(&a), Some(ID + 1));
    tap(&mut a, Key::Space, PLAIN);
    assert_eq!(a.export_dialog.map(|c| c.kind), Some(Kind::Sheet));
    tap(&mut a, Key::Left, PLAIN);
    tap(&mut a, Key::Enter, PLAIN);
    assert_eq!(a.export_dialog.map(|c| c.kind), Some(Kind::Png));
    assert!(a.export_dialog.is_some(), "Enter on a choice only chooses");
}

#[test]
fn a_stepper_and_the_layer_choices_work_by_key() {
    let mut a = open(false);
    a.ui.focus.set(ID + 23);
    tap(&mut a, Key::Enter, PLAIN);
    assert_eq!(a.export_dialog.map(|c| c.scale), Some(2), "Scale +");
    tap(&mut a, Key::Space, PLAIN);
    assert_eq!(a.export_dialog.map(|c| c.scale), Some(3));
    tap(&mut a, Key::Left, PLAIN);
    assert_eq!(focused(&a), Some(ID + 22));
    tap(&mut a, Key::Space, PLAIN);
    assert_eq!(a.export_dialog.map(|c| c.scale), Some(2), "Scale -");
    tap(&mut a, Key::Left, PLAIN);
    assert_eq!(focused(&a), Some(ID + 21), "This layer");
    tap(&mut a, Key::Space, PLAIN);
    assert_eq!(a.export_dialog.map(|c| c.this_layer), Some(true));
    tap(&mut a, Key::Left, PLAIN);
    tap(&mut a, Key::Space, PLAIN);
    assert_eq!(a.export_dialog.map(|c| c.this_layer), Some(false));
}

#[test]
fn up_and_down_move_between_rows() {
    let mut a = open(false);
    a.ui.focus.set(ID);
    tap(&mut a, Key::Down, PLAIN);
    assert_eq!(focused(&a), Some(ID + 10), "This frame");
    tap(&mut a, Key::Down, PLAIN);
    assert_eq!(focused(&a), Some(ID + 20), "Visible layers");
    tap(&mut a, Key::Down, PLAIN);
    assert_eq!(focused(&a), Some(ID + 36), "RGBA");
    tap(&mut a, Key::Up, PLAIN);
    tap(&mut a, Key::Up, PLAIN);
    tap(&mut a, Key::Up, PLAIN);
    let id = focused(&a).expect("focused");
    assert!((ID..ID + 5).contains(&id), "back in the kinds row: {id}");
}

#[test]
fn enter_on_cancel_closes_and_escape_and_the_digits_still_work() {
    let mut a = open(false);
    a.ui.focus.set(ID + 51);
    tap(&mut a, Key::Enter, PLAIN);
    assert!(a.export_dialog.is_none());
    let mut a = open(false);
    tap(&mut a, Key::Char(b'4'), PLAIN);
    assert_eq!(a.export_dialog.map(|c| c.kind), Some(Kind::Gif));
    tap(&mut a, Key::Escape, PLAIN);
    assert!(a.export_dialog.is_none());
}

#[test]
fn the_focused_stepper_button_is_outlined() {
    let mut a = open(false);
    a.ui.focus.set(ID + 23);
    let (w, h) = WINDOW;
    let mut px = vec![0u32; (w * h) as usize];
    a.draw_interface(
        &mut pikslar_render::Surface { px: &mut px, w, h },
        Window::detached(),
        &mut Vec::new(),
    );
    let r =
        a.ui.widgets()
            .iter()
            .find(|(_, id)| *id == ID + 23)
            .map(|(r, _)| *r)
            .expect("plus");
    assert_eq!(
        px[((r.y + 5) * w + r.right() + 1) as usize],
        pikslar_ui::theme::focus()
    );
}
