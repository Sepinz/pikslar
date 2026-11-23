//! The Canvas size dialog, driven as a person would: typed sides, an anchor, Enter.

use super::CanvasDialog;
use crate::test_app::*;
use pikslar_platform::Key;

/// Keys pressed as Windows sends them: the key, then its character.
fn typed(a: &mut crate::app::App, text: &str) {
    for c in text.bytes() {
        key(a, Key::Char(c.to_ascii_uppercase()), PLAIN);
    }
}

/// One screen update of the canvas.
fn canvas(a: &mut crate::app::App) {
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    let mut surface = pikslar_render::Surface {
        px: &mut px,
        w: WINDOW.0,
        h: WINDOW.1,
    };
    a.draw_canvas(&mut surface, &mut Vec::new());
}

fn press(a: &mut crate::app::App, k: Key) {
    key(a, k, PLAIN);
}

#[test]
fn typed_sides_and_an_anchor_resize_the_canvas_as_one_step() {
    let mut a = app(1);
    let (w, h) = (a.editor.doc().width, a.editor.doc().height);
    a.open_canvas_dialog();
    for _ in 0..4 {
        press(&mut a, Key::Backspace);
    }
    typed(&mut a, "100");
    // Tab follows the focus order of the last full draw, which the test keys do not make.
    draw_whole(&mut a);
    // The test keys redraw only the bottom panel, which drops the dialog's focus; a real
    // frame draws the dialog every time.
    a.ui.focus.set(50_000);
    tap(&mut a, Key::Tab, PLAIN);
    for _ in 0..4 {
        press(&mut a, Key::Backspace);
    }
    typed(&mut a, "4x0");
    a.canvas_dialog.as_mut().expect("open").anchor = (2, 0);
    press(&mut a, Key::Enter);
    assert!(a.canvas_dialog.is_none(), "closed once done");
    assert_eq!((a.editor.doc().width, a.editor.doc().height), (100, 40));
    let depth = a.editor.history().undo_depth();
    assert!(depth >= 1);
    a.run(pikslar_core::StepCmd::Undo);
    assert_eq!((a.editor.doc().width, a.editor.doc().height), (w, h));
}

#[test]
fn blending_switches_as_one_step_and_undo_brings_it_back() {
    use pikslar_core::Blending;
    let mut a = app(1);
    a.open_canvas_dialog();
    canvas(&mut a);
    let pushes = a.editor.history().pushes();
    a.toggle_blending();
    assert_eq!(a.editor.doc().blending, Blending::Aseprite);
    assert_eq!(a.editor.history().pushes(), pushes + 1);
    assert!(a.canvas_dialog.is_some(), "the dialog stays open");
    canvas(&mut a);
    a.run(pikslar_core::StepCmd::Undo);
    assert_eq!(a.editor.doc().blending, Blending::Pikslar);
    a.run(pikslar_core::StepCmd::Redo);
    assert_eq!(a.editor.doc().blending, Blending::Aseprite);
}

#[test]
fn a_side_out_of_range_says_so_and_keeps_the_dialog() {
    let mut a = app(1);
    a.open_canvas_dialog();
    for _ in 0..4 {
        press(&mut a, Key::Backspace);
    }
    typed(&mut a, "9999");
    press(&mut a, Key::Enter);
    assert!(a.canvas_dialog.is_some());
    let said = a.message.as_ref().map_or("", |(text, _)| text.as_str());
    assert!(said.contains("8192"), "{said}");
}

#[test]
fn a_side_takes_digits_only_and_at_most_four() {
    let mut d = CanvasDialog {
        width: String::new(),
        height: "1".into(),
        on_height: false,
        anchor: (1, 1),
    };
    for c in "a1-2345".chars() {
        d.typed(c);
    }
    assert_eq!(d.width, "1234");
    assert_eq!(d.size(), Some((1234, 1)));
    d.height.clear();
    assert_eq!(d.size(), None, "an empty side");
}

#[test]
fn escape_closes_and_changes_nothing() {
    let mut a = app(1);
    let before = a.editor.doc().clone();
    a.open_canvas_dialog();
    typed(&mut a, "7");
    press(&mut a, Key::Escape);
    assert!(a.canvas_dialog.is_none());
    assert_eq!(*a.editor.doc(), before);
}

#[test]
fn the_screen_follows_a_new_canvas_and_its_undo() {
    let mut a = app(1);
    canvas(&mut a);
    a.run(pikslar_core::command::CanvasCmd::Resize {
        width: 20,
        height: 12,
        dx: 0,
        dy: 0,
    });
    canvas(&mut a);
    assert_eq!((a.comp.width(), a.comp.height()), (20, 12));
    a.run(pikslar_core::StepCmd::Undo);
    canvas(&mut a);
    let doc = a.editor.doc();
    assert_eq!((a.comp.width(), a.comp.height()), (doc.width, doc.height));
}

/// A key as the window delivers it, then the whole interface drawn, which builds the focus order.
fn tap(a: &mut crate::app::App, k: Key, mods: pikslar_platform::Mods) {
    a.key_down(k, mods, false, pikslar_platform::Window::detached());
    draw_whole(a);
}

fn opened() -> crate::app::App {
    let mut a = app(1);
    a.open_canvas_dialog();
    draw_whole(&mut a);
    a
}

#[test]
fn the_dialog_opens_on_the_width_and_tab_walks_every_control() {
    let mut a = opened();
    assert_eq!(a.ui.focus.focused(), Some(50_000));
    let mut seen = vec![50_000];
    for _ in 0..14 {
        tap(&mut a, Key::Tab, PLAIN);
        seen.push(a.ui.focus.focused().expect("focused"));
    }
    let expected = [
        50_000, 50_001, 50_002, 50_003, 50_004, 50_005, 50_006, 50_007, 50_008, 50_009, 50_010,
        50_014, 50_011, 50_013, 50_000,
    ];
    assert_eq!(&seen[..], &expected[..]);
    tap(&mut a, Key::Tab, SHIFT);
    assert_eq!(a.ui.focus.focused(), Some(50_013), "Shift Tab goes back");
}

#[test]
fn tab_to_the_height_sends_the_digits_there() {
    let mut a = opened();
    tap(&mut a, Key::Tab, PLAIN);
    assert!(a.canvas_dialog.as_ref().expect("open").on_height);
    for _ in 0..4 {
        tap(&mut a, Key::Backspace, PLAIN);
    }
    a.canvas_dialog_typed('9');
    assert_eq!(a.canvas_dialog.as_ref().expect("open").height, "9");
    tap(&mut a, Key::Tab, SHIFT);
    assert!(!a.canvas_dialog.as_ref().expect("open").on_height);
}

#[test]
fn arrows_move_over_the_anchor_grid_and_space_picks_a_place() {
    let mut a = opened();
    a.ui.focus.set(50_006);
    tap(&mut a, Key::Right, PLAIN);
    assert_eq!(a.ui.focus.focused(), Some(50_007));
    tap(&mut a, Key::Space, PLAIN);
    assert_eq!(a.canvas_dialog.as_ref().expect("open").anchor, (2, 1));
    tap(&mut a, Key::Up, PLAIN);
    assert_eq!(a.ui.focus.focused(), Some(50_004), "top row");
    tap(&mut a, Key::Up, PLAIN);
    assert_eq!(a.ui.focus.focused(), Some(50_001), "up into the fields");
    a.ui.focus.set(50_010);
    tap(&mut a, Key::Down, PLAIN);
    assert_eq!(a.ui.focus.focused(), Some(50_014), "Blending");
    tap(&mut a, Key::Down, PLAIN);
    assert_eq!(
        a.ui.focus.focused(),
        Some(50_011),
        "Resize, in the same column"
    );
}

#[test]
fn enter_toggles_blending_resizes_or_cancels_by_what_is_focused() {
    let mut a = opened();
    let before = a.editor.doc().blending;
    a.ui.focus.set(50_014);
    tap(&mut a, Key::Enter, PLAIN);
    assert!(a.canvas_dialog.is_some(), "Blending keeps the dialog open");
    assert_ne!(a.editor.doc().blending, before);

    a.ui.focus.set(50_013);
    tap(&mut a, Key::Enter, PLAIN);
    assert!(a.canvas_dialog.is_none(), "Cancel closed it");

    let mut a = opened();
    for _ in 0..4 {
        tap(&mut a, Key::Backspace, PLAIN);
    }
    a.canvas_dialog_typed('2');
    a.canvas_dialog_typed('0');
    a.ui.focus.set(50_011);
    tap(&mut a, Key::Enter, PLAIN);
    assert!(a.canvas_dialog.is_none());
    assert_eq!(a.editor.doc().width, 20);

    let mut a = opened();
    tap(&mut a, Key::Escape, PLAIN);
    assert!(a.canvas_dialog.is_none(), "Escape still cancels");
}

#[test]
fn the_focused_anchor_is_outlined() {
    let mut a = opened();
    a.ui.focus.set(50_006);
    let (w, h) = WINDOW;
    let mut px = vec![0u32; (w * h) as usize];
    a.draw_interface(
        &mut pikslar_render::Surface { px: &mut px, w, h },
        pikslar_platform::Window::detached(),
        &mut Vec::new(),
    );
    let r =
        a.ui.widgets()
            .iter()
            .find(|(_, id)| *id == 50_006)
            .map(|(r, _)| *r)
            .expect("anchor");
    assert_eq!(
        px[((r.y + 5) * w + r.x - 2) as usize],
        pikslar_ui::theme::focus()
    );
}

#[test]
fn delete_clears_the_side_being_typed_so_a_new_number_replaces_it() {
    let mut a = app(1);
    a.open_canvas_dialog();
    press(&mut a, Key::Delete);
    typed(&mut a, "48");
    let d = a.canvas_dialog.as_ref().expect("open");
    assert_eq!(d.width, "48", "the old width is gone, not appended to");
    assert_ne!(d.height, "", "the other side is kept");
}
