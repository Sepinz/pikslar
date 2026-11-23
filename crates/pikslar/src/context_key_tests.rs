//! Shift F10 and the Menu key open the context menu of the active row.

use crate::frame_menu::MenuKind;
use crate::test_app::*;
use pikslar_platform::Key;

fn open_with_shift_f10(a: &mut crate::app::App) {
    draw_whole(a);
    key(a, Key::F(10), SHIFT);
}

#[test]
fn shift_f10_opens_the_frame_menu_at_the_active_frame() {
    let mut a = app(3);
    a.set_frame(1);
    open_with_shift_f10(&mut a);
    let open = a.menu.expect("a menu is open");
    assert_eq!(open.kind, MenuKind::Frame);
    assert_eq!(a.frame_at(open.at.0, open.at.1), Some(1), "at frame 2");
    assert_eq!(a.editor.frame, 1, "the active frame stays active");
}

#[test]
fn the_menu_key_does_the_same_and_the_menu_is_usable_from_the_keyboard() {
    let mut a = app(3);
    a.set_frame(2);
    draw_whole(&mut a);
    key(&mut a, Key::Menu, PLAIN);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Frame));
    key(&mut a, Key::Down, PLAIN);
    key(&mut a, Key::Enter, PLAIN);
    assert!(a.menu.is_none());
    assert_eq!(a.editor.doc().frames.len(), 4, "New frame was chosen");
}

#[test]
fn plain_f10_and_other_function_keys_open_nothing() {
    let mut a = app(2);
    draw_whole(&mut a);
    key(&mut a, Key::F(10), PLAIN);
    key(&mut a, Key::F(1), SHIFT);
    assert!(a.menu.is_none());
}

#[test]
fn after_walking_the_layers_it_is_the_layer_menu_at_the_active_row() {
    let mut a = app(1);
    a.layer_action(0);
    draw_whole(&mut a);
    key(&mut a, Key::Down, PLAIN);
    draw_whole(&mut a);
    key(&mut a, Key::F(10), SHIFT);
    let open = a.menu.expect("a menu is open");
    assert_eq!(open.kind, MenuKind::NewTilemap);
    let row = a.layer_row_for_keys();
    assert!(
        row.contains(open.at.0, open.at.1),
        "{:?} not in {row:?}",
        open.at
    );
}

#[test]
fn a_press_in_the_timeline_takes_the_context_back() {
    let mut a = app(2);
    a.layer_action(0);
    draw_whole(&mut a);
    key(&mut a, Key::Down, PLAIN);
    let strip = a.timeline.strip;
    a.pointer_down(
        strip.x + 3,
        strip.y + 3,
        pikslar_platform::Button::Left,
        PLAIN,
    );
    a.pointer_up(strip.x + 3, strip.y + 3, pikslar_platform::Button::Left);
    draw_whole(&mut a);
    key(&mut a, Key::F(10), SHIFT);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Frame));
}

const CTRL_SHIFT: pikslar_platform::Mods = pikslar_platform::Mods {
    ctrl: true,
    shift: true,
    alt: false,
};

#[test]
fn canvas_size_the_menus_and_the_keys_dialog_have_shortcuts() {
    use crate::shortcuts::{Action, Binding};
    let mut a = app(1);
    draw_whole(&mut a);
    key(&mut a, Key::Char(b'M'), CTRL_SHIFT);
    assert!(a.canvas_dialog.is_some(), "Canvas size");
    a.canvas_dialog = None;
    key(&mut a, Key::Char(b'V'), CTRL_SHIFT);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::View));
    a.close_menu();
    key(&mut a, Key::Char(b'P'), CTRL_SHIFT);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Scripts));
    a.close_menu();
    key(&mut a, Key::Char(b'H'), CTRL_SHIFT);
    assert!(a.keys_dialog.is_some(), "Keyboard shortcuts");
    a.close_keys_dialog();
    // They can be rebound like the rest.
    let b = Binding {
        letter: b'Y',
        ctrl: true,
        shift: false,
    };
    a.keymap.bind(Action::CanvasSize, Some(b));
    key(&mut a, Key::Char(b'Y'), crate::test_app::PLAIN);
    assert!(a.canvas_dialog.is_none(), "needs Ctrl");
}

#[test]
fn a_menu_opened_by_its_shortcut_sits_under_its_button_and_takes_the_keyboard() {
    let mut a = app(1);
    draw_whole(&mut a);
    key(&mut a, Key::Char(b'V'), CTRL_SHIFT);
    let open = a.menu.expect("view menu");
    assert!(open.at.1 >= a.layout.top.y, "{:?}", open.at);
    key(&mut a, Key::Down, PLAIN);
    key(&mut a, Key::Escape, PLAIN);
    assert!(a.menu.is_none());
}

fn wheel_strip(a: &mut crate::app::App, ticks: i32) {
    use pikslar_platform::Handler;
    let strip = a.timeline.strip;
    a.event(
        pikslar_platform::Event::Wheel {
            x: strip.x + 5,
            y: strip.y + 5,
            ticks,
        },
        pikslar_platform::Window::detached(),
    );
    draw_whole(a);
}

#[test]
fn an_active_frame_scrolled_out_of_the_strip_is_shown_and_never_changed() {
    for (active, ticks) in [(0usize, -100), (59, 100)] {
        let mut a = app(60);
        a.set_frame(active);
        draw_whole(&mut a);
        wheel_strip(&mut a, ticks);
        assert_ne!(a.timeline.first() == 0, active == 0, "scrolled away");
        key(&mut a, Key::F(10), SHIFT);
        let open = a.menu.expect("the frame menu is open");
        assert_eq!(open.kind, MenuKind::Frame);
        assert_eq!(a.editor.frame, active, "the active frame did not change");
        assert_eq!(a.frame_at(open.at.0, open.at.1), Some(active));
    }
}

#[test]
fn an_empty_strip_opens_nothing() {
    let mut a = app(2);
    a.timeline.strip = pikslar_render::Rect::default();
    key(&mut a, Key::F(10), SHIFT);
    assert!(a.menu.is_none());
}

#[test]
fn up_or_down_that_moved_no_layer_does_not_hand_the_menu_to_the_layers() {
    let mut a = app(2);
    draw_whole(&mut a);
    // One layer: Up has nowhere to go.
    key(&mut a, Key::Up, PLAIN);
    key(&mut a, Key::F(10), SHIFT);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Frame));
}
