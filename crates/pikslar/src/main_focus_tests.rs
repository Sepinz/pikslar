//! Tab reaches every control of the main window, Enter and Space use the one with focus, and
//! a ring shows which it is.

use crate::app::{App, ToolChoice};
use crate::panel_side::ID_TOOL;
use crate::test_app::*;
use pikslar_core::Tool;
use pikslar_platform::{Key, Mods, Window};

fn tap(a: &mut App, key: Key, mods: Mods) {
    a.key_down(key, mods, false, Window::detached());
    draw_whole(a);
}

/// Tabs until `id` has focus, failing after a full round.
fn tab_to(a: &mut App, id: u32) {
    let stops = a.ui.focus.order().len();
    for _ in 0..=stops {
        if a.ui.focus.focused() == Some(id) {
            return;
        }
        tap(a, Key::Tab, PLAIN);
    }
    panic!("{id} is not reached by Tab among {stops} stops");
}

#[test]
fn tab_reaches_the_buttons_of_every_panel_and_not_only_the_sliders() {
    let mut a = app(2);
    draw_whole(&mut a);
    let order = a.ui.focus.order().to_vec();
    assert!(order.contains(&ID_TOOL), "a tool button");
    assert!(order.contains(&(ID_TOOL + 1)), "the eraser");
    assert!(
        order.contains(&crate::panel_right::ID_COLOR),
        "a colour slider"
    );
    assert!(
        order.contains(&crate::panel_right::ID_LAYER_TOOLS),
        "the layer buttons"
    );
    assert!(
        order.contains(&crate::panel_bottom::ID_TRANSPORT),
        "the frames"
    );
    let first_tool = order.iter().position(|&id| id == ID_TOOL);
    let first_layer = order
        .iter()
        .position(|&id| id == crate::panel_right::ID_LAYER_TOOLS);
    assert!(first_tool < first_layer, "left panel before the right one");
}

#[test]
fn enter_and_space_use_the_focused_button() {
    let mut a = app(2);
    draw_whole(&mut a);
    tab_to(&mut a, ID_TOOL + 1);
    tap(&mut a, Key::Enter, PLAIN);
    assert_eq!(a.tool, ToolChoice::Paint(Tool::Eraser));
    assert!(!a.playing, "Enter did not start playback");
    tab_to(&mut a, ID_TOOL);
    tap(&mut a, Key::Space, PLAIN);
    assert_eq!(a.tool, ToolChoice::Paint(Tool::Pencil));
    assert!(!a.space_held, "Space did not start panning");
}

#[test]
fn the_arrows_move_focus_between_buttons_and_leave_the_frames_alone() {
    let mut a = app(3);
    draw_whole(&mut a);
    tab_to(&mut a, ID_TOOL);
    tap(&mut a, Key::Right, PLAIN);
    let moved = a.ui.focus.focused();
    assert!(moved.is_some() && moved != Some(ID_TOOL), "{moved:?}");
    assert_eq!(a.editor.frame, 0);
    tap(&mut a, Key::Escape, PLAIN);
    assert_eq!(a.ui.focus.focused(), None);
    tap(&mut a, Key::Right, PLAIN);
    assert_eq!(
        a.editor.frame, 1,
        "with no focus the arrows walk the frames"
    );
}

#[test]
fn a_focused_control_without_a_ring_of_its_own_gets_one() {
    let mut a = app(2);
    draw_whole(&mut a);
    let row = crate::panel_right::ID_LAYER_ROW;
    let r =
        a.ui.widgets()
            .iter()
            .find(|(_, id)| *id == row)
            .map(|(r, _)| *r)
            .expect("a layer row");
    tab_to(&mut a, row);
    let (w, h) = WINDOW;
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = pikslar_render::Surface { px: &mut px, w, h };
    a.draw_interface(&mut surface, Window::detached(), &mut Vec::new());
    let at = |x: i32, y: i32| px[(y * w + x) as usize];
    assert_eq!(at(r.x, r.y + r.h / 2), pikslar_ui::theme::focus());
    assert_eq!(at(r.right() - 1, r.y + r.h / 2), pikslar_ui::theme::focus());
}
