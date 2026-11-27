//! Tab reaches the sliders of the main window, and the arrow keys move the one with focus.

use crate::test_app::*;
use pikslar_platform::Key;

fn tap(a: &mut crate::app::App, key: Key, mods: pikslar_platform::Mods) {
    a.key_down(key, mods, false, pikslar_platform::Window::detached());
    draw_whole(a);
}

/// Tabs to the first slider of the main window: the controls before it come first.
fn tab_to_slider(a: &mut crate::app::App) -> u32 {
    for _ in 0..=a.ui.focus.order().len() {
        tap(a, Key::Tab, PLAIN);
        if let Some(id) = a.ui.focus.focused().filter(|&id| a.ui.focus.is_slider(id)) {
            return id;
        }
    }
    panic!("no slider is reached by Tab");
}

#[test]
fn tab_focuses_a_colour_slider_and_the_arrows_step_it() {
    let mut a = app(3);
    draw_whole(&mut a);
    a.hsva.h = 0.5;
    draw_whole(&mut a);
    let first = tab_to_slider(&mut a);
    assert_eq!(
        first,
        crate::panel_right::ID_COLOR,
        "the first colour slider"
    );
    let before = a.hsva.h;
    tap(&mut a, Key::Right, PLAIN);
    assert!(
        (a.hsva.h - (before + 1.0 / 255.0)).abs() < 1e-4,
        "{}",
        a.hsva.h
    );
    tap(&mut a, Key::Left, SHIFT);
    assert!((a.hsva.h - (before + 1.0 / 255.0 - 10.0 / 255.0)).abs() < 1e-4);
    tap(&mut a, Key::Home, PLAIN);
    assert!(a.hsva.h < 1e-6);
    tap(&mut a, Key::End, PLAIN);
    assert!(a.hsva.h > 0.99);
    assert_eq!(a.editor.frame, 0, "the arrows did not walk the frames");
}

#[test]
fn escape_gives_the_arrows_back_to_the_frames() {
    let mut a = app(3);
    draw_whole(&mut a);
    tap(&mut a, Key::Tab, PLAIN);
    tap(&mut a, Key::Escape, PLAIN);
    assert_eq!(a.ui.focus.focused(), None);
    tap(&mut a, Key::Right, PLAIN);
    assert_eq!(a.editor.frame, 1);
}

#[test]
fn the_layer_opacity_slider_steps_by_one_in_255() {
    let mut a = app(1);
    draw_whole(&mut a);
    let opacity = a.editor.doc().layers()[0].opacity;
    let ids: Vec<u32> = a.ui.focus.order().to_vec();
    let at = ids
        .iter()
        .position(|&id| id == crate::panel_right::ID_LAYER_TOOLS + 10)
        .expect("the opacity slider is reachable");
    for _ in 0..=at {
        tap(&mut a, Key::Tab, PLAIN);
    }
    tap(&mut a, Key::Left, PLAIN);
    assert_eq!(a.editor.doc().layers()[0].opacity, opacity - 1);
}

#[test]
fn folding_the_section_of_a_focused_slider_gives_the_arrows_back() {
    let mut a = app(3);
    draw_whole(&mut a);
    tab_to_slider(&mut a);
    a.fold_section(crate::panel_right::Section::Color, true);
    draw_whole(&mut a);
    assert_eq!(a.ui.focus.focused(), None, "the slider is gone");
    tap(&mut a, Key::Right, PLAIN);
    assert_eq!(a.editor.frame, 1, "Right steps the frame again");
}

#[test]
fn enter_falls_through_when_the_focused_export_button_is_gone() {
    let mut a = app(2);
    a.open_export_dialog(false);
    draw_whole(&mut a);
    // Focus "Palette" (a PNG option), then pick GIF with a digit, which has no such button.
    a.ui.focus.set(530);
    a.key_down(
        Key::Char(b'4'),
        PLAIN,
        false,
        pikslar_platform::Window::detached(),
    );
    draw_whole(&mut a);
    assert_eq!(a.ui.focus.focused(), None);
}
