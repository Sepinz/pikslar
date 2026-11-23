//! Renaming a library brush, and the pictures of the library in the brush menus.

use crate::app::App;
use crate::brush_store::FILE_NAME;
use crate::frame_menu::MenuKind;
use crate::settings::SettingsFile;
use crate::test_app::*;
use pikslar_format::brush_lib::{write_library, Handle, LibraryBrush, Paint};
use pikslar_platform::{Button, Key};
use std::fs;
use std::path::{Path, PathBuf};

const AT: (i32, i32) = (300, 40);
const CTRL: pikslar_platform::Mods = pikslar_platform::Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
/// The main brush menu's rename row.
const RENAME_ROW: usize = 13;

fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("pikslar-brush-name-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

fn red(name: &str) -> LibraryBrush {
    LibraryBrush {
        name: name.to_string(),
        width: 2,
        height: 1,
        paint: Paint::OwnColors,
        orientation: 0,
        handle: Handle::Centre,
        rgba: vec![255, 0, 0, 255, 0, 0, 0, 0],
    }
}

/// An app whose library holds `brushes`, read as the menu opens.
fn with_brushes(dir: &Path, brushes: &[LibraryBrush]) -> App {
    let bytes = write_library(brushes).expect("bytes");
    fs::write(dir.join(FILE_NAME), bytes).expect("file");
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(dir));
    a.open_brush_menu(AT);
    a
}

fn type_text(a: &mut App, text: &str) {
    text.chars().for_each(|c| a.text_typed(c));
}

fn start_renaming(a: &mut App, row: usize) {
    a.brush_menu_action(MenuKind::Brush, RENAME_ROW, AT);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::BrushRename(0)));
    a.close_menu();
    a.brush_menu_action(MenuKind::BrushRename(0), row, AT);
}

fn names(a: &App) -> Vec<String> {
    a.brush_ui
        .store
        .brushes()
        .iter()
        .map(|b| b.name.clone())
        .collect()
}

#[test]
fn the_menu_offers_rename_only_with_a_library() {
    let dir = temp_dir("offer");
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(&dir));
    a.open_brush_menu(AT);
    let rows = a.brush_entries(MenuKind::Brush);
    assert_eq!(rows[RENAME_ROW], ("Rename in the library...".into(), false));
    let b = with_brushes(&dir, &[red("Moss")]);
    assert!(b.brush_entries(MenuKind::Brush)[RENAME_ROW].1);
    assert_eq!(b.brush_entries(MenuKind::BrushRename(0))[0].0, "Moss  2x1");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_typed_name_is_kept_with_enter_and_is_there_after_a_restart() {
    let dir = temp_dir("enter");
    let mut a = with_brushes(&dir, &[red("Moss"), red("Bark")]);
    start_renaming(&mut a, 1);
    assert_eq!(
        a.brush_ui
            .naming
            .as_ref()
            .map(|n| n.edit.text().to_string()),
        Some("Bark".to_string())
    );
    let revision = a.editor.revision();
    key(&mut a, Key::Char(b'A'), CTRL);
    type_text(&mut a, "Old bark");
    key(&mut a, Key::Enter, PLAIN);
    assert!(a.brush_ui.naming.is_none());
    assert_eq!(names(&a), ["Moss", "Old bark"]);
    assert!(said(&a).contains("Old bark"), "{}", said(&a));
    assert_eq!(a.editor.revision(), revision, "app data: no undo step");
    assert!(!a.is_modified(), "and the document is not changed");

    let b = with_brushes_read(&dir);
    assert_eq!(names(&b), ["Moss", "Old bark"]);
    let _ = fs::remove_dir_all(&dir);
}

/// A second run on the same settings folder, the file as the first one left it.
fn with_brushes_read(dir: &Path) -> App {
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(dir));
    a.open_brush_menu(AT);
    a
}

#[test]
fn escape_or_an_empty_name_leaves_it_and_a_press_elsewhere_keeps_it() {
    let dir = temp_dir("escape");
    let mut a = with_brushes(&dir, &[red("Moss")]);
    let before = fs::read(dir.join(FILE_NAME)).expect("file");
    start_renaming(&mut a, 0);
    type_text(&mut a, "Nope");
    key(&mut a, Key::Escape, PLAIN);
    assert!(a.brush_ui.naming.is_none());
    start_renaming(&mut a, 0);
    key(&mut a, Key::Backspace, PLAIN);
    key(&mut a, Key::Enter, PLAIN);
    assert_eq!(names(&a), ["Moss"]);
    assert_eq!(fs::read(dir.join(FILE_NAME)).expect("file"), before);
    // A press anywhere else keeps what was typed, as a layer's name.
    start_renaming(&mut a, 0);
    type_text(&mut a, "Lichen");
    a.pointer_down(5, 5, Button::Left, PLAIN);
    a.pointer_up(5, 5, Button::Left);
    assert!(a.brush_ui.naming.is_none());
    assert_eq!(names(&a), ["Lichen"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn hidden_characters_go_and_a_taken_name_is_numbered() {
    let dir = temp_dir("rule");
    let mut a = with_brushes(&dir, &[red("Moss"), red("Bark")]);
    start_renaming(&mut a, 1);
    type_text(&mut a, " Mo\u{202E}ss\u{200B} ");
    key(&mut a, Key::Enter, PLAIN);
    assert_eq!(names(&a), ["Moss", "Moss 2"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn while_a_brush_is_named_keys_type_and_are_no_shortcuts() {
    let dir = temp_dir("keys");
    let mut a = with_brushes(&dir, &[red("Moss")]);
    start_renaming(&mut a, 0);
    a.message = None;
    let tool = a.tool;
    key(&mut a, Key::Char(b'E'), PLAIN);
    key(&mut a, Key::Char(b'B'), PLAIN);
    assert_eq!(a.tool, tool, "E and B are letters of the name, not tools");
    assert_eq!(
        a.brush_ui
            .naming
            .as_ref()
            .map(|n| n.edit.text().to_string()),
        Some("eb".to_string())
    );
    assert_eq!(
        a.status_text(),
        Some("Type the brush's name. Enter keeps it, Esc cancels")
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_library_menus_show_each_brush_as_a_picture_beside_its_name() {
    let dir = temp_dir("pictures");
    let mut a = with_brushes(&dir, &[red("Moss"), red("Bark")]);
    a.ui.record_names();
    a.brush_menu_action(MenuKind::Brush, 10, AT);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::BrushLibrary(0)));
    let (w, h) = WINDOW;
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = pikslar_render::Surface { px: &mut px, w, h };
    a.draw_interface(
        &mut surface,
        pikslar_platform::Window::detached(),
        &mut Vec::new(),
    );
    let layout = a.ui.layout().expect("names");
    let picture = layout
        .lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .find(|f| f.get(5) == Some(&"Picture of Bark"))
        .expect("the second row has a picture");
    let num = |i: usize| picture[i].parse::<i32>().expect("number");
    let (x, y, side) = (num(1), num(2), num(3));
    assert_eq!(side, 16 * a.ui.scale);
    assert_eq!(num(4), side);
    // The red left half of the two by one brush, and the checker where it is clear.
    let pixel = |px: &[u32], dx: i32, dy: i32| px[((y + dy) * w + x + dx) as usize];
    assert_eq!(pixel(&px, 0, side / 2), 0xffff_0000);
    assert_ne!(pixel(&px, side - 1, side / 2), 0xffff_0000);
    let label = layout
        .lines()
        .find(|l| l.split('\t').nth(5) == Some("Bark  2x1"))
        .expect("the name is still listed");
    let label_x: i32 = label.split('\t').nth(1).expect("x").parse().expect("x");
    assert!(
        label_x < x,
        "the row starts before the picture, and its words after it"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn drawing_the_menu_again_builds_no_pictures() {
    let dir = temp_dir("again");
    let mut a = with_brushes(&dir, &[red("Moss")]);
    a.brush_menu_action(MenuKind::Brush, 10, AT);
    draw_whole(&mut a);
    assert!(
        !a.brush_ui.thumbs.update(&a.brush_ui.store),
        "built by the first draw"
    );
    draw_whole(&mut a);
    assert!(!a.brush_ui.thumbs.update(&a.brush_ui.store), "and kept");
    let _ = fs::remove_dir_all(&dir);
}
