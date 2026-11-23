//! Custom brushes in the app: capture, turning, the outline, the menu and the library file.

use crate::app::{App, ToolChoice};
use crate::brush_store::FILE_NAME;
use crate::frame_menu::MenuKind;
use crate::panel_side::ID_BRUSH;
use crate::settings::SettingsFile;
use crate::test_app::*;
use pikslar_core::custom_brush::{BrushChoice, BrushPaint, Handle, Preset};
use pikslar_core::{rgba, Edge, Mask, SelectionCmd, Tool};
use pikslar_platform::Key;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

const CTRL: pikslar_platform::Mods = pikslar_platform::Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
const RED: u32 = rgba(255, 0, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-brush-ui-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

fn said(a: &App) -> String {
    a.message
        .as_ref()
        .map(|(t, _)| t.clone())
        .unwrap_or_default()
}

/// An L of three pixels at (3, 3), selected; the fourth cell of the square is clear.
fn with_selected_ell() -> App {
    let mut a = app(1);
    for (x, y, color) in [(3, 3, RED), (4, 3, BLUE), (3, 4, RED)] {
        a.editor.color = color;
        dot(&mut a, x, y);
    }
    a.editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(3, 3, 2, 2)),
            op: pikslar_core::SelectOp::Replace,
        })
        .expect("select");
    a
}

fn capture(a: &mut App) {
    key(a, Key::Char(b'B'), CTRL);
}

fn deselect(a: &mut App) {
    key(a, Key::Char(b'D'), CTRL);
}

fn painted(a: &App) -> BTreeSet<(i32, i32)> {
    let doc = a.editor.doc();
    let cel = doc.layers()[0].cel(0);
    (0..32)
        .flat_map(|y| (0..32).map(move |x| (x, y)))
        .filter(|&(x, y)| cel.get(x, y) != 0)
        .collect()
}

fn sorted(mut edges: Vec<Edge>) -> Vec<(i32, i32, i32, bool)> {
    edges.sort_by_key(|e| (e.y, e.x, e.horizontal, e.len));
    edges
        .iter()
        .map(|e| (e.x, e.y, e.len, e.horizontal))
        .collect()
}

fn hover(a: &mut App, x: i32, y: i32) -> (i32, i32) {
    let at = screen(a, x, y);
    a.ui.track(at.0, at.1);
    at
}

#[test]
fn ctrl_b_makes_a_brush_of_the_selection_and_picks_the_pencil() {
    let mut a = with_selected_ell();
    a.set_tool(ToolChoice::Marquee);
    let revision = a.editor.revision();
    capture(&mut a);
    let brush = a.editor.brushes().active_custom().expect("a brush").clone();
    assert_eq!(brush.source_size(), (2, 2));
    assert_eq!(a.tool, ToolChoice::Paint(Tool::Pencil));
    assert!(said(&a).starts_with("Brush captured"), "{}", said(&a));
    assert_eq!(a.editor.revision(), revision, "not an undo step");
}

#[test]
fn ctrl_b_with_nothing_to_capture_says_so_and_changes_nothing() {
    let mut a = app(1);
    capture(&mut a);
    assert!(a.editor.brushes().active_custom().is_none());
    assert_eq!(said(&a), "Error: Select some pixels to make a brush");
}

#[test]
fn the_flip_and_rotate_keys_turn_a_custom_brush_but_still_flip_a_selection_otherwise() {
    let mut a = with_selected_ell();
    // Round brush: the key acts on the selection, as before.
    let before = a.editor.revision();
    key(&mut a, Key::Char(b'H'), SHIFT);
    assert!(a.editor.is_floating() || a.editor.revision() != before);
    a.run(pikslar_core::StepCmd::Undo);
    capture(&mut a);
    let before = a.editor.revision();
    let source = a.editor.brushes().active_custom().expect("brush").clone();
    for _ in 0..4 {
        key(&mut a, Key::Char(b'R'), PLAIN);
    }
    let back = a.editor.brushes().active_custom().expect("brush");
    assert_eq!(back.orientation(), source.orientation(), "four turns");
    key(&mut a, Key::Char(b'H'), SHIFT);
    let flipped = a
        .editor
        .brushes()
        .active_custom()
        .expect("brush")
        .orientation();
    assert_ne!(flipped, source.orientation());
    assert!(!a.editor.is_floating());
    assert_eq!(a.editor.revision(), before, "the selection was left alone");
    // With a selection tool in hand, the same key flips the selection again.
    a.set_tool(ToolChoice::Marquee);
    key(&mut a, Key::Char(b'H'), SHIFT);
    assert_eq!(
        a.editor
            .brushes()
            .active_custom()
            .expect("brush")
            .orientation(),
        flipped
    );
}

/// What a click paints with the brush, and what the outline showed before it.
fn outline_and_paint(a: &mut App, cell: (i32, i32)) -> (Vec<Edge>, BTreeSet<(i32, i32)>) {
    let at = hover(a, cell.0, cell.1);
    let shown = a.footprint_edges();
    let before = painted(a);
    click(a, at, PLAIN);
    let after = painted(a);
    (shown, after.difference(&before).copied().collect())
}

#[test]
fn the_outline_is_exactly_what_one_stamp_paints_for_every_turn_and_handle() {
    let mut a = with_selected_ell();
    capture(&mut a);
    deselect(&mut a);
    let handles = [Handle::Centre, Handle::Corner(0), Handle::Corner(3)];
    let mut at = 0;
    for turn in 0..8 {
        match turn {
            0..=3 => {}
            4 => {
                key(&mut a, Key::Char(b'H'), SHIFT);
            }
            _ => {
                key(&mut a, Key::Char(b'R'), PLAIN);
            }
        }
        a.editor.set_brush_handle(handles[turn % 3]);
        at += 6;
        let cell = (at % 24 + 4, 14 + turn as i32);
        let (shown, cells) = outline_and_paint(&mut a, cell);
        assert!(!cells.is_empty(), "turn {turn}");
        let mut mask = Mask::blank(0, 0, 32, 32);
        for &(x, y) in &cells {
            mask.set(x, y);
        }
        assert_eq!(sorted(shown), sorted(mask.outline()), "turn {turn}");
    }
}

#[test]
fn presets_and_the_round_brush_show_the_cells_they_paint() {
    let mut a = app(1);
    a.editor.brush_size = 5;
    for (i, choice) in [
        BrushChoice::Round,
        BrushChoice::Preset(Preset::Diamond),
        BrushChoice::Preset(Preset::HorizontalLine),
    ]
    .into_iter()
    .enumerate()
    {
        a.editor.set_brush(choice);
        let (shown, cells) = outline_and_paint(&mut a, (8 + i as i32 * 8, 10));
        let mut mask = Mask::blank(0, 0, 32, 32);
        for &(x, y) in &cells {
            mask.set(x, y);
        }
        assert_eq!(sorted(shown), sorted(mask.outline()), "preset {i}");
    }
}

#[test]
fn turning_the_brush_moves_the_outline() {
    let mut a = with_selected_ell();
    capture(&mut a);
    deselect(&mut a);
    hover(&mut a, 12, 12);
    let before = sorted(a.footprint_edges());
    key(&mut a, Key::Char(b'H'), SHIFT);
    assert_ne!(sorted(a.footprint_edges()), before);
}

#[test]
fn the_brush_menu_chooses_shapes_and_marks_the_one_in_use() {
    let mut a = app(1);
    a.open_brush_menu((300, 40));
    let rows = a.brush_entries(MenuKind::Brush);
    assert_eq!(rows[0].0, "Round brush, in use");
    assert!(!rows[0].1 && rows[1].1);
    assert!(!rows[5].1, "nothing to capture");
    assert!(
        !rows[6].1 && !rows[7].1 && !rows[8].1,
        "no custom brush yet"
    );
    a.close_menu();
    a.brush_menu_action(MenuKind::Brush, 2, (300, 40));
    assert_eq!(
        a.editor.brushes().active(),
        &BrushChoice::Preset(Preset::Diamond)
    );
    assert_eq!(a.brush_entries(MenuKind::Brush)[2].0, "Diamond, in use");
}

#[test]
fn paint_mode_and_handle_follow_the_menu() {
    let mut a = with_selected_ell();
    capture(&mut a);
    let rows = a.brush_entries(MenuKind::Brush);
    assert_eq!(rows[6].0, "Paint its own colors, in use");
    a.brush_menu_action(MenuKind::Brush, 7, (0, 0));
    let brush = a.editor.brushes().active_custom().expect("brush");
    assert_eq!(brush.paint(), BrushPaint::Shape);
    a.brush_menu_action(MenuKind::BrushHandle, 4, (0, 0));
    let brush = a.editor.brushes().active_custom().expect("brush");
    assert_eq!(brush.handle(), Handle::Corner(3));
    assert_eq!(
        a.brush_entries(MenuKind::BrushHandle)[4].0,
        "Handle at the bottom right, in use"
    );
}

#[test]
fn a_recent_brush_comes_back_from_the_menu() {
    let mut a = with_selected_ell();
    capture(&mut a);
    a.brush_menu_action(MenuKind::Brush, 0, (0, 0));
    assert!(a.editor.brushes().active_custom().is_none());
    assert_eq!(a.brush_entries(MenuKind::BrushRecent).len(), 1);
    a.brush_menu_action(MenuKind::BrushRecent, 0, (0, 0));
    assert!(a.editor.brushes().active_custom().is_some());
}

#[test]
fn the_tool_strip_has_a_brush_button_that_opens_the_menu() {
    let mut a = app(1);
    a.ui.record_names();
    a.ui_dirty = true;
    draw_whole(&mut a);
    assert!(a.ui.layout().expect("names").contains("\tBrush\t"));
    let rect =
        a.ui.widgets()
            .iter()
            .find(|&&(_, id)| id == ID_BRUSH + 4)
            .map(|&(r, _)| r)
            .expect("the button is on the strip");
    let at = (rect.x + rect.w / 2, rect.y + rect.h / 2);
    a.pointer_down(at.0, at.1, pikslar_platform::Button::Left, PLAIN);
    draw_whole(&mut a);
    a.pointer_up(at.0, at.1, pikslar_platform::Button::Left);
    draw_whole(&mut a);
    draw_whole(&mut a);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Brush));
}

#[test]
fn a_new_document_keeps_the_brush() {
    let mut a = with_selected_ell();
    capture(&mut a);
    let brush = a.editor.brushes().active_custom().expect("brush").clone();
    let doc = pikslar_core::Document::new(16, 16, pikslar_core::ColorMode::Rgba).expect("valid");
    a.replace_document(doc, None);
    assert_eq!(a.editor.brushes().active_custom(), Some(&brush));
}

fn with_library(dir: &std::path::Path) -> App {
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(dir));
    a
}

#[test]
fn a_saved_brush_is_there_after_a_restart_with_its_turn_and_paint() {
    let dir = temp_dir("restart");
    let mut a = with_library(&dir);
    for (x, y, color) in [(3, 3, RED), (4, 3, BLUE), (3, 4, RED)] {
        a.editor.color = color;
        dot(&mut a, x, y);
    }
    a.editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(3, 3, 2, 2)),
            op: pikslar_core::SelectOp::Replace,
        })
        .expect("select");
    capture(&mut a);
    key(&mut a, Key::Char(b'R'), PLAIN);
    a.brush_menu_action(MenuKind::Brush, 7, (0, 0));
    a.brush_menu_action(MenuKind::Brush, 11, (0, 0));
    assert!(said(&a).starts_with("Saved Brush"), "{}", said(&a));
    let saved = a.editor.brushes().active_custom().expect("brush").clone();
    assert!(dir.join(FILE_NAME).exists());

    let mut b = with_library(&dir);
    assert!(!b.brush_ui.store.is_read());
    b.open_brush_menu((300, 40));
    assert_eq!(b.brush_entries(MenuKind::BrushLibrary(0)).len(), 1);
    b.brush_menu_action(MenuKind::BrushLibrary(0), 0, (300, 40));
    let loaded = b.editor.brushes().active_custom().expect("loaded").clone();
    assert_eq!(loaded.colors(), saved.colors());
    assert_eq!(loaded.orientation(), saved.orientation());
    assert_eq!(loaded.paint(), BrushPaint::Shape);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn removing_from_the_library_asks_once() {
    let dir = temp_dir("remove");
    let mut a = with_library(&dir);
    a.editor.color = RED;
    dot(&mut a, 3, 3);
    a.editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(3, 3, 1, 1)),
            op: pikslar_core::SelectOp::Replace,
        })
        .expect("select");
    capture(&mut a);
    a.brush_menu_action(MenuKind::Brush, 11, (0, 0));
    a.open_brush_menu((300, 40));
    a.brush_menu_action(MenuKind::Brush, 12, (300, 40));
    let rows = a.brush_entries(MenuKind::BrushRemove(0));
    assert!(!rows[0].0.contains("Choose again"));
    a.brush_menu_action(MenuKind::BrushRemove(0), 0, (300, 40));
    assert_eq!(a.brush_ui.store.brushes().len(), 1, "asked, not removed");
    assert!(a.brush_entries(MenuKind::BrushRemove(0))[0]
        .0
        .contains("Choose again"));
    a.brush_menu_action(MenuKind::BrushRemove(0), 0, (300, 40));
    assert!(a.brush_ui.store.brushes().is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn launch_reads_no_brush_file() {
    let dir = temp_dir("launch");
    let junk = dir.join(FILE_NAME);
    fs::write(&junk, b"not a brush library").expect("file");
    let mut a = with_library(&dir);
    draw_whole(&mut a);
    key(&mut a, Key::Char(b'B'), PLAIN);
    assert!(
        !a.brush_ui.store.is_read(),
        "nothing read at launch or by drawing"
    );
    assert_eq!(fs::read(&junk).expect("untouched"), b"not a brush library");
    assert!(!dir.join("brushes.damaged.pkb").exists());
    assert_eq!(a.message, None);
    // The first use reads it, and sets the unreadable file aside with a message.
    a.open_brush_menu((300, 40));
    assert!(a.brush_ui.store.is_read());
    assert!(said(&a).contains("brushes.damaged.pkb"), "{}", said(&a));
    assert!(dir.join("brushes.damaged.pkb").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn with_settings_off_a_brush_cannot_be_saved_and_no_file_appears() {
    let mut a = with_selected_ell();
    a.use_settings(SettingsFile::disabled());
    capture(&mut a);
    a.brush_menu_action(MenuKind::Brush, 11, (0, 0));
    assert!(said(&a).contains("not kept"), "{}", said(&a));
    assert!(a.editor.brushes().active_custom().is_some(), "still usable");
}

#[test]
fn the_brush_menu_has_size_rows_that_change_the_size_and_stop_at_the_ends() {
    let mut a = app(1);
    a.editor.brush_size = 5;
    a.open_brush_menu((300, 40));
    let rows = a.brush_entries(MenuKind::Brush);
    let larger = rows
        .iter()
        .position(|(l, _)| l.starts_with("Larger"))
        .expect("a larger row");
    let smaller = rows
        .iter()
        .position(|(l, _)| l.starts_with("Smaller"))
        .expect("a smaller row");
    assert!(rows[larger].0.contains("5 to 6"), "{}", rows[larger].0);
    a.brush_menu_action(MenuKind::Brush, larger, (0, 0));
    assert_eq!(a.editor.brush_size, 6);
    a.brush_menu_action(MenuKind::Brush, smaller, (0, 0));
    a.brush_menu_action(MenuKind::Brush, smaller, (0, 0));
    assert_eq!(a.editor.brush_size, 4);
    a.editor.brush_size = 1;
    assert!(!a.brush_entries(MenuKind::Brush)[smaller].1, "not below 1");
    a.editor.brush_size = 64;
    assert!(!a.brush_entries(MenuKind::Brush)[larger].1, "not above 64");
}

#[test]
fn some_brush_size_control_can_be_pressed_at_every_named_screen_size() {
    use crate::test_app::draw_whole_at;
    for (name, dpi, w, h) in crate::layout_tests::SCREENS {
        let scale = pikslar_platform::interface_scale(dpi, w, h);
        let mut a = crate::app::App::launch(None, Some(32), None);
        a.resized(w, h, scale);
        draw_whole_at(&mut a, w, h);
        let has = |id: u32| a.ui.widgets().iter().any(|&(_, i)| i == id);
        let stepper = has(140) && has(141);
        let menu_has_size = a
            .brush_entries(MenuKind::Brush)
            .iter()
            .any(|(l, _)| l.starts_with("Larger"));
        assert!(
            stepper || (has(144) && menu_has_size),
            "{name}: no way to size the brush"
        );
    }
}

/// Chooses row `n` of the open brush menu with a pointer: a move, then a press and a release.
fn choose_menu_row(a: &mut App, n: usize) {
    let open = a.menu.expect("a menu is open");
    let r = pikslar_ui::menu::menu_row(a.menu_area(open), n, a.ui.scale);
    click_whole(a, (r.x + r.w / 2, r.y + r.h / 2), PLAIN);
}

/// A captured brush, deselected, with the brush menu open (as in workflow W5, defect D2).
fn with_brush_menu_open() -> App {
    let mut a = with_selected_ell();
    capture(&mut a);
    deselect(&mut a);
    a.ui.record_names();
    a.ui_dirty = true;
    draw_whole(&mut a);
    let at = centre_of(&a, ID_BRUSH + 4);
    click_whole(&mut a, at, PLAIN);
    draw_whole(&mut a);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Brush));
    a
}

#[test]
fn choosing_a_row_in_use_leaves_the_menu_open_and_the_next_click_only_closes_it() {
    // A captured brush already paints its own colors, so that row reads "in use" and is
    // disabled, like every marked row: clicking it does nothing and the menu stays.
    let mut a = with_brush_menu_open();
    assert_eq!(
        a.brush_entries(MenuKind::Brush)[6].0,
        "Paint its own colors, in use"
    );
    let before = painted(&a);
    choose_menu_row(&mut a, 6);
    assert!(a.menu.is_some(), "a row in use is not a choice");
    // The click on the canvas then closes the menu, as a click outside any popup does.
    let at = screen(&a, 20, 20);
    click_whole(&mut a, at, PLAIN);
    assert!(a.menu.is_none());
    assert_eq!(painted(&a), before, "that click only closed the menu");
    let at = screen(&a, 20, 20);
    click_whole(&mut a, at, PLAIN);
    assert_ne!(painted(&a), before, "the next click stamps");
}

#[test]
fn the_first_canvas_click_after_a_row_that_was_chosen_stamps() {
    let mut a = with_brush_menu_open();
    choose_menu_row(&mut a, 7); // Paint as a shape in the color
    assert!(a.menu.is_none(), "choosing a row closes the menu");
    let before = painted(&a);
    let at = screen(&a, 20, 20);
    click_whole(&mut a, at, PLAIN);
    assert_ne!(painted(&a), before, "the first click stamps");
}
