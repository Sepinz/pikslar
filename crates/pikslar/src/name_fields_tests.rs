//! The keys the name fields share: word moves, and Ctrl+C, X and V staying inside the name.
//! None of these reach the real clipboard: the test window has none.

use crate::test_app::*;
use pikslar_platform::{Key, Mods};

const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
const CTRL_SHIFT: Mods = Mods {
    ctrl: true,
    shift: true,
    alt: false,
};

fn naming_layer(text: &str) -> crate::app::App {
    let mut a = app(1);
    a.start_layer_naming(0);
    text.chars().for_each(|c| a.text_typed(c));
    a
}

fn field(a: &crate::app::App) -> &pikslar_ui::TextEdit {
    &a.layer_naming.as_ref().expect("naming").edit
}

#[test]
fn ctrl_arrows_move_by_words_and_ctrl_shift_selects_them() {
    let mut a = naming_layer("one two");
    key(&mut a, Key::Left, CTRL);
    assert_eq!(field(&a).caret(), 4);
    key(&mut a, Key::Left, CTRL_SHIFT);
    assert_eq!(field(&a).selection(), (0, 4));
    key(&mut a, Key::Right, CTRL);
    assert_eq!(
        field(&a).caret(),
        4,
        "a move without Shift ends the selection"
    );
    assert_eq!(field(&a).selection(), (4, 4));
    assert_eq!(field(&a).text(), "one two");
}

#[test]
fn clipboard_chords_stay_in_the_name_and_never_copy_or_paste_the_image() {
    let mut a = naming_layer("walk");
    let (tool, depth) = (a.tool, a.editor.history().undo_depth());
    key(&mut a, Key::Char(b'C'), CTRL);
    assert!(a.message.is_none(), "nothing selected, nothing to copy");
    key(&mut a, Key::Char(b'V'), CTRL);
    let said = a.message.as_ref().map(|(text, _)| text.as_str());
    assert_eq!(
        said,
        Some("The clipboard was busy"),
        "not the image's message"
    );
    key(&mut a, Key::Char(b'X'), CTRL);
    assert_eq!(field(&a).text(), "walk");
    assert_eq!((a.tool, a.editor.history().undo_depth()), (tool, depth));
    assert!(!a.editor.is_floating());
}

#[test]
fn the_other_fields_use_the_same_keys() {
    let mut a = app(1);
    a.start_naming(0, true);
    a.text_typed('a');
    a.text_typed(' ');
    a.text_typed('b');
    key(&mut a, Key::Left, CTRL);
    assert_eq!(a.timeline.name_edit.caret(), 2);
    key(&mut a, Key::Escape, Mods::default());
}

#[test]
fn a_huge_paste_into_a_nearly_full_name_fills_it_and_says_so_once() {
    let mut a = naming_layer(&"a".repeat(pikslar_core::command::MAX_LAYER_NAME - 3));
    let huge = "b".repeat(70_000);
    within(5, "pasting a huge text", || a.insert_pasted_text(&huge));
    let max = pikslar_core::command::MAX_LAYER_NAME;
    assert_eq!(field(&a).text().chars().count(), max);
    assert_eq!(field(&a).text().chars().filter(|&c| c == 'b').count(), 3);
    assert_eq!(said(&a), "The pasted text was cut to fit");
}

#[test]
fn a_paste_that_fits_says_nothing_and_a_selection_makes_room() {
    let mut a = naming_layer("walk");
    a.insert_pasted_text(" cycle");
    assert_eq!(field(&a).text(), "walk cycle");
    assert_eq!(said(&a), "");
    key(&mut a, Key::Char(b'A'), CTRL);
    a.insert_pasted_text("run");
    assert_eq!(field(&a).text(), "run");
}

#[test]
fn a_busy_clipboard_says_so_and_an_empty_one_says_there_is_no_text() {
    use pikslar_platform::TextPaste;
    let mut a = naming_layer("walk");
    a.report_paste(TextPaste::Busy);
    assert_eq!(said(&a), "The clipboard was busy");
    a.report_paste(TextPaste::NoText);
    assert_eq!(said(&a), "There is no text to paste");
    a.report_paste(TextPaste::Text("!".into()));
    assert_eq!(field(&a).text(), "walk!");
}

#[test]
fn alt_arrows_still_move_a_name_s_caret() {
    let mut a = naming_layer("one two");
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    key(&mut a, Key::Left, alt);
    assert_eq!(field(&a).caret(), 6);
    key(&mut a, Key::Home, alt);
    assert_eq!(field(&a).caret(), 0);
}
