//! The slot menu, through the window's own entry points.

use crate::app::App;
use crate::dock::strip_id;
use crate::frame_menu::MenuKind;
use crate::test_app::*;
use crate::workspace::model::{Panel, Preset, Side};
use pikslar_platform::{Button, Key, Window};

const SIZE: (i32, i32) = (1366, 705);
const CTRL: pikslar_platform::Mods = pikslar_platform::Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
/// A tab strip's menu button, after its tabs.
const MENU_BUTTON: u32 = 17;

fn at() -> App {
    let mut a = app(1);
    a.ui.record_names();
    a.resized(SIZE.0, SIZE.1, 1);
    redraw(&mut a);
    a
}

fn redraw(a: &mut App) {
    draw_whole_at(a, SIZE.0, SIZE.1);
    draw_whole_at(a, SIZE.0, SIZE.1);
}

fn centre(a: &App, id: u32) -> (i32, i32) {
    let r =
        a.ui.widgets()
            .iter()
            .rev()
            .find(|&&(_, w)| w == id)
            .map(|&(r, _)| r)
            .unwrap_or_else(|| panic!("{id} is shown"));
    (r.x + r.w / 2, r.y + r.h / 2)
}

fn press_at(a: &mut App, id: u32, button: Button) {
    let (x, y) = centre(a, id);
    a.ui.mouse = (x, y);
    a.pointer_down(x, y, button, PLAIN);
    redraw(a);
    a.pointer_up(x, y, button);
    redraw(a);
}

fn rows(a: &App) -> Vec<(String, bool)> {
    let open = a.menu.expect("a menu is open");
    let MenuKind::Slot { side, slot, panel } = open.kind else {
        panic!("not the slot menu: {:?}", open.kind);
    };
    a.dock_slot_entries(side, slot, panel)
        .into_iter()
        .map(|(word, enabled)| (word.into_owned(), enabled))
        .collect()
}

fn choose(a: &mut App, word: &str) {
    let open = a.menu.expect("a menu is open");
    let n = rows(a)
        .iter()
        .position(|(w, _)| w == word)
        .unwrap_or_else(|| panic!("no row {word}: {:?}", rows(a)));
    a.close_menu();
    let MenuKind::Slot { side, slot, panel } = open.kind else {
        return;
    };
    a.slot_menu_action((side, slot), panel, n);
    redraw(a);
}

fn enabled(a: &App, word: &str) -> bool {
    rows(a).iter().any(|(w, on)| w == word && *on)
}

#[test]
fn the_menu_button_moves_the_palette_to_the_left_dock_and_back() {
    let mut a = at();
    press_at(&mut a, strip_id(Side::Right, 1) + MENU_BUTTON, Button::Left);
    assert_eq!(
        a.menu.map(|m| m.kind),
        Some(MenuKind::Slot {
            side: Side::Right,
            slot: 1,
            panel: Panel::Palette
        })
    );
    let words: Vec<String> = rows(&a).into_iter().map(|(w, _)| w).collect();
    assert_eq!(
        words,
        [
            "Move to a new slot",
            "Move to the left dock",
            "Move to the bottom dock",
            "Move beside Color",
            "Move beside Layers",
            "Move beside Timeline",
            "Fold",
            "Hide Palette",
            "Reset the Draw workspace"
        ]
    );
    assert!(!enabled(&a, "Move to a new slot"), "alone in its slot");
    choose(&mut a, "Move to the left dock");
    assert_eq!(
        a.workspaces.active().find(Panel::Palette).map(|f| f.0),
        Some(Side::Left)
    );
    assert!(a.layout.left.w > 0, "the left dock opened");
    let canvas = a.layout.canvas;
    assert!(canvas.x >= a.layout.left.right(), "the canvas moved over");
    press_at(&mut a, strip_id(Side::Left, 0) + MENU_BUTTON, Button::Left);
    choose(&mut a, "Move beside Color");
    let first = a.workspaces.active().area(Side::Right).slots()[0];
    assert_eq!(first.tabs.as_slice(), [Panel::Color, Panel::Palette]);
    assert_eq!(a.layout.left.w, 0, "the left dock is empty again");
}

#[test]
fn layers_and_the_timeline_keep_their_places() {
    let mut a = at();
    press_at(&mut a, strip_id(Side::Right, 2) + 1, Button::Right);
    assert!(
        !enabled(&a, "Move to the bottom dock"),
        "Layers needs height"
    );
    assert!(enabled(&a, "Move to the left dock"));
    assert!(!enabled(&a, "Move beside Timeline"));
    a.close_menu();
    a.open_slot_menu((Side::Bottom, 0), Panel::Timeline, (100, 600));
    assert!(!enabled(&a, "Move to the right dock"));
    assert!(!enabled(&a, "Move to the left dock"));
    assert!(!enabled(&a, "Hide Timeline"));
}

#[test]
fn shift_f10_on_a_focused_tab_opens_its_menu_and_the_keys_choose_a_row() {
    let mut a = at();
    let slices = strip_id(Side::Right, 2) + 2;
    a.focus_on(slices);
    redraw(&mut a);
    let shift_f10 = pikslar_platform::Mods {
        shift: true,
        ..PLAIN
    };
    a.key_down(Key::F(10), shift_f10, false, Window::detached());
    redraw(&mut a);
    assert_eq!(
        a.menu.map(|m| m.kind),
        Some(MenuKind::Slot {
            side: Side::Right,
            slot: 2,
            panel: Panel::Slices
        })
    );
    // Down skips Move to a new slot, which has no room in Draw's three slots.
    for k in [Key::Down, Key::Enter] {
        a.key_down(k, PLAIN, false, Window::detached());
        redraw(&mut a);
    }
    assert_eq!(a.menu, None);
    let left = a.workspaces.active().area(Side::Left).slots().to_vec();
    assert_eq!(left.len(), 1, "Slices went to the left dock");
    assert_eq!(left[0].tabs.as_slice(), [Panel::Slices]);
}

#[test]
fn hide_takes_a_panel_away_and_reset_brings_the_preset_back() {
    let mut a = at();
    a.open_slot_menu((Side::Right, 2), Panel::Lint, (900, 400));
    choose(&mut a, "Hide Pixel Lint");
    let ws = a.workspaces.active();
    assert_eq!(ws.find(Panel::Lint), None);
    assert!(ws.hidden.contains(Panel::Lint));
    a.open_slot_menu((Side::Right, 0), Panel::Color, (900, 100));
    assert!(!enabled(&a, "Hide Color"), "Color only folds");
    choose(&mut a, "Reset the Draw workspace");
    assert_eq!(
        a.workspaces.active(),
        crate::workspace::presets::default_layout(Preset::Draw)
    );
}

#[test]
fn fold_in_the_menu_folds_the_slot() {
    let mut a = at();
    a.open_slot_menu((Side::Right, 0), Panel::Color, (900, 100));
    choose(&mut a, "Fold");
    assert!(!a.workspaces.active().area(Side::Right).slots()[0].open);
    a.open_slot_menu((Side::Right, 0), Panel::Color, (900, 100));
    choose(&mut a, "Unfold");
    assert!(a.workspaces.active().area(Side::Right).slots()[0].open);
}

#[test]
fn a_moved_panel_comes_back_from_the_settings_lines() {
    use crate::workspace::persist::{lines, read};
    let mut a = at();
    a.open_slot_menu((Side::Right, 1), Panel::Palette, (900, 200));
    choose(&mut a, "Move to the left dock");
    a.open_slot_menu((Side::Right, 1), Panel::Slices, (900, 400));
    choose(&mut a, "Hide Slices");
    let written = lines(&a.workspaces);
    let pairs: Vec<(&str, &str)> = written.iter().filter_map(|l| l.split_once(" = ")).collect();
    assert_eq!(read(&pairs).active(), a.workspaces.active(), "{written:?}");
}

#[test]
fn the_timeline_alone_never_folds_out_of_reach() {
    let mut a = at();
    a.open_slot_menu((Side::Bottom, 0), Panel::Timeline, (100, 600));
    assert!(
        !enabled(&a, "Fold"),
        "a Timeline alone has no strip to unfold it"
    );
    a.close_menu();
    a.fold_slot(Side::Bottom, 0);
    assert!(a.workspaces.active().area(Side::Bottom).slots()[0].open);
}

#[test]
fn a_right_click_before_the_new_preset_is_drawn_never_opens_another_panel() {
    let mut a = at();
    let (x, y) = centre(&a, strip_id(Side::Right, 0) + 1);
    // Ctrl+3 lays out Tiles, whose first slot holds Tiles where Color's tab was drawn.
    a.key_down(Key::Char(b'3'), CTRL, false, Window::detached());
    a.ui.mouse = (x, y);
    a.pointer_down(x, y, Button::Right, PLAIN);
    let opened = a.menu.and_then(|m| match m.kind {
        MenuKind::Slot { panel, .. } => Some(panel),
        _ => None,
    });
    assert!(
        opened.is_none() || opened == Some(Panel::Color),
        "opened for {opened:?}"
    );
}
