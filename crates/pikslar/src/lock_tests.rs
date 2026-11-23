//! The lock mark in a layer row: it locks the layer, a tool then says so and draws nothing.

use crate::panel_right::ID_LAYER_LOCK;
use crate::test_app::*;
use pikslar_platform::{Button, Mods};

fn lock_mark(a: &mut crate::app::App) -> (i32, i32) {
    draw_whole(a);
    let id = ID_LAYER_LOCK + a.editor.layer as u32;
    let (r, _) = *a
        .ui
        .widgets()
        .iter()
        .find(|&&(_, w)| w == id)
        .expect("the active layer's lock mark is shown");
    (r.x + r.w / 2, r.y + r.h / 2)
}

/// A click with the whole window drawn, so the side panels see it.
fn click_whole(a: &mut crate::app::App, (x, y): (i32, i32)) {
    a.pointer_down(x, y, Button::Left, Mods::default());
    draw_whole(a);
    a.pointer_up(x, y, Button::Left);
    draw_whole(a);
}

#[test]
fn the_lock_mark_locks_the_layer_and_a_tool_on_it_says_so() {
    let mut a = app(1);
    let at = lock_mark(&mut a);
    click_whole(&mut a, at);
    assert!(a.editor.doc().layers()[a.editor.layer].locked);
    let (before, revision) = (a.editor.doc().clone(), a.editor.revision());
    let on_canvas = screen(&a, 5, 5);
    click(&mut a, on_canvas, Mods::default());
    assert!(*a.editor.doc() == before, "nothing was drawn");
    assert_eq!(a.editor.revision(), revision);
    let said = a.message.as_ref().map(|(text, _)| text.as_str());
    assert_eq!(said, Some("Error: This layer is locked"));

    let at = lock_mark(&mut a);
    click_whole(&mut a, at);
    assert!(!a.editor.doc().layers()[a.editor.layer].locked, "unlocked");
    click(&mut a, on_canvas, Mods::default());
    assert!(
        a.editor.doc().layers()[a.editor.layer].cel(0).get(5, 5) != 0,
        "drawn"
    );
}
