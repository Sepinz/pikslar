//! Shortcut hints follow the live keymap.

use crate::app::App;
use crate::shortcuts::{Action, Binding};
use crate::test_app::*;

fn layout_line(a: &App, id: u32) -> String {
    let text = a.ui.layout().expect("names are kept");
    text.lines()
        .find(|l| l.split('\t').next() == Some(&id.to_string()))
        .map(|l| l.split('\t').nth(5).unwrap_or_default().to_string())
        .unwrap_or_default()
}

fn named_app() -> App {
    let mut a = app(1);
    a.ui.record_names();
    a
}

fn plain(letter: u8) -> Option<Binding> {
    Some(Binding {
        letter,
        ctrl: false,
        shift: false,
    })
}

fn ctrl(letter: u8) -> Option<Binding> {
    Some(Binding {
        letter,
        ctrl: true,
        shift: false,
    })
}

/// What the status line says with the pointer resting on widget `id`.
fn hint_of(a: &mut App, id: u32) -> String {
    draw_whole(a);
    let r =
        a.ui.widgets()
            .iter()
            .find(|(_, w)| *w == id)
            .map(|(r, _)| *r)
            .expect("widget");
    a.ui.mouse = (r.x + r.w / 2, r.y + r.h / 2);
    draw_whole(a);
    a.hint.as_deref().unwrap_or_default().to_string()
}

#[test]
fn rebinding_the_pencil_changes_its_hint() {
    let mut a = named_app();
    draw_whole(&mut a);
    assert_eq!(layout_line(&a, 120), "Pencil  B");
    a.keymap.bind(Action::Pencil, plain(b'P'));
    draw_whole(&mut a);
    assert_eq!(layout_line(&a, 120), "Pencil  P");
}

#[test]
fn an_unbound_action_leaves_the_key_out_of_its_hint() {
    let mut a = named_app();
    a.keymap.bind(Action::Pencil, None);
    draw_whole(&mut a);
    assert_eq!(layout_line(&a, 120), "Pencil");
}

#[test]
fn file_and_frame_hints_follow_the_keymap_and_fixed_keys_stay() {
    let mut a = named_app();
    a.keymap.bind(Action::New, ctrl(b'W'));
    a.keymap.bind(Action::Save, ctrl(b'H'));
    let rows: Vec<String> = a
        .file_entries()
        .iter()
        .map(|(l, _)| l.to_string())
        .collect();
    assert_eq!(rows[0], "New sprite...	Ctrl W");
    assert!(rows.contains(&"Save	Ctrl H".to_string()), "{rows:?}");
    a.keymap.bind(Action::DuplicateFrame, plain(b'Q'));
    assert_eq!(hint_of(&mut a, 100), "New, open, save and export");
    assert_eq!(hint_of(&mut a, 304), "Duplicate frame  Q");
    assert_eq!(hint_of(&mut a, 305), "Remove frame  Shift Delete");
}

#[test]
fn the_text_tools_hints_follow_the_keymap() {
    let mut a = named_app();
    a.keymap.bind(Action::Text, plain(b'Y'));
    assert_eq!(hint_of(&mut a, 126), "Text  Y  Enter keeps, Esc cancels");
}
