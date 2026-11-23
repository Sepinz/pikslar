//! The palette in the app: dragging swatches, the menu, groups and ramps, the remaps and Replace
//! color, and palette files. Every action is one step of undo.

use crate::app::App;
use crate::frame_menu::MenuKind;
use crate::test_app::*;
use pikslar_core::{rgba, GroupKind, LoadHow, StepCmd};
use pikslar_platform::{Button, Key, Window};
use pikslar_render::Rect;
use std::path::PathBuf;

const RED: u32 = rgba(255, 0, 0, 255);
const GREEN: u32 = rgba(0, 255, 0, 255);

fn swatches(a: &App) -> Vec<u32> {
    a.editor.doc().colors.swatches().to_vec()
}

fn said(a: &App) -> String {
    a.message
        .as_ref()
        .map(|(t, _)| t.clone())
        .unwrap_or_default()
}

fn depth(a: &App) -> usize {
    a.editor.history().undo_depth()
}

fn undo(a: &mut App) {
    a.run(StepCmd::Undo);
}

fn cell(a: &App, id: u32) -> Rect {
    a.ui.widgets()
        .iter()
        .find(|(_, w)| *w == 1_000 + id)
        .map(|(r, _)| *r)
        .expect("that swatch is drawn")
}

fn centre(r: Rect) -> (i32, i32) {
    (r.x + r.w / 2, r.y + r.h / 2)
}

/// Presses on one swatch, moves to a point and lets go there, drawing the whole interface as the
/// window does between events.
fn drag_to(a: &mut App, id: u32, to: (i32, i32)) {
    draw_whole(a);
    let from = centre(cell(a, id));
    a.pointer_down(from.0, from.1, Button::Left, PLAIN);
    draw_whole(a);
    a.ui.track(to.0, to.1);
    draw_whole(a);
    a.pointer_up(to.0, to.1, Button::Left);
    draw_whole(a);
}

fn pick(a: &mut App, id: u32) {
    draw_whole(a);
    drag_to(a, id, centre(cell(a, id)));
}

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("pikslar-palette-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

/// A palette image with two colors painted: RED at (1,1) and GREEN at (2,1).
fn two_colors() -> App {
    let mut a = app(1);
    for (x, color) in [(1, RED), (2, GREEN)] {
        a.editor.color = color;
        dot(&mut a, x, 1);
    }
    draw_whole(&mut a);
    a
}

fn menu(a: &mut App, kind: MenuKind) -> Vec<(String, bool)> {
    a.open_palette_kind((300, 300), kind);
    a.palette_entries(kind)
        .into_iter()
        .map(|(l, on)| (l.into_owned(), on))
        .collect()
}

#[test]
fn dragging_a_swatch_to_another_cell_moves_it_in_one_step() {
    let mut a = app(1);
    let before = swatches(&a);
    let moved = before[0];
    draw_whole(&mut a);
    let to = centre(cell(&a, before[3]));
    let steps = depth(&a);
    drag_to(&mut a, moved, to);
    let mut want = before.clone();
    want.remove(0);
    want.insert(3, moved);
    assert_eq!(swatches(&a), want);
    assert_eq!(depth(&a), steps + 1);
    undo(&mut a);
    assert_eq!(swatches(&a), before);
}

#[test]
fn a_drag_moves_order_only_so_no_pixel_changes() {
    let mut a = two_colors();
    let cels = a.editor.doc().layers()[0].cel(0).to_cells(32, 32);
    let colors = swatches(&a);
    let to = centre(cell(&a, *colors.last().expect("swatches")));
    drag_to(&mut a, colors[0], to);
    assert_eq!(a.editor.doc().layers()[0].cel(0).to_cells(32, 32), cels);
}

#[test]
fn a_press_and_release_on_one_swatch_still_picks_it_and_adds_no_step() {
    let mut a = app(1);
    let id = swatches(&a)[2];
    let steps = depth(&a);
    pick(&mut a, id);
    assert_eq!(a.selected_swatch, Some(id));
    assert_eq!(a.editor.color, a.editor.doc().colors.color(id));
    assert_eq!(depth(&a), steps);
}

#[test]
fn letting_go_off_the_grid_moves_nothing() {
    let mut a = app(1);
    let before = swatches(&a);
    drag_to(&mut a, before[0], (5, 5));
    assert_eq!(swatches(&a), before);
}

#[test]
fn the_last_two_swatches_clicked_are_the_pair() {
    let mut a = app(1);
    let s = swatches(&a);
    assert_eq!(a.swatch_pair(), None);
    pick(&mut a, s[1]);
    assert_eq!(a.swatch_pair(), None, "one swatch is not a pair");
    pick(&mut a, s[4]);
    assert_eq!(a.swatch_pair(), Some((s[1], s[4])));
    pick(&mut a, s[4]);
    assert_eq!(a.swatch_pair(), Some((s[1], s[4])), "the same one again");
}

#[test]
fn a_right_click_on_the_palette_header_opens_the_menu() {
    let mut a = app(1);
    draw_whole(&mut a);
    let (x, y) = centre(a.palette_ui.header);
    a.pointer_down(x, y, Button::Right, PLAIN);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Palette));
    draw_whole(&mut a);
    let rows = menu(&mut a, MenuKind::Palette);
    let names: Vec<&str> = rows.iter().map(|(l, _)| l.as_str()).collect();
    assert!(names.contains(&"Indices: renumber to palette order"));
    assert!(names.contains(&"Pixels: replace a color..."));
    assert!(names.contains(&"Import palette..."));
    let on = |label: &str| rows.iter().find(|(l, _)| l == label).expect("row").1;
    assert!(!on("New ramp..."), "a ramp needs two swatches");
    assert!(!on("Pixels: replace a color..."));
}

#[test]
fn sorting_is_one_step_and_undone_whole() {
    let mut a = app(1);
    let before = swatches(&a);
    a.palette_menu_action(MenuKind::PaletteSort, 1, (0, 0), Window::detached());
    assert_ne!(swatches(&a), before);
    undo(&mut a);
    assert_eq!(swatches(&a), before);
}

#[test]
fn a_ramp_between_the_last_two_swatches_is_a_marked_group() {
    let mut a = app(1);
    let s = swatches(&a);
    pick(&mut a, s[0]);
    pick(&mut a, s[5]);
    let rows = menu(&mut a, MenuKind::PaletteRamp);
    assert_eq!(rows.len(), 4);
    assert!(rows[0].0.starts_with("3 steps, #"), "{}", rows[0].0);
    let (steps, count) = (depth(&a), swatches(&a).len());
    a.palette_menu_action(MenuKind::PaletteRamp, 0, (0, 0), Window::detached());
    let table = &a.editor.doc().colors;
    assert_eq!(table.groups().len(), 1);
    assert_eq!(table.groups()[0].kind, GroupKind::Ramp);
    assert_eq!(swatches(&a).len(), count + 3);
    assert_eq!(a.palette_ui.marked, Some(0));
    assert_eq!(depth(&a), steps + 1);
    undo(&mut a);
    assert!(a.editor.doc().colors.groups().is_empty());
}

#[test]
fn a_new_group_is_named_in_the_header_and_renamed_as_one_step() {
    let mut a = app(1);
    let s = swatches(&a);
    pick(&mut a, s[2]);
    a.palette_menu_action(MenuKind::PaletteGroups(0), 0, (0, 0), Window::detached());
    assert_eq!(a.editor.doc().colors.groups()[0].name, "Group 1");
    assert!(a.palette_ui.naming.is_some(), "named at once");
    let steps = depth(&a);
    for _ in 0..7 {
        key(&mut a, Key::Backspace, PLAIN);
    }
    for c in "Skin".chars() {
        a.text_typed(c);
    }
    key(&mut a, Key::Enter, PLAIN);
    assert_eq!(a.editor.doc().colors.groups()[0].name, "Skin");
    assert_eq!(depth(&a), steps + 1);
    undo(&mut a);
    assert_eq!(a.editor.doc().colors.groups()[0].name, "Group 1");
}

#[test]
fn escape_leaves_the_name_and_a_press_elsewhere_keeps_it() {
    let mut a = app(1);
    let s = swatches(&a);
    pick(&mut a, s[2]);
    a.palette_menu_action(MenuKind::PaletteGroups(0), 0, (0, 0), Window::detached());
    a.text_typed('x');
    key(&mut a, Key::Escape, PLAIN);
    assert_eq!(a.editor.doc().colors.groups()[0].name, "Group 1");
    a.start_group_naming(0);
    a.text_typed('x');
    pick(&mut a, s[3]);
    assert_eq!(a.editor.doc().colors.groups()[0].name, "Group 1x");
}

#[test]
fn group_members_are_added_taken_out_and_the_group_removed() {
    let mut a = app(1);
    let s = swatches(&a);
    pick(&mut a, s[1]);
    a.palette_menu_action(MenuKind::PaletteGroups(0), 0, (0, 0), Window::detached());
    key(&mut a, Key::Enter, PLAIN);
    pick(&mut a, s[3]);
    let rows = menu(&mut a, MenuKind::PaletteGroup(0));
    assert!(rows[2].1 && !rows[3].1, "not a member yet");
    a.palette_menu_action(MenuKind::PaletteGroup(0), 2, (0, 0), Window::detached());
    assert_eq!(a.editor.doc().colors.groups()[0].ids, [s[1], s[3]]);
    a.palette_menu_action(MenuKind::PaletteGroup(0), 3, (0, 0), Window::detached());
    assert_eq!(a.editor.doc().colors.groups()[0].ids, [s[1]]);
    let list = menu(&mut a, MenuKind::PaletteGroups(0));
    assert_eq!(list[1].0, "Group 1  (1)");
    a.palette_menu_action(MenuKind::PaletteGroup(0), 4, (0, 0), Window::detached());
    assert!(a.editor.doc().colors.groups().is_empty());
    assert_eq!(a.palette_ui.marked, None);
    assert_eq!(swatches(&a), s, "the swatches stay");
}

#[test]
fn replace_color_changes_pixels_of_the_earlier_swatch_to_the_later_one() {
    let mut a = app(1);
    let s = swatches(&a);
    let (from, to) = (s[0], s[1]);
    let (old, new) = (
        a.editor.doc().colors.color(from),
        a.editor.doc().colors.color(to),
    );
    a.editor.color = old;
    dot(&mut a, 1, 1);
    pick(&mut a, from);
    pick(&mut a, to);
    let rows = menu(&mut a, MenuKind::PaletteReplace);
    let want = format!(
        "This layer: {} to {}",
        crate::color_ui::hex(old),
        crate::color_ui::hex(new)
    );
    assert_eq!(rows[0].0, want);
    let steps = depth(&a);
    a.palette_menu_action(MenuKind::PaletteReplace, 1, (0, 0), Window::detached());
    let cel = a.editor.doc().layers()[0].cel(0).to_cells(32, 32);
    assert_eq!(cel[32 + 1], to);
    assert_eq!(
        a.editor.doc().colors.color(from),
        old,
        "the swatch is untouched"
    );
    assert_eq!(depth(&a), steps + 1);
    assert!(said(&a).contains(&crate::color_ui::hex(old)));
    undo(&mut a);
    let cel = a.editor.doc().layers()[0].cel(0).to_cells(32, 32);
    assert_eq!(cel[32 + 1], from);
}

#[test]
fn the_remaps_keep_every_frame_and_are_one_step_each() {
    let mut a = two_colors();
    let before = a.editor.doc().clone();
    let id = swatches(&a)[7];
    pick(&mut a, id);
    for n in 3..6 {
        let steps = depth(&a);
        a.palette_menu_action(MenuKind::Palette, n, (0, 0), Window::detached());
        assert!(depth(&a) <= steps + 1, "row {n}");
        let doc = a.editor.doc();
        let look = |d: &pikslar_core::Document| {
            d.layers()[0]
                .cel(0)
                .to_cells(32, 32)
                .iter()
                .map(|&c| d.colors.color(c))
                .collect::<Vec<_>>()
        };
        assert_eq!(look(doc), look(&before), "row {n}");
    }
}

#[test]
fn a_remap_forgets_the_swatch_pair_because_ids_moved() {
    let mut a = two_colors();
    let s = swatches(&a);
    a.run(pikslar_core::PaletteCmd::MoveSwatch { from: 5, to: 0 });
    pick(&mut a, s[3]);
    pick(&mut a, s[0]);
    assert!(a.swatch_pair().is_some());
    a.palette_menu_action(MenuKind::Palette, 3, (0, 0), Window::detached());
    assert_eq!(a.selected_swatch, None);
    assert_eq!(a.swatch_pair(), None);
}

#[test]
fn a_remap_that_changes_nothing_says_so() {
    let mut a = two_colors();
    a.palette_menu_action(MenuKind::Palette, 3, (0, 0), Window::detached());
    let steps = depth(&a);
    a.palette_menu_action(MenuKind::Palette, 3, (0, 0), Window::detached());
    assert_eq!(depth(&a), steps);
    assert!(said(&a).starts_with("Already so"));
}

fn write(dir: &std::path::Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("written");
    path
}

#[test]
fn import_appends_replaces_and_swaps_colors_each_as_one_step() {
    let dir = temp_dir("import");
    let mut a = app(1);
    let start = swatches(&a).len();
    let hex = write(&dir, "two.hex", "112233\n445566\n");
    let steps = depth(&a);
    let said = a
        .import_palette_from(&hex, LoadHow::Append)
        .expect("imported");
    assert!(said.starts_with("Imported 2 colors from two.hex"));
    assert_eq!(swatches(&a).len(), start + 2);
    assert_eq!(depth(&a), steps + 1);
    let again = a.import_palette_from(&hex, LoadHow::Append).expect("again");
    assert_eq!(again, "The palette already matches two.hex");
    assert_eq!(depth(&a), steps + 1, "nothing changed, no step");
    a.import_palette_from(&hex, LoadHow::Replace)
        .expect("replace");
    let colors = &a.editor.doc().colors;
    let now: Vec<u32> = swatches(&a).iter().map(|&i| colors.color(i)).collect();
    assert_eq!(
        now,
        [rgba(0x11, 0x22, 0x33, 255), rgba(0x44, 0x55, 0x66, 255)]
    );
    undo(&mut a);
    assert_eq!(swatches(&a).len(), start + 2);
    std::fs::remove_dir_all(&dir).expect("clean up");
}

#[test]
fn load_as_colors_needs_a_list_as_long_as_the_swatches_and_keeps_ids() {
    let dir = temp_dir("swap");
    let mut a = app(1);
    let ids = swatches(&a);
    let text: String = (0..ids.len())
        .map(|i| format!("{:02x}0000\n", i + 1))
        .collect();
    let full = write(&dir, "swap.hex", &text);
    a.import_palette_from(&full, LoadHow::AsColors)
        .expect("swap");
    assert_eq!(swatches(&a), ids, "no id moved");
    assert_eq!(a.editor.doc().colors.color(ids[0]), rgba(1, 0, 0, 255));
    let short = write(&dir, "short.hex", "010203\n");
    let before = depth(&a);
    let why = a
        .import_palette_from(&short, LoadHow::AsColors)
        .expect_err("refused");
    assert!(why.starts_with("short.hex: "), "{why}");
    assert_eq!(depth(&a), before);
    std::fs::remove_dir_all(&dir).expect("clean up");
}

#[test]
fn a_bad_palette_file_is_refused_whole_and_names_the_line() {
    let dir = temp_dir("bad");
    let mut a = app(1);
    let before = swatches(&a);
    let bad = write(&dir, "bad.gpl", "GIMP Palette\nName: x\n#\n1 2 x\n");
    let why = a
        .import_palette_from(&bad, LoadHow::Append)
        .expect_err("bad");
    assert!(why.starts_with("bad.gpl: Line 4:"), "{why}");
    assert_eq!(swatches(&a), before);
    let missing = dir.join("nothing.pal");
    assert!(a.import_palette_from(&missing, LoadHow::Append).is_err());
    std::fs::remove_dir_all(&dir).expect("clean up");
}

#[test]
fn export_writes_the_swatches_in_the_order_shown_and_says_what_it_did() {
    let dir = temp_dir("export");
    let mut a = app(1);
    let s = swatches(&a);
    draw_whole(&mut a);
    let to = centre(cell(&a, s[2]));
    drag_to(&mut a, s[0], to);
    let order: Vec<u32> = swatches(&a)
        .iter()
        .map(|&i| a.editor.doc().colors.color(i))
        .collect();
    let path = dir.join("out.hex");
    let modified = a.is_modified();
    let said = a.export_palette_to(&path).expect("written");
    assert_eq!(said, format!("Exported {} colors to out.hex", order.len()));
    let read = crate::files::read_palette(&path).expect("reads back");
    assert_eq!(read.palette.packed(), order);
    assert_eq!(a.is_modified(), modified, "export changes nothing");
    std::fs::remove_dir_all(&dir).expect("clean up");
}

#[test]
fn the_plain_gpl_name_is_added_to_a_name_without_a_palette_extension() {
    let a = crate::palette_io::palette_target(std::path::Path::new("x/mine"));
    assert_eq!(a, PathBuf::from("x/mine.gpl"));
    let b = crate::palette_io::palette_target(std::path::Path::new("x/mine.PAL"));
    assert_eq!(b, PathBuf::from("x/mine.PAL"));
}

#[test]
fn an_import_tells_every_note_the_reader_made() {
    let said = crate::palette_io::imported_message(3, "a.gpl", &["One".into(), "Two".into()]);
    assert_eq!(said, "Imported 3 colors from a.gpl. One. Two");
    assert_eq!(
        crate::palette_io::imported_message(1, "a.hex", &[]),
        "Imported 1 colors from a.hex"
    );
    let dir = temp_dir("named");
    let mut a = app(1);
    let gpl = write(&dir, "n.gpl", "GIMP Palette\n1 2 3 Sky\n4 5 6\n");
    let said = a
        .import_palette_from(&gpl, LoadHow::Append)
        .expect("imported");
    assert!(
        said.contains("1 color name(s) in the file are not kept"),
        "{said}"
    );
    std::fs::remove_dir_all(&dir).expect("clean up");
}

/// Presses and releases on the swatch `id` with no draw between, as two posted messages do.
fn click_without_a_draw_between(a: &mut App, id: u32) {
    draw_whole(a);
    let (x, y) = centre(cell(a, id));
    a.pointer_down(x, y, Button::Left, PLAIN);
    a.pointer_up(x, y, Button::Left);
    draw_whole(a);
}

fn channels(c: u32) -> [u32; 4] {
    [c >> 24, (c >> 16) & 255, (c >> 8) & 255, c & 255]
}

/// Swatch names are the ids, not the places: "Swatch n" is id n - 1 (id 0 is clear), so a name
/// keeps its colour when a drag moves the swatch. Automation that means "the nth cell" must look
/// at the places, not the names.
#[test]
fn after_a_drag_the_pair_and_the_ramp_follow_the_clicked_swatches() {
    let mut a = app(1);
    let before = swatches(&a);
    draw_whole(&mut a);
    let to = centre(cell(&a, before[3]));
    drag_to(&mut a, before[0], to);
    let now = swatches(&a);
    assert_eq!(&now[..4], &[before[1], before[2], before[3], before[0]]);
    // The first two places, clicked by where they are now.
    pick(&mut a, now[0]);
    pick(&mut a, now[1]);
    assert_eq!(a.swatch_pair(), Some((now[0], now[1])));
    let table = &a.editor.doc().colors;
    let (start, end) = (table.color(now[0]), table.color(now[1]));
    a.palette_menu_action(MenuKind::PaletteRamp, 0, (0, 0), Window::detached());
    let table = &a.editor.doc().colors;
    let added = &swatches(&a)[before.len()..];
    assert_eq!(added.len(), 3);
    for &id in added {
        let (c, s, e) = (channels(table.color(id)), channels(start), channels(end));
        for i in 0..4 {
            assert!(c[i] >= s[i].min(e[i]) && c[i] <= s[i].max(e[i]), "{c:?}");
        }
    }
    // The name stayed with the colour, not the place.
    let named = a.ui.widgets().iter().any(|(_, w)| *w == 1_000 + before[0]);
    assert!(named);
}

#[test]
fn replace_color_after_a_drag_uses_the_swatches_clicked() {
    let mut a = app(1);
    let s = swatches(&a);
    a.editor.color = a.editor.doc().colors.color(s[0]);
    dot(&mut a, 1, 1);
    draw_whole(&mut a);
    let to = centre(cell(&a, s[3]));
    drag_to(&mut a, s[0], to);
    pick(&mut a, s[0]);
    pick(&mut a, s[1]);
    assert_eq!(a.swatch_pair(), Some((s[0], s[1])));
    a.palette_menu_action(MenuKind::PaletteReplace, 1, (0, 0), Window::detached());
    let cel = a.editor.doc().layers()[0].cel(0).to_cells(32, 32);
    assert_eq!(cel[32 + 1], s[1]);
}

#[test]
fn a_click_whose_press_and_release_come_before_a_draw_still_sets_the_colour() {
    let mut a = app(1);
    let s = swatches(&a);
    for &id in &[s[3], s[0], s[7]] {
        click_without_a_draw_between(&mut a, id);
        assert_eq!(a.selected_swatch, Some(id));
        assert_eq!(a.editor.color, a.editor.doc().colors.color(id));
    }
}

#[test]
fn a_click_after_a_canvas_stroke_sets_the_colour() {
    let mut a = app(1);
    let s = swatches(&a);
    dot(&mut a, 3, 3);
    draw_whole(&mut a);
    let (x, y) = screen(&a, 3, 3);
    a.pointer_down(x, y, Button::Left, PLAIN);
    a.pointer_up(x, y, Button::Left);
    draw_whole(&mut a);
    click_without_a_draw_between(&mut a, s[5]);
    assert_eq!(a.editor.color, a.editor.doc().colors.color(s[5]));
}
