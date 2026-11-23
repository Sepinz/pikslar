//! Splitters and the docks' keys, through the window's own key handler.

use super::*;
use crate::panel_right::ID_SECTION;
use crate::test_app::*;
use pikslar_platform::Window;

const SIZE: (i32, i32) = (1366, 705);

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

fn press(a: &mut App, key: Key, mods: Mods) {
    a.key_down(key, mods, false, Window::detached());
    redraw(a);
}

const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};

fn has(a: &App, id: u32) -> bool {
    a.ui.widgets().iter().any(|&(_, w)| w == id)
}

#[test]
fn the_right_dock_edge_moves_by_keys_and_keeps_its_limits() {
    let mut a = at();
    let edge = ID_SPLIT + Side::Right.index() as u32;
    assert!(has(&a, edge));
    let names = a.ui.layout().unwrap_or_default();
    assert!(
        names.contains("\tSize the right dock\tsplitter\tvalue=248\n"),
        "{names}"
    );
    a.focus_on(edge);
    redraw(&mut a);
    press(&mut a, Key::Left, PLAIN);
    assert_eq!(
        a.workspaces.active().area(Side::Right).size,
        252,
        "left widens it"
    );
    press(&mut a, Key::Right, SHIFT);
    assert_eq!(a.workspaces.active().area(Side::Right).size, 236);
    press(&mut a, Key::End, PLAIN);
    assert_eq!(a.workspaces.active().area(Side::Right).size, 400);
    assert_eq!(a.layout.right.w, 400);
    press(&mut a, Key::Home, PLAIN);
    assert_eq!(a.layout.right.w, 200);
    assert!(a.ui.focus.is(edge), "focus stays on the splitter");
}

#[test]
fn the_timeline_splitter_opens_matrix_rows_a_row_a_step() {
    let mut a = at();
    for _ in 0..3 {
        a.layer_action(0);
    }
    redraw(&mut a);
    let gutter = ID_SPLIT + Side::Bottom.index() as u32;
    a.focus_on(gutter);
    redraw(&mut a);
    press(&mut a, Key::Up, PLAIN);
    assert!(a.matrix.open);
    assert_eq!(a.layout.matrix_rows(1), 1);
    press(&mut a, Key::Up, PLAIN);
    assert_eq!(a.layout.matrix_rows(1), 2);
    press(&mut a, Key::Down, PLAIN);
    press(&mut a, Key::Down, PLAIN);
    assert!(!a.matrix.open);
    assert_eq!(a.layout.matrix_rows(1), 0);
    a.toggle_matrix();
    assert_eq!(a.layout.matrix_rows(1), 4, "the Cels button: a row a layer");
}

#[test]
fn a_slot_gutter_sizes_the_slot_above_it() {
    let mut a = at();
    let gutter = slot_split_id(Side::Right, 0);
    assert!(has(&a, gutter));
    let before = a.layout.solved.area(Side::Right).slots()[0].rect.h;
    a.focus_on(gutter);
    redraw(&mut a);
    press(&mut a, Key::Down, SHIFT);
    let after = a.layout.solved.area(Side::Right).slots()[0].rect.h;
    assert_eq!(after, before + 16, "{before} {after}");
}

#[test]
fn f6_walks_the_regions_and_shift_f6_goes_back() {
    let mut a = at();
    press(&mut a, Key::F(6), PLAIN);
    let first = a.ui.focus.focused().expect("focus");
    assert!(a.layout.top.contains(rect(&a, first).x, rect(&a, first).y));
    press(&mut a, Key::F(6), PLAIN);
    let bar = a.ui.focus.focused().expect("focus");
    assert!(a.layout.context.contains(rect(&a, bar).x, rect(&a, bar).y));
    press(&mut a, Key::F(6), PLAIN);
    let tools = a.ui.focus.focused().expect("focus");
    assert!(a.layout.side.contains(rect(&a, tools).x, rect(&a, tools).y));
    press(&mut a, Key::F(6), PLAIN);
    let dock = a.ui.focus.focused().expect("focus");
    assert!(a.layout.right.contains(rect(&a, dock).x, rect(&a, dock).y));
    press(&mut a, Key::F(6), SHIFT);
    assert_eq!(a.ui.focus.focused(), Some(tools));
}

fn rect(a: &App, id: u32) -> Rect {
    a.ui.widgets()
        .iter()
        .rev()
        .find(|&&(_, w)| w == id)
        .map(|&(r, _)| r)
        .unwrap_or_default()
}

#[test]
fn ctrl_tab_walks_the_tabs_of_the_focused_slot_and_arrows_walk_a_strip() {
    let mut a = at();
    let layers_tab = strip_id(Side::Right, 2) + 1;
    a.focus_on(layers_tab);
    redraw(&mut a);
    press(&mut a, Key::Tab, CTRL);
    assert_eq!(a.workspaces.active().area(Side::Right).slots()[2].active, 1);
    assert!(a.ui.focus.is(layers_tab + 1), "focus follows the tab");
    press(&mut a, Key::Right, PLAIN);
    assert_eq!(a.workspaces.active().area(Side::Right).slots()[2].active, 2);
    assert!(has(&a, ID_SECTION + 3), "Pixel Lint shown");
    let shift_ctrl = Mods {
        shift: true,
        ..CTRL
    };
    press(&mut a, Key::Tab, shift_ctrl);
    press(&mut a, Key::Tab, shift_ctrl);
    assert_eq!(a.workspaces.active().area(Side::Right).slots()[2].active, 0);
    assert!(has(&a, ID_SECTION + 2), "Layers again");
}

#[test]
fn enter_on_a_focused_fold_chevron_folds_its_slot() {
    let mut a = at();
    let chevron = strip_id(Side::Right, 1) + 18;
    a.focus_on(chevron);
    redraw(&mut a);
    press(&mut a, Key::Enter, PLAIN);
    assert!(!a.workspaces.active().area(Side::Right).slots()[1].open);
    assert!(!a.playing, "Enter went to the chevron, not to play");
}

#[test]
fn ctrl_1_to_4_choose_the_presets_and_keep_the_document() {
    use crate::workspace::model::Preset;
    let mut a = at();
    let pixels_before = a.editor.doc().layers().len();
    let cases = [
        (b'2', Preset::Animate, true),
        (b'3', Preset::Tiles, false),
        (b'4', Preset::Palette, false),
        (b'1', Preset::Draw, false),
    ];
    for (digit, preset, matrix) in cases {
        press(&mut a, Key::Char(digit), CTRL);
        assert_eq!(a.workspaces.current, preset);
        assert_eq!(a.matrix.open, matrix, "{preset:?}: the matrix");
        let right = a.workspaces.active().area(Side::Right).size;
        assert_eq!(a.layout.right.w, right, "{preset:?}: the right dock");
    }
    assert_eq!(a.editor.doc().layers().len(), pixels_before);
    let left = a.layout.left;
    press(&mut a, Key::Char(b'4'), CTRL);
    assert!(a.layout.left.w > left.w, "Palette places the left dock");
    assert!(
        a.layout.solved.area(Side::Left).rail,
        "Pixel Lint waits on the left dock's rail"
    );
}

#[test]
fn a_change_in_one_preset_stays_with_it() {
    let mut a = at();
    let edge = ID_SPLIT + Side::Right.index() as u32;
    a.focus_on(edge);
    redraw(&mut a);
    press(&mut a, Key::End, PLAIN);
    assert_eq!(a.layout.right.w, 400);
    press(&mut a, Key::Char(b'2'), CTRL);
    assert_eq!(a.layout.right.w, crate::workspace::presets::ANIMATE_RIGHT);
    press(&mut a, Key::Char(b'1'), CTRL);
    assert_eq!(a.layout.right.w, 400, "Draw kept its width");
}
