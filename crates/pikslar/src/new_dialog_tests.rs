//! The New sprite dialog used from the keyboard alone.

use crate::test_app::*;
use pikslar_core::ColorMode;
use pikslar_platform::Key;

fn tap(a: &mut crate::app::App, key: Key, mods: pikslar_platform::Mods) {
    a.key_down(key, mods, false, pikslar_platform::Window::detached());
    draw_whole(a);
}

fn open() -> crate::app::App {
    let mut a = app(1);
    a.open_new_dialog();
    draw_whole(&mut a);
    a
}

#[test]
fn tab_walks_sizes_then_modes_then_create_and_cancel() {
    let mut a = open();
    let mut seen = Vec::new();
    for _ in 0..10 {
        tap(&mut a, Key::Tab, PLAIN);
        seen.push(a.ui.focus.focused().expect("focused"));
    }
    assert_eq!(seen, [160, 161, 162, 163, 164, 165, 170, 171, 180, 181]);
    tap(&mut a, Key::Tab, PLAIN);
    assert_eq!(a.ui.focus.focused(), Some(160), "wraps");
    tap(&mut a, Key::Tab, SHIFT);
    assert_eq!(a.ui.focus.focused(), Some(181), "Shift Tab goes back");
}

#[test]
fn arrows_space_and_enter_choose_the_size_and_mode() {
    let mut a = open();
    tap(&mut a, Key::Tab, PLAIN);
    tap(&mut a, Key::Right, PLAIN);
    tap(&mut a, Key::Space, PLAIN);
    assert_eq!(a.new_dialog.map(|d| d.size), Some(32));
    tap(&mut a, Key::Down, PLAIN);
    assert_eq!(a.ui.focus.focused(), Some(171), "same column, the mode row");
    tap(&mut a, Key::Enter, PLAIN);
    assert_eq!(a.new_dialog.map(|d| d.mode), Some(ColorMode::Rgba));
    assert!(a.new_dialog.is_some(), "Enter on a choice only chooses");
    tap(&mut a, Key::Up, PLAIN);
    tap(&mut a, Key::Left, PLAIN);
    assert_eq!(a.ui.focus.focused(), Some(160));
    tap(&mut a, Key::Left, PLAIN);
    assert_eq!(a.ui.focus.focused(), Some(160), "the row ends");
}

#[test]
fn enter_on_cancel_closes_and_enter_on_create_makes_the_sprite() {
    let mut a = open();
    tap(&mut a, Key::Tab, SHIFT);
    assert_eq!(a.ui.focus.focused(), Some(181));
    tap(&mut a, Key::Enter, PLAIN);
    assert!(a.new_dialog.is_none());
    assert_eq!(a.editor.doc().width, 32, "nothing was created");

    let mut a = open();
    tap(&mut a, Key::Tab, PLAIN);
    tap(&mut a, Key::Space, PLAIN);
    tap(&mut a, Key::Tab, SHIFT);
    tap(&mut a, Key::Tab, SHIFT);
    assert_eq!(a.ui.focus.focused(), Some(180));
    tap(&mut a, Key::Enter, PLAIN);
    assert!(a.new_dialog.is_none());
    assert_eq!(a.editor.doc().width, 16, "the 16 pixel size was made");
}

#[test]
fn enter_with_nothing_focused_still_creates_and_escape_still_cancels() {
    let mut a = open();
    tap(&mut a, Key::Escape, PLAIN);
    assert!(a.new_dialog.is_none());
    let mut a = open();
    tap(&mut a, Key::Enter, PLAIN);
    assert!(a.new_dialog.is_none());
    assert_eq!(a.editor.doc().width, 64);
}

#[test]
fn the_focused_choice_is_outlined() {
    let mut a = open();
    tap(&mut a, Key::Tab, PLAIN);
    let r =
        a.ui.widgets()
            .iter()
            .find(|(_, id)| *id == 160)
            .map(|(r, _)| *r)
            .expect("16");
    let (w, h) = WINDOW;
    let mut px = vec![0u32; (w * h) as usize];
    a.draw_interface(
        &mut pikslar_render::Surface { px: &mut px, w, h },
        pikslar_platform::Window::detached(),
        &mut Vec::new(),
    );
    assert_eq!(
        px[((r.y + 5) * w + r.x - 2) as usize],
        pikslar_ui::theme::focus()
    );
}
