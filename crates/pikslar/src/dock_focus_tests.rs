//! Keyboard focus when what has it goes: its panel hidden, folded or moved, or another preset
//! chosen. Focus never stays on a widget that is not drawn, and lands somewhere near.

use crate::app::App;
use crate::dock::strip_id;
use crate::test_app::*;
use crate::workspace::model::{Panel, Side};
use pikslar_platform::{Key, Mods, Window};
use pikslar_render::Rect;

const SIZE: (i32, i32) = (1366, 705);
const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
/// The layer tools' New layer button.
const NEW_LAYER: u32 = 240;

fn at() -> App {
    let mut a = app(1);
    a.resized(SIZE.0, SIZE.1, 1);
    redraw(&mut a);
    a
}

fn redraw(a: &mut App) {
    draw_whole_at(a, SIZE.0, SIZE.1);
    draw_whole_at(a, SIZE.0, SIZE.1);
}

fn focus(a: &mut App, id: u32) {
    a.focus_on(id);
    redraw(a);
    assert_eq!(a.ui.focus.focused(), Some(id));
}

/// Where focus is: a widget drawn this frame, and its place.
fn landed(a: &App, what: &str) -> Rect {
    let id =
        a.ui.focus
            .focused()
            .unwrap_or_else(|| panic!("{what}: focus was dropped"));
    a.ui.widgets()
        .iter()
        .rev()
        .find(|&&(_, w)| w == id)
        .map(|&(r, _)| r)
        .unwrap_or_else(|| panic!("{what}: focus on {id}, which is not drawn"))
}

#[test]
fn another_preset_moves_focus_to_a_control_of_the_same_dock() {
    let mut a = at();
    focus(&mut a, NEW_LAYER);
    a.key_down(Key::Char(b'2'), CTRL, false, Window::detached());
    redraw(&mut a);
    let r = landed(&a, "Animate has no Layers");
    assert!(a.layout.right.contains(r.x, r.y), "{r:?}");
}

#[test]
fn a_hidden_panel_leaves_focus_on_its_dock() {
    let mut a = at();
    let slices = strip_id(Side::Right, 2) + 2;
    focus(&mut a, slices);
    a.open_slot_menu((Side::Right, 2), Panel::Slices, (900, 400));
    a.close_menu();
    let n = a
        .dock_slot_entries(Side::Right, 2, Panel::Slices)
        .iter()
        .position(|(w, _)| w == "Hide Slices")
        .expect("a Hide row");
    a.slot_menu_action((Side::Right, 2), Panel::Slices, n);
    redraw(&mut a);
    let r = landed(&a, "Slices hidden");
    assert!(a.layout.right.contains(r.x, r.y), "{r:?}");
}

#[test]
fn a_folded_slot_leaves_focus_on_a_control_still_drawn() {
    let mut a = at();
    focus(&mut a, NEW_LAYER);
    a.fold_slot(Side::Right, 2);
    redraw(&mut a);
    landed(&a, "Layers folded");
}

#[test]
fn a_moved_panel_takes_focus_on_its_tab_along() {
    let mut a = at();
    let palette = strip_id(Side::Right, 1) + 1;
    focus(&mut a, palette);
    let n = a
        .dock_slot_entries(Side::Right, 1, Panel::Palette)
        .iter()
        .position(|(w, _)| w == "Move to the left dock")
        .expect("a Move row");
    a.slot_menu_action((Side::Right, 1), Panel::Palette, n);
    redraw(&mut a);
    landed(&a, "Palette moved");
    assert_eq!(a.ui.focus.focused(), Some(strip_id(Side::Left, 0) + 1));
}
