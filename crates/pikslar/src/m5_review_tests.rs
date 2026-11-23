//! Fixes from the review of M5 as a whole, each pinned by a test that failed first.

use crate::app::App;
use crate::settings::SettingsFile;
use crate::shortcuts::{Action, Binding};
use crate::test_app::*;
use pikslar_platform::{Button, Key, Mods, Window};
use pikslar_render::Rect;
use std::fs;

const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-m5-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

fn widget(a: &App, id: u32) -> Option<Rect> {
    a.ui.widgets()
        .iter()
        .find(|(_, w)| *w == id)
        .map(|(r, _)| *r)
}

#[test]
fn opening_a_file_keeps_the_onion_skin_style_and_writes_nothing() {
    let dir = temp_dir("onion");
    let walk = dir.join("walk.piks");
    crate::files::save(&walk, app(1).editor.doc()).expect("saved");
    let mut a = app(1);
    a.use_settings(SettingsFile::in_dir(&dir.join("settings")));
    a.comp.onion_style.strength = 0;
    a.comp.onion_style.future = false;
    a.tick_settings();
    let kept = a.comp.onion_style;
    a.open_now(Window::detached(), walk);
    assert_eq!(
        a.comp.onion_style, kept,
        "a setting, not part of the document"
    );
    a.tick_settings();
    let mut b = app(1);
    b.use_settings(SettingsFile::in_dir(&dir.join("settings")));
    assert_eq!(b.comp.onion_style, kept);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn controls_under_the_keys_dialog_show_no_tooltip() {
    let (w, h) = (1280, 800);
    let mut a = app(1);
    draw_whole_at(&mut a, w, h);
    let rects: Vec<Rect> = a.ui.widgets().iter().map(|(r, _)| *r).collect();
    // A control that does show a tooltip with nothing over it.
    let shows = rects.into_iter().find(|r| {
        a.hint = None;
        a.ui.mouse = (r.x + r.w / 2, r.y + r.h / 2);
        draw_whole_at(&mut a, w, h);
        a.hint.is_some()
    });
    let r = shows.expect("some control has a tooltip");
    a.open_keys_dialog();
    a.hint = None;
    a.ui.mouse = (r.x + r.w / 2, r.y + r.h / 2);
    draw_whole_at(&mut a, w, h);
    let inside = widget(&a, 900).is_some_and(|d| d.contains(a.ui.mouse.0, a.ui.mouse.1));
    assert!(inside || a.hint.is_none(), "{r:?} said {:?}", a.hint);
}

#[test]
fn a_section_folded_for_room_says_so_and_a_click_changes_nothing() {
    let w = 1280;
    let mut found = false;
    for h in (300..=900).step_by(20) {
        let mut a = app(1);
        a.resized(w, h, 1);
        draw_whole_at(&mut a, w, h);
        let Some(i) = a
            .layout
            .solved
            .area(crate::workspace::model::Side::Right)
            .slots()
            .iter()
            .position(|s| s.panel == Some(crate::workspace::model::Panel::Palette) && !s.open)
        else {
            continue;
        };
        let id =
            crate::dock::strip_id(crate::workspace::model::Side::Right, i) + pikslar_ui::tabs::FOLD;
        let Some(r) = widget(&a, id) else { continue };
        a.ui.mouse = (r.x + r.w / 2, r.y + r.h / 2);
        draw_whole_at(&mut a, w, h);
        if a.hint.as_deref() != Some("Folded for room. A taller window shows it") {
            continue;
        }
        found = true;
        let before = a.workspaces.clone();
        let (x, y) = a.ui.mouse;
        a.pointer_down(x, y, Button::Left, PLAIN);
        draw_whole_at(&mut a, w, h);
        a.pointer_up(x, y, Button::Left);
        draw_whole_at(&mut a, w, h);
        assert_eq!(a.workspaces, before, "{h}: the choice by hand is kept");
    }
    assert!(found, "some height folds the palette for room");
}

#[test]
fn a_character_typed_after_a_ctrl_shortcut_that_starts_naming_is_kept() {
    let mut a = app(2);
    let ctrl_t = Binding {
        letter: b'T',
        ctrl: true,
        shift: false,
    };
    a.set_binding(Action::Tag, Some(ctrl_t));
    a.key_down(Key::Char(b'T'), CTRL, false, Window::detached());
    assert!(a.timeline.naming.is_some(), "Ctrl T starts naming");
    a.text_typed('@');
    assert!(
        a.timeline.name_edit.text().ends_with('@'),
        "{}",
        a.timeline.name_edit.text()
    );
}

#[test]
fn a_held_escape_in_the_keys_dialog_only_stops_the_wait() {
    let mut a = app(1);
    a.open_keys_dialog();
    a.keys_dialog = a.keys_dialog.map(|d| crate::keys_dialog::KeysDialog {
        waiting: Some(Action::Fill),
        ..d
    });
    a.key_down(Key::Escape, PLAIN, false, Window::detached());
    for _ in 0..4 {
        a.key_down(Key::Escape, PLAIN, true, Window::detached());
    }
    assert!(a.keys_dialog.is_some(), "the repeats do not close it");
}
