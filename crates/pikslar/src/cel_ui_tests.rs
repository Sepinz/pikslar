//! Moving and turning whole cels through the application's own entry points.

use crate::app::{App, ToolChoice};
use crate::test_app::*;
use pikslar_core::{CelCmd, OFFSET_LIMIT};
use pikslar_platform::{Button, Handler, Key, Mods, Window};
use pikslar_render::{Rect, Surface, CEL_EPOCH};

const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};

fn position(a: &App) -> (i16, i16) {
    a.editor.doc().layers()[a.editor.layer]
        .cel(a.editor.frame)
        .position()
}

/// Every pixel the active cel keeps, on the canvas and past it, in canvas positions.
fn kept(a: &App) -> Vec<((i32, i32), u32)> {
    let cel = a.editor.doc().layers()[a.editor.layer].cel(a.editor.frame);
    let (cx, cy) = cel.position();
    let (x0, y0, tw, th) = cel.image().table();
    let mut out = Vec::new();
    for y in y0 * 16..(y0 + th as i32) * 16 {
        for x in x0 * 16..(x0 + tw as i32) * 16 {
            let v = cel.image().get(x, y);
            if v != 0 {
                out.push(((x + cx as i32, y + cy as i32), v));
            }
        }
    }
    out
}

/// Drags between two canvas positions.
fn drag(a: &mut App, from: (i32, i32), to: (i32, i32), mods: Mods) {
    let (from, to) = (screen(a, from.0, from.1), screen(a, to.0, to.1));
    a.pointer_down(from.0, from.1, Button::Left, mods);
    a.canvas_move(to.0, to.1);
    draw(a);
    a.pointer_up(to.0, to.1, Button::Left);
    draw(a);
}

fn with_dots() -> App {
    let mut a = app(1);
    dot(&mut a, 5, 5);
    dot(&mut a, 30, 2);
    a.set_tool(ToolChoice::Move);
    a
}

#[test]
fn alt_with_the_move_tool_moves_only_this_cel_of_its_hold() {
    let mut a = app(3);
    dot(&mut a, 5, 5);
    let layer = a.editor.layer_id(0).expect("layer");
    let hold = pikslar_core::FrameCmd::HoldAcross {
        layer,
        source: 0,
        from: 0,
        to: 2,
    };
    a.editor.apply(hold).expect("hold");
    a.set_frame(1);
    a.set_tool(ToolChoice::Move);
    let depth = a.editor.history().undo_depth();
    drag(&mut a, (5, 5), (9, 7), ALT);
    let at = |a: &App, f: usize| a.editor.doc().layers()[0].cel(f).position();
    assert_eq!([at(&a, 0), at(&a, 1), at(&a, 2)], [(0, 0), (4, 2), (0, 0)]);
    assert_eq!(a.editor.history().undo_depth(), depth + 1, "one step");
    drag(&mut a, (9, 7), (10, 7), PLAIN);
    assert_eq!([at(&a, 0), at(&a, 1), at(&a, 2)], [(1, 0), (5, 2), (1, 0)]);
    assert_eq!(a.editor.history().undo_depth(), depth + 2);
}

#[test]
fn a_drag_with_the_move_tool_moves_the_whole_cel_as_one_step() {
    let mut a = with_dots();
    let before = kept(&a);
    let depth = a.editor.history().undo_depth();
    drag(&mut a, (20, 20), (26, 17), PLAIN);
    assert_eq!(position(&a), (6, -3));
    assert!(!a.editor.is_floating() && !a.has_selection());
    assert_eq!(a.editor.history().undo_depth(), depth + 1, "one step");
    let moved: Vec<_> = before
        .iter()
        .map(|&((x, y), v)| ((x + 6, y - 3), v))
        .collect();
    assert_eq!(kept(&a), moved, "the dot moved past the right edge is kept");
    a.undo();
    assert_eq!(kept(&a), before);
}

#[test]
fn dragging_past_the_edge_and_back_gives_back_the_picture() {
    let mut a = with_dots();
    let before = kept(&a);
    drag(&mut a, (20, 20), (0, 20), PLAIN);
    assert_eq!(position(&a), (-20, 0));
    drag(&mut a, (0, 20), (20, 20), PLAIN);
    assert_eq!(position(&a), (0, 0));
    assert_eq!(kept(&a), before);
}

#[test]
fn escape_during_a_cel_drag_puts_it_back_and_leaves_no_step() {
    let mut a = with_dots();
    let depth = a.editor.history().undo_depth();
    let (from, to) = (screen(&a, 20, 20), screen(&a, 10, 25));
    a.pointer_down(from.0, from.1, Button::Left, PLAIN);
    a.canvas_move(to.0, to.1);
    assert_eq!(position(&a), (-10, 5));
    key(&mut a, Key::Escape, PLAIN);
    a.pointer_up(to.0, to.1, Button::Left);
    assert_eq!(position(&a), (0, 0));
    assert_eq!(a.editor.history().undo_depth(), depth);
    assert!(!a.editor.history().can_redo(), "nothing left to redo");
}

#[test]
fn ctrl_drag_with_the_move_tool_still_picks_up_a_floating_copy() {
    let mut a = with_dots();
    drag(&mut a, (20, 20), (22, 20), CTRL);
    assert!(a.editor.is_floating());
    assert_eq!(a.editor.selection().map(|m| m.bounds().0), Some(7));
    assert_eq!(position(&a), (0, 0), "the cel stays");
}

#[test]
fn a_drag_on_an_empty_frame_does_nothing() {
    let mut a = app(1);
    a.set_tool(ToolChoice::Move);
    drag(&mut a, (20, 20), (25, 20), PLAIN);
    assert_eq!(position(&a), (0, 0));
    assert!(!a.editor.history().can_undo());
}

#[test]
fn arrows_on_an_empty_frame_move_nothing_and_say_so() {
    let mut a = app(1);
    a.set_tool(ToolChoice::Move);
    key(&mut a, Key::Right, PLAIN);
    assert_eq!(position(&a), (0, 0), "a blank cel keeps its place");
    assert!(!a.editor.history().can_undo(), "and no step is recorded");
    let said = a.message.as_ref().map_or("", |(text, _)| text.as_str());
    assert!(said.contains("nothing"), "{said}");
}

#[test]
fn arrows_move_the_cel_with_the_move_tool_and_a_held_key_is_one_step() {
    let mut a = with_dots();
    a.new_frame(false);
    a.set_frame(0);
    let depth = a.editor.history().undo_depth();
    key(&mut a, Key::Right, PLAIN);
    key(&mut a, Key::Down, SHIFT);
    assert_eq!(position(&a), (1, 10));
    assert_eq!(a.editor.frame, 0, "the arrows no longer walk the frames");
    assert_eq!(
        a.editor.history().undo_depth(),
        depth + 2,
        "one step a press"
    );
    for _ in 0..5 {
        a.key_down(Key::Left, PLAIN, true, Window::detached());
    }
    assert_eq!(position(&a), (-4, 10));
    assert_eq!(
        a.editor.history().undo_depth(),
        depth + 2,
        "a held key joins"
    );
    a.set_tool(ToolChoice::Marquee);
    key(&mut a, Key::Right, PLAIN);
    assert_eq!(a.editor.frame, 1, "other tools still walk the frames");
}

#[test]
fn a_move_stopped_at_the_range_limit_says_so() {
    let mut a = with_dots();
    let layer = a.editor.layer_id(0).expect("layer");
    a.editor
        .apply(CelCmd::Move {
            layer,
            frame: 0,
            dx: OFFSET_LIMIT - 3,
            dy: 0,
        })
        .expect("move");
    key(&mut a, Key::Right, SHIFT);
    assert_eq!(position(&a).0 as i32, OFFSET_LIMIT);
    let said = a.message.as_ref().map_or("", |(text, _)| text.as_str());
    assert!(said.contains("as far"), "{said}");
}

#[test]
fn flip_and_rotate_without_a_selection_turn_the_whole_drawing() {
    let mut a = with_dots();
    drag(&mut a, (20, 20), (25, 20), PLAIN);
    let before = kept(&a);
    let depth = a.editor.history().undo_depth();
    key(&mut a, Key::Char(b'H'), SHIFT);
    let flipped: Vec<_> = {
        let mut v: Vec<_> = before.iter().map(|&((x, y), c)| ((31 - x, y), c)).collect();
        v.sort();
        v
    };
    let mut now = kept(&a);
    now.sort();
    assert_eq!(now, flipped, "the dot past the edge flips too");
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    for _ in 0..4 {
        key(&mut a, Key::Char(b'R'), PLAIN);
    }
    let mut now = kept(&a);
    now.sort();
    assert_eq!(now, flipped, "four quarter turns");
    key(&mut a, Key::Char(b'R'), SHIFT);
    key(&mut a, Key::Char(b'R'), PLAIN);
    let mut now = kept(&a);
    now.sort();
    assert_eq!(now, flipped);
}

/// The window's pixels, kept between frames as a real window keeps them.
fn paint(a: &mut App, px: &mut [u32]) {
    let mut surface = Surface {
        px,
        w: WINDOW.0,
        h: WINDOW.1,
    };
    a.frame(&mut surface, Window::detached(), &mut Vec::new());
}

fn window_pixels(a: &mut App) -> Vec<u32> {
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    paint(a, &mut px);
    px
}

#[test]
fn the_outline_shows_how_far_the_drawing_reaches_past_the_canvas() {
    let mut a = with_dots();
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    paint(&mut a, &mut px);
    assert_eq!(a.off_canvas.shown, None, "everything is on the canvas");
    // The outline's right side will be here, in the margin past the canvas edge.
    let cell = a.view.num / a.view.den;
    let side = (
        (a.view.x + 41 * cell) as usize,
        (a.view.y + 3 * cell + cell / 2) as usize,
    );
    let at = |px: &[u32]| px[side.1 * WINDOW.0 as usize + side.0];
    let before = at(&px);
    a.key_down(Key::Right, SHIFT, false, Window::detached());
    paint(&mut a, &mut px);
    // The dots at (5, 5) and (30, 2) are now at (15, 5) and (40, 2).
    assert_eq!(a.off_canvas.shown, Some(Rect::new(15, 2, 26, 4)));
    assert_ne!(at(&px), before, "the outline is drawn past the canvas");
    a.undo();
    paint(&mut a, &mut px);
    assert_eq!(a.off_canvas.shown, None);
    assert_eq!(at(&px), before, "and goes again");
}

fn canvas_pixels(a: &mut App) -> Vec<u32> {
    let px = window_pixels(a);
    let c = a.layout.canvas;
    let mut out = Vec::new();
    for y in c.y..c.bottom() {
        let row = (y * WINDOW.0) as usize;
        out.extend_from_slice(&px[row + c.x as usize..row + c.right() as usize]);
    }
    out
}

#[test]
fn a_large_cel_drags_through_a_preview_that_shows_what_the_canvas_will() {
    let mut a = App::launch(None, Some(600), None);
    a.resized(WINDOW.0, WINDOW.1, 1);
    for (x, y) in [(5, 5), (300, 300), (599, 10), (10, 599)] {
        dot(&mut a, x, y);
    }
    a.set_tool(ToolChoice::Move);
    canvas_pixels(&mut a);
    let (from, to) = (screen(&a, 300, 300), screen(&a, 230, 340));
    a.pointer_down(from.0, from.1, Button::Left, PLAIN);
    a.canvas_move(to.0, to.1);
    let previewed = canvas_pixels(&mut a);
    assert!(a.preview.as_ref().is_some_and(|p| p.epoch() == CEL_EPOCH));
    a.pointer_up(to.0, to.1, Button::Left);
    let settled = canvas_pixels(&mut a);
    assert!(a.preview.is_none());
    assert!(
        previewed == settled,
        "the preview matches the composited canvas"
    );
}

#[test]
fn a_drag_follows_the_pointer_and_does_not_drift_at_the_range_limit() {
    let mut a = with_dots();
    let layer = a.editor.layer_id(0).expect("layer");
    a.editor
        .apply(CelCmd::Move {
            layer,
            frame: 0,
            dx: OFFSET_LIMIT - 4,
            dy: 0,
        })
        .expect("move");
    let start = position(&a);
    let press = screen(&a, 10, 10);
    a.pointer_down(press.0, press.1, Button::Left, PLAIN);
    for (x, y, want) in [
        (12, 11, (start.0 + 2, start.1 + 1)),
        (20, 11, (OFFSET_LIMIT as i16, start.1 + 1)),
        (25, 9, (OFFSET_LIMIT as i16, start.1 - 1)),
        (10, 10, start),
        (7, 13, (start.0 - 3, start.1 + 3)),
    ] {
        let at = screen(&a, x, y);
        a.canvas_move(at.0, at.1);
        assert_eq!(position(&a), want, "pointer at {x},{y}");
    }
    a.pointer_up(press.0, press.1, Button::Left);
}

#[test]
fn an_arrow_move_and_a_drag_after_it_are_two_steps() {
    let mut a = with_dots();
    let depth = a.editor.history().undo_depth();
    key(&mut a, Key::Right, PLAIN);
    drag(&mut a, (10, 10), (13, 10), PLAIN);
    key(&mut a, Key::Left, PLAIN);
    assert_eq!(position(&a), (3, 0));
    assert_eq!(a.editor.history().undo_depth(), depth + 3);
    a.undo();
    assert_eq!(position(&a), (4, 0));
    a.undo();
    assert_eq!(position(&a), (1, 0));
}
