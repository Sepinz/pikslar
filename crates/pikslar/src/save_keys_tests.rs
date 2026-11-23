//! Ctrl+S in every state that keeps keys from the shortcuts: it saves, or it says why not and
//! leaves the state as it was. A new branch in the key routing that could swallow it fails
//! the last test until its state is listed.

use super::*;
use crate::test_app::*;
use pikslar_core::{PaletteCmd, Slice, SliceCmd, SliceRect};
use pikslar_platform::{Button, Window};
use std::fs;
use std::path::{Path, PathBuf};

const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
const PLAIN: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-savekeys-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

/// An app with unsaved work whose file is at `art`, which is then removed so a save shows.
fn saved_once(dir: &Path) -> (App, PathBuf) {
    let art = dir.join("art.piks");
    let mut a = app(1);
    assert!(a.save_to(Window::detached(), art.clone()), "the first save");
    dot(&mut a, 2, 2);
    fs::remove_file(&art).expect("removed");
    (a, art)
}

fn shown(a: &App) -> String {
    a.save_label().map(|l| l.text).unwrap_or_default()
}

fn a_plugin(dir: &Path) -> PathBuf {
    let source = dir.join("plugin");
    fs::create_dir_all(&source).expect("a folder");
    let manifest = r#"{"manifest": 1, "id": "com.example.one", "name": "One", "version": "1.0.0",
"api": 1, "capabilities": ["read-document"], "commands": [
{"id": "count", "title": "Count frames", "script": "count.pks"}]}"#;
    fs::write(source.join("plugin.json"), manifest).expect("written");
    fs::write(source.join("count.pks"), "print(len(frames()));").expect("written");
    source
}

/// Puts `a` in the state `blocker` names, the way a person gets there.
fn enter(a: &mut App, blocker: SaveBlocker, dir: &Path) {
    match blocker {
        SaveBlocker::TagName => a.tag_key(),
        SaveBlocker::LayerName => a.start_layer_naming(0),
        SaveBlocker::SliceName => {
            let bounds = SliceRect {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            };
            let slice = Slice {
                name: "Head".into(),
                color: 0x00ff_00ff,
                bounds,
                pivot: None,
                center: None,
            };
            a.run(SliceCmd::Add {
                slice: Box::new(slice),
            });
            a.start_slice_naming(0);
        }
        SaveBlocker::GroupName => {
            let first = a.editor.doc().colors.swatches()[0];
            a.run(PaletteCmd::AddGroup {
                name: "Skin".into(),
                ids: vec![first].into(),
            });
            a.start_group_naming(0);
        }
        SaveBlocker::TilesetName => {
            let made = a.editor.apply(pikslar_core::TileCmd::NewMapLayer {
                at: 1,
                name: "Floor".into(),
                mode: pikslar_core::ColorMode::Rgba,
                tile_w: 4,
                tile_h: 4,
            });
            let id = made.expect("room").created.expect("a tileset");
            a.start_tileset_naming(pikslar_core::tileset::TilesetId::new(id));
        }
        SaveBlocker::BrushName => {
            let library = dir.join("brushes.pikbrush");
            a.brush_ui.store = crate::brush_store::BrushStore::at(Some(library));
            let _ = a.brush_ui.store.ensure_read();
            a.brush_ui.store.add(moss()).expect("a library brush");
            a.start_brush_naming(0, (300, 40));
        }
        SaveBlocker::KeysDialog => a.open_keys_dialog(),
        SaveBlocker::CanvasDialog => a.open_canvas_dialog(),
        SaveBlocker::PluginReview => {
            a.plugins.store = pikslar_plugin::PluginStore::in_dir(&dir.join("store"));
            a.review_plugin(&a_plugin(dir));
        }
        SaveBlocker::Scripts => a.open_script_panel(),
        SaveBlocker::Menu => a.open_file_menu((0, 0)),
        SaveBlocker::Export => a.open_export_dialog(false),
        SaveBlocker::New => a.open_new_dialog(),
        SaveBlocker::Pointer => {
            let at = screen(a, 4, 4);
            a.pointer_down(at.0, at.1, Button::Left, PLAIN);
        }
    }
    draw(a);
}

/// What the reason says to do.
fn leave(a: &mut App, blocker: SaveBlocker) {
    let reason = blocker.reason();
    if reason.contains("(Enter)") {
        key(a, Key::Enter, PLAIN);
    } else if reason.contains("(Esc") {
        key(a, Key::Escape, PLAIN);
    } else {
        let at = screen(a, 4, 4);
        a.pointer_up(at.0, at.1, Button::Left);
    }
    draw(a);
}

#[test]
fn every_state_that_keeps_ctrl_s_says_why_and_its_own_key_then_lets_it_save() {
    for blocker in ALL_BLOCKERS {
        let dir = temp_dir(&format!("{blocker:?}"));
        let (mut a, art) = saved_once(&dir);
        enter(&mut a, blocker, &dir);
        assert_eq!(a.save_blocker(), Some(blocker), "{blocker:?} is entered");
        key(&mut a, Key::Char(b'S'), CTRL);
        assert!(!art.exists(), "{blocker:?}: nothing saved underneath it");
        assert_eq!(shown(&a), format!("Not saved: {}", blocker.reason()));
        assert_eq!(a.save_blocker(), Some(blocker), "{blocker:?}: still there");
        leave(&mut a, blocker);
        assert_eq!(a.save_blocker(), None, "{blocker:?}: its key ends it");
        frame(&mut a);
        assert!(
            !shown(&a).starts_with("Not saved"),
            "{blocker:?}: {}",
            shown(&a)
        );
        key(&mut a, Key::Char(b'S'), CTRL);
        finish_saves(&mut a);
        assert!(art.exists(), "{blocker:?}: then Ctrl+S saves");
        let _ = fs::remove_dir_all(&dir);
    }
}

fn frame(a: &mut App) {
    use pikslar_platform::Handler;
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    let (w, h) = WINDOW;
    let mut surface = pikslar_render::Surface { px: &mut px, w, h };
    a.frame(&mut surface, Window::detached(), &mut Vec::new());
}

fn moss() -> pikslar_format::brush_lib::LibraryBrush {
    use pikslar_format::brush_lib::{Handle, LibraryBrush, Paint};
    LibraryBrush {
        name: "Moss".to_string(),
        width: 2,
        height: 1,
        paint: Paint::OwnColors,
        orientation: 0,
        handle: Handle::Centre,
        rgba: vec![255, 0, 0, 255, 0, 0, 0, 0],
    }
}

#[test]
fn save_as_is_held_back_the_same_way() {
    let dir = temp_dir("save-as");
    let (mut a, _art) = saved_once(&dir);
    a.open_export_dialog(false);
    let shift = Mods {
        shift: true,
        ..CTRL
    };
    key(&mut a, Key::Char(b'S'), shift);
    assert_eq!(
        shown(&a),
        format!("Not saved: {}", SaveBlocker::Export.reason())
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_file_menus_save_is_held_back_by_what_is_open() {
    let dir = temp_dir("menu");
    let (mut a, art) = saved_once(&dir);
    a.start_layer_naming(0);
    let row = a
        .file_entries()
        .iter()
        .position(|(label, _)| label.starts_with("Save") && !label.starts_with("Save as"))
        .expect("a Save row");
    a.file_menu_action(row, Window::detached());
    assert!(!art.exists(), "nothing saved under the name being typed");
    assert_eq!(
        shown(&a),
        format!("Not saved: {}", SaveBlocker::LayerName.reason())
    );
    key(&mut a, Key::Enter, PLAIN);
    a.file_menu_action(row, Window::detached());
    finish_saves(&mut a);
    assert!(art.exists(), "then the menu saves");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_repeat_of_a_held_ctrl_s_says_nothing_more() {
    let dir = temp_dir("repeat");
    let (mut a, _art) = saved_once(&dir);
    a.open_new_dialog();
    a.key_down(Key::Char(b'S'), CTRL, false, Window::detached());
    assert!(shown(&a).starts_with("Not saved"));
    a.save_unblocked();
    a.key_down(Key::Char(b'S'), CTRL, true, Window::detached());
    assert_eq!(shown(&a), "Unsaved changes", "the first press acted");
    assert!(a.new_dialog.is_some(), "and the dialog kept the key");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_keyboard_shortcuts_dialog_waiting_for_a_key_takes_ctrl_s_as_that_key() {
    let dir = temp_dir("binding");
    let (mut a, art) = saved_once(&dir);
    a.open_keys_dialog();
    if let Some(d) = a.keys_dialog.as_mut() {
        d.waiting = Some(Action::Export);
    }
    key(&mut a, Key::Char(b'S'), CTRL);
    assert!(!art.exists());
    assert!(!shown(&a).starts_with("Not saved"), "{}", shown(&a));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn ctrl_s_saves_while_typing_with_the_text_tool() {
    let dir = temp_dir("text");
    let (mut a, art) = saved_once(&dir);
    a.set_tool(crate::app::ToolChoice::Text);
    a.start_text_by_key();
    a.text_typed('A');
    assert!(a.text.is_some(), "a text session");
    assert_eq!(a.save_blocker(), None);
    key(&mut a, Key::Char(b'S'), CTRL);
    finish_saves(&mut a);
    assert!(art.exists(), "saved");
    let _ = fs::remove_dir_all(&dir);
}

/// The condition of every `if` in `body` whose block returns.
fn returning_branches(body: &str) -> Vec<String> {
    let lines: Vec<&str> = body.lines().collect();
    let mut found = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if !(t.starts_with("if ") && t.ends_with('{')) {
            continue;
        }
        let indent = line.len() - t.len();
        let close = format!("{}}}", " ".repeat(indent));
        let block = lines[i + 1..].iter().take_while(|l| !l.starts_with(&close));
        if block.into_iter().any(|l| l.contains("return")) {
            found.push(t.trim_end_matches('{').trim().to_string());
        }
    }
    found
}

fn function<'a>(source: &'a str, signature: &str, end: &str) -> &'a str {
    let start = source.find(signature).expect("the function");
    let len = source[start..].find(end).expect("its end");
    &source[start..start + len]
}

/// Each branch of `key_down` that ends a key early, and how Save is kept from silence there.
const ROUTES: [(&str, &str); 6] = [
    (
        "if self.save_key_held_back(key, mods, repeat)",
        "this module",
    ),
    (
        "if self.naming_route(key, mods, window)",
        "the naming blockers; text lets it through",
    ),
    (
        "if let Some(modal) = self.modal()",
        "every Modal is a blocker, read from the same list (modal.rs)",
    ),
    (
        "if self.key_cursor_key(key, mods, repeat)",
        "takes only arrows and Space",
    ),
    (
        "if self.pointer_busy() && !allowed_while_held(key)",
        "Pointer",
    ),
    (
        "if self.dock_key(key, mods) || self.slider_key(key, mods)",
        "never letters",
    ),
];

#[test]
fn every_modal_the_key_routing_gives_the_keyboard_to_is_the_blocker_save_names() {
    for modal in crate::modal::Modal::ALL {
        let dir = temp_dir(&format!("modal-{modal:?}"));
        let mut a = app(1);
        enter(&mut a, modal.blocker(), &dir);
        assert_eq!(a.modal(), Some(modal), "{modal:?} takes the keys");
        assert_eq!(a.save_blocker(), Some(modal.blocker()), "{modal:?}");
        assert!(a.panel_open());
        let _ = fs::remove_dir_all(&dir);
    }
}

#[test]
fn the_blocker_named_with_scripts_and_a_menu_open_is_the_one_the_keys_go_to() {
    let mut a = app(1);
    a.open_script_panel();
    a.open_file_menu((0, 0));
    assert_eq!(
        a.modal(),
        Some(crate::modal::Modal::Menu),
        "keys go to the menu"
    );
    assert_eq!(a.save_blocker(), Some(SaveBlocker::Menu));
}

#[test]
fn every_branch_of_the_key_routing_that_can_keep_ctrl_s_has_its_state_listed() {
    let keys = include_str!("keys.rs");
    let body = function(keys, "pub fn key_down(", "self.note_context_key(key);");
    let listed: Vec<&str> = ROUTES.iter().map(|(c, _)| *c).collect();
    let branches = returning_branches(body);
    assert_eq!(
        branches.len(),
        ROUTES.len(),
        "every route is found: {branches:#?}"
    );
    for branch in branches {
        assert!(
            listed.contains(&branch.as_str()),
            "a new branch in key_down can swallow Ctrl+S: list its state in save_keys.rs and \
             ROUTES: {branch}"
        );
    }
    // Every name `naming_route` hands keys to is a blocker.
    let naming = function(keys, "fn naming_route(", "pub fn key_down(");
    let blockers = include_str!("save_keys.rs");
    for field in naming
        .lines()
        .filter_map(|l| l.trim().strip_prefix("} else if self."))
        .chain(
            naming
                .lines()
                .filter_map(|l| l.trim().strip_prefix("if self.")),
        )
        .filter_map(|l| l.split(".is_some()").next())
        .filter(|f| *f != "text" && !f.starts_with("naming_clipboard_key"))
    {
        assert!(
            blockers.contains(&format!("self.{field}.is_some()")),
            "naming_route hands keys to {field}, which has no blocker"
        );
    }
    // Text's own route returns only for keys it uses, and Ctrl+S is not one (tested above).
    assert!(naming.contains("return self.text_key(key, mods);"));
}

#[test]
fn save_bound_to_a_plain_letter_is_typed_into_a_name_not_held_back() {
    let mut a = app(1);
    let plain_y = crate::shortcuts::Binding {
        letter: b'Y',
        ctrl: false,
        shift: false,
    };
    a.keymap.bind(Action::Save, Some(plain_y));
    a.start_layer_naming(0);
    key(&mut a, Key::Char(b'Y'), PLAIN);
    assert!(a.save.blocked.is_none(), "{:?}", a.save.blocked);
    key(&mut a, Key::Enter, PLAIN);
    assert!(
        a.editor.doc().layers()[0].name.ends_with('y'),
        "the letter went into the name: {}",
        a.editor.doc().layers()[0].name
    );
}

#[test]
fn save_bound_to_a_plain_letter_under_a_menu_says_not_saved_and_does_nothing_else() {
    let mut a = app(1);
    let plain_y = crate::shortcuts::Binding {
        letter: b'Y',
        ctrl: false,
        shift: false,
    };
    a.keymap.bind(Action::Save, Some(plain_y));
    a.open_file_menu((0, 0));
    let before = a.editor.revision();
    key(&mut a, Key::Char(b'Y'), PLAIN);
    assert_eq!(
        a.save.blocked.as_deref(),
        Some(SaveBlocker::Menu.reason()),
        "the letter is not dropped without a word"
    );
    assert!(a.menu.is_some(), "the menu still has the keyboard");
    assert_eq!(a.editor.revision(), before);
}
