//! The timeline's single line, its layer-name column, the cel matrix by keyboard and the
//! History tab, through the window's own handlers.

use crate::app::App;
use crate::dock_keys::ID_SPLIT;
use crate::panel_matrix::{ID_MATRIX, ID_NAMES, ID_NAMES_SPLIT};
use crate::shortcuts::Action;
use crate::test_app::*;
use crate::workspace::model::{Preset, Side, Slot};
use pikslar_platform::{Handler, Key, Mods, Window};
use pikslar_render::Rect;

const SIZE: (i32, i32) = (1366, 705);
const ID_REWIND: u32 = 330;

fn open_app(layers: usize, frames: usize) -> App {
    let mut a = app(frames);
    a.ui.record_names();
    a.resized(SIZE.0, SIZE.1, 1);
    for _ in 1..layers {
        a.layer_action(0);
    }
    a.toggle_matrix();
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

fn rect_of(a: &App, id: u32) -> Rect {
    a.ui.widgets()
        .iter()
        .find(|&&(_, w)| w == id)
        .map(|&(r, _)| r)
        .unwrap_or_else(|| panic!("widget {id} is not drawn"))
}

fn has(a: &App, id: u32) -> bool {
    a.ui.widgets().iter().any(|&(_, w)| w == id)
}

#[test]
fn the_unfold_action_opens_and_folds_the_cel_rows() {
    let mut a = open_app(3, 3);
    assert!(a.matrix.open);
    assert!(a.timeline_form_action(Action::TimelineForm));
    redraw(&mut a);
    assert!(!a.matrix.open && !has(&a, ID_MATRIX));
    assert!(a.timeline_form_action(Action::TimelineForm));
    redraw(&mut a);
    assert!(a.matrix.open && has(&a, ID_MATRIX));
    assert!(!a.timeline_form_action(Action::Onion), "not this phase");
}

#[test]
fn the_names_splitter_moves_the_cels_by_keys_within_its_range() {
    let mut a = open_app(3, 3);
    let before = rect_of(&a, ID_MATRIX).x;
    assert_eq!(a.workspaces.timeline_names, 176);
    a.focus_on(ID_NAMES_SPLIT);
    redraw(&mut a);
    press(&mut a, Key::Right, PLAIN);
    assert_eq!(a.workspaces.timeline_names, 180);
    assert_eq!(rect_of(&a, ID_MATRIX).x, before + 4);
    press(&mut a, Key::Right, SHIFT);
    assert_eq!(a.workspaces.timeline_names, 196);
    press(&mut a, Key::Home, PLAIN);
    assert_eq!(a.workspaces.timeline_names, 120);
    press(&mut a, Key::End, PLAIN);
    assert_eq!(a.workspaces.timeline_names, 320);
    press(&mut a, Key::Enter, PLAIN);
    assert_eq!(
        a.workspaces.timeline_names, 176,
        "Enter restores the default"
    );
    let dump = a.ui.layout().unwrap_or_default();
    assert!(
        dump.contains("Size the layer names\tsplitter\tvalue=176"),
        "{dump}"
    );
}

#[test]
fn arrow_keys_move_the_current_cel_while_the_matrix_has_focus() {
    let mut a = open_app(3, 4);
    assert_eq!((a.editor.layer, a.editor.frame), (2, 0));
    a.focus_on(ID_MATRIX);
    redraw(&mut a);
    press(&mut a, Key::Right, PLAIN);
    press(&mut a, Key::Right, PLAIN);
    assert_eq!(a.editor.frame, 2);
    press(&mut a, Key::Down, PLAIN);
    assert_eq!(a.editor.layer, 1);
    press(&mut a, Key::Down, PLAIN);
    press(&mut a, Key::Down, PLAIN);
    assert_eq!(a.editor.layer, 0, "stops at the bottom layer");
    press(&mut a, Key::Up, PLAIN);
    assert_eq!(a.editor.layer, 1);
    press(&mut a, Key::End, PLAIN);
    assert_eq!(a.editor.frame, 3);
    press(&mut a, Key::Home, PLAIN);
    assert_eq!(a.editor.frame, 0);
    assert!(a.ui.focus.is(ID_MATRIX), "focus stays on the matrix");
}

#[test]
fn shift_arrows_extend_the_picked_block_from_where_it_began() {
    let mut a = open_app(3, 4);
    a.focus_on(ID_MATRIX);
    redraw(&mut a);
    press(&mut a, Key::Right, SHIFT);
    press(&mut a, Key::Right, SHIFT);
    press(&mut a, Key::Down, SHIFT);
    let block = a.matrix.selection.expect("a block");
    assert_eq!((block.layers, block.frames), ((1, 2), (0, 2)));
    press(&mut a, Key::Left, SHIFT);
    let block = a.matrix.selection.expect("a block");
    assert_eq!((block.layers, block.frames), ((1, 2), (0, 1)));
    press(&mut a, Key::Left, PLAIN);
    let block = a.matrix.selection.expect("a cel");
    assert_eq!((block.layers, block.frames), ((1, 1), (0, 0)));
}

#[test]
fn the_names_column_picks_layers_and_toggles_marks_by_pointer_and_keys() {
    let mut a = open_app(3, 2);
    let names = rect_of(&a, ID_NAMES);
    let row = |n: i32, x: i32| (names.x + x, names.y + n * 16 + 8);
    click_whole(&mut a, row(1, 40), PLAIN);
    assert_eq!(a.editor.layer, 1, "the second row from the top is layer 2");
    click_whole(&mut a, row(1, 3), PLAIN);
    assert!(!a.editor.doc().layers()[1].visible, "the eye hides it");
    click_whole(&mut a, row(1, 14), PLAIN);
    assert!(a.editor.doc().layers()[1].locked, "the lock locks it");
    a.focus_on(ID_NAMES);
    redraw(&mut a);
    press(&mut a, Key::Enter, PLAIN);
    assert!(a.editor.doc().layers()[1].visible, "Enter shows it again");
    press(&mut a, Key::Enter, SHIFT);
    assert!(!a.editor.doc().layers()[1].locked, "Shift Enter unlocks it");
    press(&mut a, Key::Up, PLAIN);
    assert_eq!(a.editor.layer, 2);
}

#[test]
fn the_unfold_chevron_says_whether_it_is_open_and_keeps_its_old_name() {
    let mut a = open_app(2, 2);
    let state_of = |a: &App| {
        let dump = a.ui.layout().unwrap_or_default();
        dump.lines()
            .find(|l| l.contains("\tCels\t"))
            .map(str::to_string)
            .expect("a control called Cels")
    };
    let open = state_of(&a);
    assert!(
        open.contains("button") && open.contains("expanded"),
        "{open}"
    );
    a.toggle_matrix();
    redraw(&mut a);
    let folded = state_of(&a);
    assert!(!folded.ends_with("\texpanded"), "{folded}");
}

#[test]
fn the_history_tab_holds_rewind_and_the_timeline_row_gives_way_to_it() {
    let mut a = app(1);
    a.ui.record_names();
    a.resized(SIZE.0, SIZE.1, 1);
    a.choose_preset(Preset::Animate);
    redraw(&mut a);
    assert!(a.history_placed());
    assert!(!has(&a, ID_REWIND), "Timeline tab: the bar is on History");
    a.edit_layout(|ws| {
        let area = *ws.area(Side::Bottom);
        let slot = area.slots()[0];
        ws.with_area(Side::Bottom, area.with_slot(0, Slot { active: 1, ..slot }))
    });
    redraw(&mut a);
    assert!(has(&a, ID_REWIND), "History tab: Rewind's bar");
    let r = rect_of(&a, ID_REWIND);
    assert!(a.layout.bottom.contains(r.x, r.y));
    a.choose_preset(Preset::Draw);
    redraw(&mut a);
    assert!(!a.history_placed());
    assert!(has(&a, ID_REWIND), "Draw has no History: the bar stays");
}

#[test]
fn the_timeline_splitter_snaps_to_whole_rows_at_every_scale() {
    for scale in [1, 2] {
        let (w, h) = (1500 * scale, 1000 * scale);
        let mut a = app(1);
        a.resized(w, h, scale);
        for _ in 0..5 {
            a.layer_action(0);
        }
        draw_whole_at(&mut a, w, h);
        let line = a.layout.bottom.h;
        a.focus_on(ID_SPLIT + Side::Bottom.index() as u32);
        draw_whole_at(&mut a, w, h);
        for rows in 1..=4 {
            a.key_down(Key::Up, PLAIN, false, Window::detached());
            draw_whole_at(&mut a, w, h);
            assert_eq!(a.layout.matrix_rows(scale), rows, "scale {scale}");
            assert_eq!(a.layout.matrix.h, rows * 16 * scale);
            assert_eq!(a.layout.bottom.h, line + (rows * 16 + 4) * scale);
        }
    }
}

#[test]
fn every_preset_keeps_the_open_timeline_whole_at_1x_and_2x() {
    use crate::layout_tests::{assert_inside_window, assert_no_overlaps, SCREENS};
    use crate::panel_bottom::ID_TRANSPORT;
    use pikslar_platform::interface_scale;
    use pikslar_ui::role::Role;
    for preset in [
        Preset::Draw,
        Preset::Animate,
        Preset::Tiles,
        Preset::Palette,
    ] {
        for &(name, dpi, w, h) in &SCREENS {
            let scale = interface_scale(dpi, w, h);
            let mut a = App::launch(None, Some(32), None);
            a.resized(w, h, scale);
            a.choose_preset(preset);
            for _ in 1..4 {
                a.layer_action(0);
            }
            if !a.matrix.open {
                a.toggle_matrix();
            }
            draw_whole_at(&mut a, w, h);
            draw_whole_at(&mut a, w, h);
            let label = format!("{preset:?} {name} ({w}x{h}, {scale}x)");
            let kept: Vec<(Rect, u32)> =
                a.ui.widgets()
                    .iter()
                    .copied()
                    .filter(|&(_, id)| {
                        let role = a.ui.kind_of(id).map(|k| k.role);
                        !matches!(role, Some(Role::TabList | Role::Group | Role::Splitter))
                    })
                    .collect();
            assert_inside_window(&label, &kept, w, h);
            assert_no_overlaps(&label, &kept);
            for i in 0..7 {
                assert!(has(&a, ID_TRANSPORT + i), "{label}: button {i}");
            }
            let rows = a.layout.matrix_rows(scale);
            assert_eq!(has(&a, ID_MATRIX), rows > 0, "{label}: {rows} rows");
            assert_eq!(a.layout.matrix.h, rows * 16 * scale, "{label}: whole rows");
            if rows > 0 {
                assert!(has(&a, ID_NAMES) && has(&a, ID_NAMES_SPLIT), "{label}");
                let (m, n) = (rect_of(&a, ID_MATRIX), rect_of(&a, ID_NAMES));
                assert!(n.right() <= m.x, "{label}: names {n:?} cels {m:?}");
            }
        }
    }
}

#[test]
fn the_matrix_and_the_names_say_the_current_frame_and_layer() {
    let mut a = open_app(3, 4);
    a.focus_on(ID_MATRIX);
    redraw(&mut a);
    press(&mut a, Key::Right, PLAIN);
    press(&mut a, Key::Down, PLAIN);
    let dump = a.ui.layout().unwrap_or_default();
    assert!(dump.contains("Cel matrix\tlist-item\tvalue=2"), "{dump}");
    assert!(
        dump.contains("Timeline layers\tlist-item\tvalue=2"),
        "{dump}"
    );
}
// ---- from the review ----
use pikslar_core::{FloatingCmd, Mask, SelectOp, SelectionCmd};

#[test]
fn probe_eye_click_anchors_floating_pixels() {
    let mut a = open_app(3, 2);
    a.run(SelectionCmd::Select {
        shape: Box::new(Mask::rect(0, 0, 4, 4)),
        op: SelectOp::Replace,
    });
    a.run(FloatingCmd::Paste {
        clip: std::sync::Arc::new(pikslar_core::Clip::from_image(1, 1, vec![0xff00_00ff])),
        x: 1,
        y: 1,
    });
    redraw(&mut a);
    assert!(a.editor.is_floating());
    let names = rect_of(&a, ID_NAMES);
    click_whole(&mut a, (names.x + 3, names.y + 16 + 8), PLAIN);
    assert!(!a.editor.doc().layers()[1].visible, "eye toggled");
    assert!(
        a.editor.is_floating(),
        "PROBE: eye click anchored the floating pixels"
    );
}

#[test]
fn probe_double_click_name_in_animate_opens_hidden_rename() {
    let mut a = app(2);
    a.ui.record_names();
    a.resized(SIZE.0, SIZE.1, 1);
    a.layer_action(0);
    a.layer_action(0);
    a.choose_preset(Preset::Animate);
    redraw(&mut a);
    if !a.matrix.open {
        a.toggle_matrix();
        redraw(&mut a);
    }
    let names = rect_of(&a, ID_NAMES);
    let at = (names.x + 60, names.y + 8);
    a.pointer_down(at.0, at.1, pikslar_platform::Button::Left, PLAIN);
    redraw(&mut a);
    a.pointer_up(at.0, at.1, pikslar_platform::Button::Left);
    redraw(&mut a);
    a.pointer_down(at.0, at.1, pikslar_platform::Button::Left, PLAIN);
    redraw(&mut a);
    a.pointer_up(at.0, at.1, pikslar_platform::Button::Left);
    redraw(&mut a);
    let layers_placed = a
        .workspaces
        .active()
        .placed()
        .contains(crate::workspace::model::Panel::Layers);
    assert!(
        !(a.layer_naming.is_some() && !layers_placed),
        "PROBE: rename opened with no Layers panel to show it (placed={layers_placed})"
    );
}

#[test]
fn probe_one_frame_one_layer_keys() {
    let mut a = open_app(1, 1);
    a.focus_on(ID_MATRIX);
    redraw(&mut a);
    for k in [
        Key::Left,
        Key::Right,
        Key::Up,
        Key::Down,
        Key::Home,
        Key::End,
        Key::Enter,
    ] {
        press(&mut a, k, PLAIN);
        press(&mut a, k, SHIFT);
    }
    assert_eq!((a.editor.layer, a.editor.frame), (0, 0));
}

#[test]
fn probe_eye_boundaries_and_below_rows() {
    let mut a = open_app(2, 2);
    let names = rect_of(&a, ID_NAMES);
    // x = 10 still eye at 1x, 11 lock
    click_whole(&mut a, (names.x + 10, names.y + 8), PLAIN);
    assert!(!a.editor.doc().layers()[1].visible, "x=10 eye");
    click_whole(&mut a, (names.x + 11, names.y + 8), PLAIN);
    assert!(a.editor.doc().layers()[1].locked, "x=11 lock");
}

#[test]
fn probe_history_in_folded_right_dock_leaves_no_rewind_bar() {
    let mut a = app(1);
    a.ui.record_names();
    a.resized(SIZE.0, SIZE.1, 1);
    a.choose_preset(Preset::Draw);
    a.edit_layout(|ws| {
        let right = *ws.area(Side::Right);
        let slot = right.slots()[0];
        let slot = Slot {
            tabs: slot.tabs.with(crate::workspace::model::Panel::History),
            ..slot
        };
        ws.with_area(Side::Right, right.with_slot(0, slot).with_open(false))
    });
    redraw(&mut a);
    assert!(!a.history_placed(), "a folded dock cannot show it");
    assert!(
        has(&a, ID_REWIND),
        "PROBE: History placed in a folded dock: no Rewind bar anywhere"
    );
}

fn folded_group_app() -> App {
    let mut a = open_app(3, 3);
    a.editor.layer = 1;
    a.layer_action(6);
    // Layers: 0 paint, 1 paint inside the group, 2 group, 3 paint.
    a.fold_group(2, true);
    redraw(&mut a);
    a
}

#[test]
fn up_and_down_skip_the_layers_a_folded_group_holds() {
    let mut a = folded_group_app();
    a.editor.layer = 3;
    a.focus_on(ID_MATRIX);
    redraw(&mut a);
    press(&mut a, Key::Down, PLAIN);
    assert_eq!(a.editor.layer, 2, "the group");
    press(&mut a, Key::Down, PLAIN);
    assert_eq!(a.editor.layer, 0, "past what the group holds");
    press(&mut a, Key::Up, PLAIN);
    assert_eq!(a.editor.layer, 2);
    a.focus_on(ID_NAMES);
    redraw(&mut a);
    press(&mut a, Key::Down, PLAIN);
    assert_eq!(a.editor.layer, 0, "the names column skips them too");
    press(&mut a, Key::Up, PLAIN);
    assert_eq!(a.editor.layer, 2);
}

#[test]
fn a_block_is_not_picked_when_the_move_cannot_settle_floating_pixels() {
    let mut a = open_app(3, 4);
    a.run(SelectionCmd::Select {
        shape: Box::new(Mask::rect(0, 0, 4, 4)),
        op: SelectOp::Replace,
    });
    a.run(FloatingCmd::Paste {
        clip: std::sync::Arc::new(pikslar_core::Clip::from_image(1, 1, vec![0xff00_00ff])),
        x: 1,
        y: 1,
    });
    a.editor.set_memory_check(|_| false);
    a.focus_on(ID_MATRIX);
    redraw(&mut a);
    let before = (a.editor.layer, a.editor.frame, a.matrix.selection);
    press(&mut a, Key::Right, SHIFT);
    press(&mut a, Key::Down, PLAIN);
    assert_eq!((a.editor.layer, a.editor.frame), (before.0, before.1));
    assert_eq!(
        a.matrix.selection.map(|b| (b.layers, b.frames)),
        before.2.map(|b| (b.layers, b.frames))
    );
}

#[test]
fn arrow_keys_during_a_cel_drag_leave_the_drag_alone() {
    let mut a = open_app(3, 4);
    let cels = rect_of(&a, ID_MATRIX);
    let at = (cels.x + 5, cels.y + 4);
    a.pointer_down(at.0, at.1, pikslar_platform::Button::Left, PLAIN);
    redraw(&mut a);
    let to = (cels.x + 34 * 2 + 5, cels.y + 4);
    a.event(
        pikslar_platform::Event::Move { x: to.0, y: to.1 },
        Window::detached(),
    );
    redraw(&mut a);
    let press_then = a.matrix.press;
    assert!(press_then.is_some());
    a.focus_on(ID_MATRIX);
    for k in [Key::Right, Key::Down, Key::Enter, Key::End] {
        a.key_down(k, SHIFT, false, Window::detached());
        redraw(&mut a);
    }
    assert_eq!(a.matrix.press, press_then, "the drag is as it was");
    a.pointer_up(to.0, to.1, pikslar_platform::Button::Left);
    redraw(&mut a);
    assert!(a.matrix.press.is_none());
}

#[test]
fn the_names_splitter_shows_the_drawn_width_and_a_press_keeps_the_preference() {
    let mut a = app(2);
    a.ui.record_names();
    a.workspaces = a.workspaces.with_names(320);
    a.resized(640, 760, 1);
    for _ in 1..3 {
        a.layer_action(0);
    }
    a.toggle_matrix();
    draw_whole_at(&mut a, 640, 760);
    draw_whole_at(&mut a, 640, 760);
    let drawn = crate::timeline_geom::names_width(640, 320);
    assert!(
        drawn < 320,
        "a narrow window draws a narrower column: {drawn}"
    );
    let dump = a.ui.layout().unwrap_or_default();
    assert!(
        dump.contains(&format!("Size the layer names\tsplitter\tvalue={drawn}")),
        "{dump}"
    );
    let gutter = rect_of(&a, ID_NAMES_SPLIT);
    let at = (gutter.x + 2, gutter.y + 4);
    a.pointer_down(at.0, at.1, pikslar_platform::Button::Left, PLAIN);
    draw_whole_at(&mut a, 640, 760);
    a.pointer_up(at.0, at.1, pikslar_platform::Button::Left);
    draw_whole_at(&mut a, 640, 760);
    assert_eq!(a.workspaces.timeline_names, 320, "only a drag rewrites it");
}

/// The status bar is a band under the timeline; where Rewind sits in the History tab it still
/// says something (the frame and the edit count) when there is no message.
#[test]
fn the_status_bar_is_drawn_in_animate_at_both_scales() {
    for (w, h, scale) in [(1366, 705, 1), (1920, 1003, 2), (1920, 1080, 2)] {
        let mut a = app(3);
        a.resized(w, h, scale);
        a.choose_preset(Preset::Animate);
        let mut px = vec![0u32; (w * h) as usize];
        for _ in 0..2 {
            let mut surface = pikslar_render::Surface { px: &mut px, w, h };
            a.draw_interface(&mut surface, Window::detached(), &mut Vec::new());
        }
        let band = crate::timeline_geom::Geom::of(a.layout.bottom, scale, 0).status;
        let header = pikslar_ui::theme::header();
        let inked = (band.y..band.bottom())
            .flat_map(|y| (band.x..band.right()).map(move |x| (x, y)))
            .filter(|&(x, y)| px[(y * w + x) as usize] != header)
            .count();
        assert!(inked > 20, "{w}x{h} at {scale}: the status bar is blank");
    }
}
