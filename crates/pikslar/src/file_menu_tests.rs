//! The File and Edit menus' rows, shortcuts and states.

use super::*;
use crate::shortcuts::Binding;
use crate::test_app::*;
use pikslar_platform::Window;

fn names(entries: &Entries) -> Vec<String> {
    entries.iter().map(|(l, _)| l.to_string()).collect()
}

#[test]
fn the_file_menu_lists_new_open_recent_reference_save_and_export_with_live_keys() {
    let mut a = app(1);
    let rows = names(&a.file_entries());
    assert_eq!(rows[0], "New sprite...\tCtrl N");
    assert_eq!(rows[1], "Open a file...\tCtrl O");
    assert!(rows.contains(&"Save\tCtrl S".to_string()));
    assert!(rows.contains(&"Save as...\tCtrl Shift S".to_string()));
    assert!(rows.contains(&"Export...\tCtrl E".to_string()));
    assert!(rows.contains(&"Export sheet...\tCtrl Shift E".to_string()));
    let ctrl_k = Binding {
        letter: b'K',
        ctrl: true,
        shift: false,
    };
    a.keymap.bind(Action::Save, Some(ctrl_k));
    assert!(names(&a.file_entries()).contains(&"Save\tCtrl K".to_string()));
    a.keymap.bind(Action::Save, None);
    assert!(names(&a.file_entries()).contains(&"Save".to_string()));
}

#[test]
fn the_file_rows_do_what_their_shortcuts_do() {
    let mut a = app(1);
    let rows = a.file_entries();
    let at = |word: &str| {
        rows.iter()
            .position(|(l, _)| l.starts_with(word))
            .expect("row")
    };
    a.file_menu_action(at("New sprite"), Window::detached());
    assert!(a.new_dialog.is_some());
    a.close_new_dialog();
    a.file_menu_action(at("Export..."), Window::detached());
    assert!(a.export_dialog.is_some());
}

#[test]
fn undo_and_redo_rows_follow_the_history_and_selection_rows_the_selection() {
    let mut a = app(1);
    let state = |a: &App, word: &str| {
        a.edit_entries()
            .iter()
            .find(|(l, _)| l.starts_with(word))
            .map(|(_, e)| *e)
            .expect("row")
    };
    assert!(!state(&a, "Undo") && !state(&a, "Redo"));
    assert!(!state(&a, "Select none") && !state(&a, "Invert"));
    assert!(state(&a, "Select all") && state(&a, "Paste"));
    dot(&mut a, 3, 3);
    assert!(state(&a, "Undo"));
    select(&mut a);
    assert!(state(&a, "Select none") && state(&a, "Invert"));
    let undo = a
        .edit_entries()
        .iter()
        .position(|(l, _)| l.starts_with("Undo"))
        .expect("row");
    a.edit_menu_action(undo, Window::detached());
    assert!(state(&a, "Redo"));
}
