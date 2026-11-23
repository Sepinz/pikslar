//! A double click on a layer's row renames it; the header's tools duplicate and merge it.

use crate::app::App;
use crate::layer_naming::DOUBLE_CLICK;
use crate::panel_right::{ID_LAYER_ROW, ID_LAYER_TOOLS};
use crate::test_app::*;
use pikslar_core::StepCmd;
use pikslar_platform::{Button, Key, Mods};
use std::time::Instant;

fn centre_of(a: &mut App, id: u32) -> (i32, i32) {
    draw_whole(a);
    let (r, _) = *a
        .ui
        .widgets()
        .iter()
        .find(|&&(_, w)| w == id)
        .expect("the widget is shown");
    (r.x + r.w / 2, r.y + r.h / 2)
}

fn click_whole(a: &mut App, (x, y): (i32, i32)) {
    a.pointer_down(x, y, Button::Left, Mods::default());
    draw_whole(a);
    a.pointer_up(x, y, Button::Left);
    draw_whole(a);
}

fn double_click_row(a: &mut App, index: usize) {
    let at = centre_of(a, ID_LAYER_ROW + index as u32);
    click_whole(a, at);
    click_whole(a, at);
}

fn type_text(a: &mut App, text: &str) {
    text.chars().for_each(|c| a.text_typed(c));
    draw_whole(a);
}

fn name(a: &App, index: usize) -> &str {
    &a.editor.doc().layers()[index].name
}

#[test]
fn a_double_click_opens_the_name_and_enter_keeps_it_as_one_step() {
    let mut a = app(1);
    let (before, depth) = (name(&a, 0).to_string(), a.editor.history().undo_depth());
    double_click_row(&mut a, 0);
    assert!(a.layer_naming.is_some(), "the name is open for typing");
    type_text(&mut a, "Ink");
    assert_eq!(name(&a, 0), before, "nothing changes while typing");
    let tool = a.tool;
    key(&mut a, Key::Char(b'B'), Mods::default());
    assert_eq!(a.tool, tool, "letters are not shortcuts");
    key(&mut a, Key::Enter, Mods::default());
    assert!(a.layer_naming.is_none());
    assert_eq!(name(&a, 0), "Inkb");
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    a.run(StepCmd::Undo);
    assert_eq!(name(&a, 0), before);
}

#[test]
fn escape_and_an_empty_name_leave_the_name_as_it_was() {
    let mut a = app(1);
    let (before, depth) = (name(&a, 0).to_string(), a.editor.history().undo_depth());
    double_click_row(&mut a, 0);
    type_text(&mut a, "Gone");
    key(&mut a, Key::Escape, Mods::default());
    assert!(a.layer_naming.is_none());
    double_click_row(&mut a, 0);
    key(&mut a, Key::Backspace, Mods::default());
    key(&mut a, Key::Enter, Mods::default());
    assert_eq!(name(&a, 0), before);
    assert_eq!(a.editor.history().undo_depth(), depth);
}

#[test]
fn a_press_elsewhere_keeps_the_name_and_two_slow_clicks_do_not_open_it() {
    let mut a = app(1);
    double_click_row(&mut a, 0);
    type_text(&mut a, "Kept");
    let canvas = screen(&a, 3, 3);
    click(&mut a, canvas, Mods::default());
    assert_eq!(name(&a, 0), "Kept");
    assert!(
        a.layer_naming.is_none(),
        "the press ends naming, as it does a tag's"
    );

    let at = centre_of(&mut a, ID_LAYER_ROW);
    click_whole(&mut a, at);
    let long_ago = Instant::now().checked_sub(DOUBLE_CLICK * 2).expect("clock");
    a.last_row_click = a.last_row_click.map(|(row, _)| (row, long_ago));
    click_whole(&mut a, at);
    assert!(a.layer_naming.is_none());
}

#[test]
fn quick_clicks_on_two_rows_open_neither() {
    let mut a = app(1);
    a.layer_action(0);
    let (lower, upper) = (
        centre_of(&mut a, ID_LAYER_ROW),
        centre_of(&mut a, ID_LAYER_ROW + 1),
    );
    click_whole(&mut a, lower);
    click_whole(&mut a, upper);
    assert!(a.layer_naming.is_none());
    assert_eq!(a.editor.layer, 1);
}

#[test]
fn the_header_tools_duplicate_and_merge_the_active_layer() {
    let mut a = app(1);
    dot(&mut a, 4, 4);
    let duplicate = centre_of(&mut a, ID_LAYER_TOOLS + 4);
    click_whole(&mut a, duplicate);
    assert_eq!(a.editor.doc().layers().len(), 2);
    assert_eq!(a.editor.layer, 1);
    assert_eq!(name(&a, 1), format!("{} copy", name(&a, 0)));
    let (layers, cel) = (a.editor.doc().layers(), 0);
    assert!(layers[1].cel(cel).same_image(layers[0].cel(cel)));

    let merge = centre_of(&mut a, ID_LAYER_TOOLS + 5);
    click_whole(&mut a, merge);
    assert_eq!(a.editor.doc().layers().len(), 1);
    assert_eq!(a.editor.layer, 0);
    assert_ne!(a.editor.doc().layers()[0].cel(0).get(4, 4), 0);
}

#[test]
fn merging_a_hidden_layer_says_why_and_changes_nothing() {
    let mut a = app(1);
    a.layer_action(4);
    a.layer_action(4);
    let layer = a.editor.layer_id(1).expect("layer");
    a.run(pikslar_core::LayerCmd::SetVisible {
        layer,
        visible: false,
    });
    a.editor.layer = 2;
    let before = a.editor.doc().clone();
    a.layer_action(5);
    assert!(*a.editor.doc() == before);
    let said = a.message.as_ref().map(|(text, _)| text.as_str());
    assert_eq!(said, Some("Error: Show both layers to merge them"));
}
