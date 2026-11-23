//! Groups in the layer list: the group tool and its shortcut, the fold arrow, and the arrow
//! keys stepping through the rows shown.

use crate::app::App;
use crate::panel_layers::ID_LAYER_FOLD;
use crate::panel_right::{ID_LAYER_ROW, ID_LAYER_TOOLS};
use crate::test_app::*;
use pikslar_core::LayerKind;
use pikslar_platform::{Button, Key, Mods};

const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
const SHIFT: Mods = Mods {
    ctrl: false,
    shift: true,
    alt: false,
};

fn shown(a: &mut App, id: u32) -> Option<(i32, i32)> {
    draw_whole(a);
    let found = a.ui.widgets().iter().find(|&&(_, w)| w == id);
    found.map(|(r, _)| (r.x + r.w / 2, r.y + r.h / 2))
}

fn click_whole(a: &mut App, id: u32) {
    let (x, y) = shown(a, id).expect("the widget is shown");
    a.pointer_down(x, y, Button::Left, Mods::default());
    draw_whole(a);
    a.pointer_up(x, y, Button::Left);
    draw_whole(a);
}

/// Three layers, the middle one active, then grouped by the group tool.
fn grouped() -> App {
    let mut a = app(1);
    a.layer_action(0);
    a.layer_action(0);
    a.editor.layer = 1;
    click_whole(&mut a, ID_LAYER_TOOLS + 6);
    a
}

fn kinds(a: &App) -> Vec<(LayerKind, u8)> {
    let layers = a.editor.doc().layers().iter();
    layers.map(|l| (l.kind, l.depth)).collect()
}

#[test]
fn the_group_tool_groups_the_active_layer_and_ungroups_a_group() {
    let mut a = grouped();
    let group = (LayerKind::Group, 0);
    let paint = |depth| (LayerKind::Paint, depth);
    assert_eq!(kinds(&a), [paint(0), paint(1), group, paint(0)]);
    assert_eq!(a.editor.layer, 2, "the new group is active");
    click_whole(&mut a, ID_LAYER_TOOLS + 6);
    assert_eq!(kinds(&a), [paint(0), paint(0), paint(0)]);
}

#[test]
fn ctrl_g_groups_and_ungroups() {
    let mut a = app(1);
    key(&mut a, Key::Char(b'G'), CTRL);
    assert_eq!(
        a.editor.doc().layers()[a.editor.layer].kind,
        LayerKind::Group
    );
    assert_eq!(a.editor.doc().layers().len(), 2);
    key(&mut a, Key::Char(b'G'), CTRL);
    assert_eq!(a.editor.doc().layers().len(), 1);
}

#[test]
fn folding_hides_the_rows_and_hands_the_active_place_to_the_group() {
    let mut a = grouped();
    a.editor.layer = 1;
    assert!(shown(&mut a, ID_LAYER_ROW + 1).is_some());
    click_whole(&mut a, ID_LAYER_FOLD + 2);
    assert!(a.editor.doc().layers()[2].folded);
    assert!(shown(&mut a, ID_LAYER_ROW + 1).is_none(), "folded away");
    assert_eq!(a.editor.layer, 2);
    click_whole(&mut a, ID_LAYER_FOLD + 2);
    assert!(shown(&mut a, ID_LAYER_ROW + 1).is_some(), "open again");
}

#[test]
fn the_arrow_keys_step_through_the_rows_shown() {
    let mut a = grouped();
    a.editor.layer = 3;
    key(&mut a, Key::Down, Mods::default());
    assert_eq!(a.editor.layer, 2, "the group");
    key(&mut a, Key::Down, Mods::default());
    assert_eq!(a.editor.layer, 1, "into it");
    a.fold_group(2, true);
    a.editor.layer = 3;
    key(&mut a, Key::Down, Mods::default());
    key(&mut a, Key::Down, Mods::default());
    assert_eq!(a.editor.layer, 0, "past what the folded group holds");
    key(&mut a, Key::Up, Mods::default());
    assert_eq!(a.editor.layer, 2);
}

#[test]
fn shift_up_and_down_move_a_layer_into_and_out_of_a_group() {
    let mut a = grouped();
    a.editor.layer = 3;
    key(&mut a, Key::Down, SHIFT);
    assert_eq!(kinds(&a)[2], (LayerKind::Paint, 1), "in at the top");
    assert_eq!(a.editor.layer, 2);
    key(&mut a, Key::Up, SHIFT);
    assert_eq!(kinds(&a)[3], (LayerKind::Paint, 0), "out again");
    assert_eq!(a.editor.layer, 3);
}

#[test]
fn the_layer_tools_keep_their_names_for_scripts_and_the_plus_button_hints_the_right_click() {
    let mut a = app(1);
    a.ui.record_names();
    a.ui_dirty = true;
    draw_whole(&mut a);
    let layout = a.ui.layout().expect("names are kept");
    assert!(layout.contains("\tNew layer\t"), "{layout}");
    assert!(!layout.contains("Right click for a tilemap"), "{layout}");
    assert_eq!(
        a.layer_tools()[0].1,
        "New layer. Right click for a tilemap layer"
    );
}
