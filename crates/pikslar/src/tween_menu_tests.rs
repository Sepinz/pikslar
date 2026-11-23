//! Tween position as an artist reaches it: select a held run, right click it, choose
//! "Tween position", then an ease.

use crate::app::{App, ToolChoice};
use crate::frame_menu::MenuKind;
use crate::test_app::{app, click, dot, draw, range, ALT, PLAIN};
use crate::tween_menu::EASES;
use pikslar_core::FrameCmd;
use pikslar_platform::{Button, Key, Window};

/// Five frames holding one drawing, all at one place.
fn held_in_place() -> App {
    let mut a = app(5);
    dot(&mut a, 3, 3);
    let layer = a.editor.layer_id(0).expect("layer");
    a.editor
        .apply(FrameCmd::HoldAcross {
            layer,
            source: 0,
            from: 0,
            to: 4,
        })
        .expect("hold");
    draw(&mut a);
    a
}

/// Five frames holding one drawing, the last frame alone moved to (8, 0).
fn held_run() -> App {
    let mut a = held_in_place();
    let mut doc = a.editor.doc().clone();
    let last = doc.layers()[0].cel(4).moved_to(8, 0).expect("in range");
    doc.put_cel(0, 4, last).expect("frame");
    a.replace_document(doc, None);
    draw(&mut a);
    a
}

fn xs(a: &App) -> Vec<i16> {
    let layer = &a.editor.doc().layers()[0];
    layer.cels().iter().map(|c| c.position().0).collect()
}

fn right_click_frame(a: &mut App, frame: usize) {
    let at = (
        a.timeline.strip.x + frame as i32 * 34 + 10,
        a.timeline.strip.y + 5,
    );
    a.pointer_down(at.0, at.1, Button::Right, PLAIN);
    draw(a);
    a.pointer_up(at.0, at.1, Button::Right);
    draw(a);
}

/// The middle of item `n` of the open menu of `count` items.
fn item(a: &App, n: i32, count: i32) -> (i32, i32) {
    let open = a.menu.expect("a menu is open");
    let r = a.menu_area(open);
    let row = r.h / count;
    (r.x + r.w / 2, r.y + n * row + row / 2)
}

fn choose_tween(a: &mut App, ease: usize) {
    right_click_frame(a, 1);
    let at = item(a, 4, 5);
    click(a, at, PLAIN);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Tween));
    let at = item(a, ease as i32, EASES.len() as i32);
    click(a, at, PLAIN);
    assert_eq!(a.menu, None, "choosing an ease closes the menu");
}

#[test]
fn the_items_keep_the_names_scripts_find_them_by() {
    let labels: Vec<&str> = EASES.iter().map(|&(_, label)| label).collect();
    assert_eq!(labels, ["Linear", "Ease in", "Ease out", "Ease in and out"]);
    let a = held_run();
    let items = a.tween_items();
    assert!(items.iter().all(|i| i.enabled));
    assert_eq!(items.len(), 4);
}

#[test]
fn tween_position_then_ease_out_tweens_the_selected_run_as_one_step() {
    let mut a = held_run();
    range(&mut a, 0, 4);
    choose_tween(&mut a, 2);
    assert_eq!(xs(&a), [0, 4, 6, 8, 8]);
    assert_eq!(
        a.timeline.selected(),
        Some((0, 4)),
        "the run stays selected"
    );
    a.undo();
    assert_eq!(xs(&a), [0, 0, 0, 0, 8]);
    a.redo();
    assert_eq!(xs(&a), [0, 4, 6, 8, 8]);
}

#[test]
fn alt_with_the_arrows_moves_the_last_frame_alone_then_ease_out_tweens_toward_it() {
    let mut a = held_in_place();
    a.set_frame(4);
    a.set_tool(ToolChoice::Move);
    for _ in 0..8 {
        a.key_down(Key::Right, ALT, false, Window::detached());
    }
    draw(&mut a);
    assert_eq!(xs(&a), [0, 0, 0, 0, 8], "only the last frame moved");
    range(&mut a, 0, 4);
    choose_tween(&mut a, 2);
    assert_eq!(xs(&a), [0, 4, 6, 8, 8]);
}

#[test]
fn a_selection_past_the_last_frame_tweens_the_frames_still_there() {
    let mut a = held_run();
    range(&mut a, 0, 4);
    a.editor
        .apply(FrameCmd::Remove { from: 0, to: 0 })
        .expect("removed");
    draw(&mut a);
    choose_tween(&mut a, 0);
    assert_eq!(a.message, None, "nothing refused");
    let xs = xs(&a);
    assert_eq!((xs[0], xs[3]), (0, 8));
    assert!(xs[1] > 0 && xs[2] > xs[1], "{xs:?}");
}

#[test]
fn a_run_too_short_is_refused_in_the_status_line() {
    let mut a = held_run();
    range(&mut a, 0, 1);
    choose_tween(&mut a, 0);
    assert_eq!(xs(&a), [0, 0, 0, 0, 8], "nothing moved");
    let said = a.message.as_ref().map(|(text, _)| text.as_str());
    assert_eq!(said, Some("Error: Pick at least three frames"));
}
