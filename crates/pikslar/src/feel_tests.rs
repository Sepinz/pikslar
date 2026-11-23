//! Drawing and looking around: the brush outline, strokes, panning and scrubbing.

use crate::app::{App, Gesture, ToolChoice};
use crate::test_app::*;
use pikslar_core::SelectionCmd;
use pikslar_core::{ColorMode, Document, Mask, SelectOp, Tool};
use pikslar_platform::{Button, Key};
use pikslar_render::{Rect, Surface, View};
use std::time::{Duration, Instant};

fn over(a: &mut App, x: i32, y: i32) {
    let at = screen(a, x, y);
    a.ui.track(at.0, at.1);
}

/// The window as it stands after a frame, kept between frames as the real one is.
struct Screen(Vec<u32>);

impl Screen {
    fn new() -> Self {
        Screen(vec![0; (WINDOW.0 * WINDOW.1) as usize])
    }

    fn update(&mut self, a: &mut App) -> Vec<Rect> {
        let mut surface = Surface {
            px: &mut self.0,
            w: WINDOW.0,
            h: WINDOW.1,
        };
        let mut dirty = Vec::new();
        a.draw_canvas(&mut surface, &mut dirty);
        dirty
    }
}

#[test]
fn the_outline_is_as_big_as_the_brush() {
    let mut a = app(1);
    over(&mut a, 10, 10);
    let small = a.footprint_area();
    assert!(!small.is_empty());
    a.editor.brush_size = 5;
    assert!(a.footprint_area().w > small.w * 3);
}

#[test]
fn only_tools_that_use_the_brush_show_it() {
    let mut a = app(1);
    over(&mut a, 10, 10);
    for tool in [Tool::Pencil, Tool::Eraser, Tool::Line, Tool::Rect] {
        a.set_tool(ToolChoice::Paint(tool));
        assert!(!a.footprint_area().is_empty(), "{tool:?}");
    }
    for tool in [Tool::Fill, Tool::Picker] {
        a.set_tool(ToolChoice::Paint(tool));
        assert!(a.footprint_area().is_empty(), "{tool:?}");
    }
    a.set_tool(ToolChoice::Marquee);
    assert!(a.footprint_area().is_empty());
}

#[test]
fn the_outline_stays_out_of_the_way() {
    let mut a = app(1);
    over(&mut a, 10, 10);
    assert!(!a.footprint_area().is_empty());
    a.space_held = true;
    assert!(a.footprint_area().is_empty(), "while panning");
    a.space_held = false;
    a.open_new_dialog();
    assert!(a.footprint_area().is_empty(), "behind a dialog");
    a.close_new_dialog();
    a.footprint.pointer_away = true;
    assert!(a.footprint_area().is_empty(), "after leaving the window");
    a.footprint.pointer_away = false;
    let at = (a.timeline.strip.x + 5, a.timeline.strip.y + 5);
    a.ui.track(at.0, at.1);
    assert!(a.footprint_area().is_empty(), "over a panel");
}

#[test]
fn the_outline_is_hidden_during_a_stroke() {
    let mut a = app(1);
    let at = screen(&a, 10, 10);
    a.ui.track(at.0, at.1);
    a.pointer_down(at.0, at.1, Button::Left, PLAIN);
    assert!(a.footprint_area().is_empty());
}

#[test]
fn off_the_document_there_is_no_outline() {
    let mut a = app(1);
    a.view = a.view.zoomed(-4, 0, 0);
    let c = a.layout.canvas;
    a.ui.track(c.right() - 2, c.bottom() - 2);
    let (x, y) = a.view.to_doc(c.right() - 2, c.bottom() - 2);
    assert!(x >= 32 || y >= 32, "the test needs a point off the sprite");
    assert!(a.footprint_area().is_empty());
}

#[test]
fn the_outline_leaves_nothing_behind_when_it_moves() {
    let mut a = app(1);
    a.editor.brush_size = 5;
    dot(&mut a, 8, 8);
    let mut screen_now = Screen::new();
    a.ui.track(0, 0);
    a.full_canvas = true;
    screen_now.update(&mut a);
    let clean = screen_now.0.clone();

    over(&mut a, 8, 8);
    let dirty = screen_now.update(&mut a);
    assert_ne!(screen_now.0, clean, "the outline is on screen");
    let area = a.footprint_area();
    assert!(
        dirty.iter().any(|r| r.intersect(area) == area),
        "the window is told to repaint it"
    );

    a.ui.track(0, 0);
    screen_now.update(&mut a);
    assert_eq!(screen_now.0, clean, "gone, with the art intact");
}

#[test]
fn an_unmoving_outline_costs_nothing() {
    let mut a = app(1);
    let mut screen_now = Screen::new();
    over(&mut a, 8, 8);
    screen_now.update(&mut a);
    assert!(screen_now.update(&mut a).is_empty());
}

#[test]
fn stepping_during_a_stroke_does_not_end_it() {
    let mut a = app(3);
    let at = screen(&a, 5, 5);
    a.pointer_down(at.0, at.1, Button::Left, PLAIN);
    key(&mut a, Key::Period, PLAIN);
    assert_eq!(a.editor.frame, 0);
    key(&mut a, Key::Enter, PLAIN);
    assert!(!a.playing);
    assert!(a.editor.is_stroking());
    key(&mut a, Key::Char(b'E'), PLAIN);
    assert!(a.editor.is_stroking(), "changing tool would end it too");
}

#[test]
fn escape_still_cancels_a_stroke() {
    let mut a = app(1);
    let at = screen(&a, 5, 5);
    a.pointer_down(at.0, at.1, Button::Left, PLAIN);
    key(&mut a, Key::Escape, PLAIN);
    assert!(!a.editor.is_stroking());
}

#[test]
fn panning_does_not_stop_playback() {
    let mut a = app(3);
    a.toggle_play();
    a.space_held = true;
    let at = screen(&a, 5, 5);
    a.pointer_down(at.0, at.1, Button::Left, PLAIN);
    assert!(a.playing);
}

#[test]
fn panning_survives_the_next_playback_frame() {
    let mut a = app(3);
    a.toggle_play();
    a.space_held = true;
    let at = screen(&a, 5, 5);
    a.pointer_down(at.0, at.1, Button::Left, PLAIN);
    a.next_frame_at = Instant::now() - Duration::from_millis(1);
    a.advance_playback();
    assert_eq!(a.editor.frame, 1, "the animation moved on");
    assert!(
        matches!(a.gesture, Gesture::Pan { .. }),
        "and so did the pan"
    );
}

#[test]
fn zoom_keys_wait_for_the_stroke_to_end() {
    let mut a = app(1);
    let before = (a.view.num, a.view.den);
    let at = screen(&a, 5, 5);
    a.pointer_down(at.0, at.1, Button::Left, PLAIN);
    key(&mut a, Key::Plus, PLAIN);
    key(&mut a, Key::Minus, PLAIN);
    key(&mut a, Key::Minus, PLAIN);
    assert_eq!(
        (a.view.num, a.view.den),
        before,
        "a zoom would add a jump to the stroke"
    );
}

#[test]
fn an_outline_bigger_than_the_canvas_still_follows() {
    let mut a = app(1);
    // Its square covers the whole canvas, while the round edge still crosses the corners.
    a.editor.brush_size = 31;
    let c = a.layout.canvas;
    a.view = View {
        num: 32,
        den: 1,
        x: c.x,
        y: c.y,
    };
    a.full_canvas = true;
    let (x, y) = (c.x + c.w / 2, c.y + c.h / 2);
    let mut now = Screen::new();
    a.ui.track(x, y);
    now.update(&mut a);
    let before = now.0.clone();
    let area = a.footprint_area();
    a.ui.track(x + 32, y + 32);
    now.update(&mut a);
    assert_eq!(
        a.footprint_area(),
        area,
        "the test needs an area that cannot change"
    );
    assert_ne!(now.0, before);
}

#[test]
fn dragging_a_panel_control_over_the_canvas_shows_no_outline() {
    let mut a = app(3);
    let (x, y) = (a.timeline.strip.x + 10, a.timeline.strip.y + 5);
    a.pointer_down(x, y, Button::Left, PLAIN);
    draw(&mut a);
    over(&mut a, 10, 10);
    assert!(a.footprint_area().is_empty());
}

#[test]
fn a_smaller_window_keeps_the_outline_off_the_panels() {
    let mut a = app(1);
    a.editor.brush_size = 16;
    let c = a.layout.canvas;
    a.view = View {
        num: 8,
        den: 1,
        x: c.right() - 8 * 32 - 2,
        y: c.y + 4,
    };
    a.full_canvas = true;
    let mut now = Screen::new();
    a.ui.track(c.right() - 80, c.y + 120);
    now.update(&mut a);
    assert!(!a.footprint_area().is_empty());
    let before = now.0.clone();
    a.resized(WINDOW.0 - 200, WINDOW.1, a.ui.scale);
    let edge = a.layout.canvas.right() as usize;
    now.update(&mut a);
    let width = WINDOW.0 as usize;
    let beyond = |px: &[u32]| -> Vec<u32> {
        px.chunks(width)
            .flat_map(|row| row[edge..].to_vec())
            .collect()
    };
    assert!(beyond(&now.0) == beyond(&before), "drawn over the panel");
}

/// What part of the canvas looks like once a large selection is being previewed.
fn beside_a_drag_preview(hover: bool) -> Vec<u32> {
    let mut a = app(1);
    a.replace_document(
        Document::new(600, 600, ColorMode::Rgba).expect("valid"),
        None,
    );
    a.editor.brush_size = 3;
    a.editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(0, 0, 600, 600)),
            op: SelectOp::Replace,
        })
        .expect("select");
    let c = a.layout.canvas;
    a.view = View {
        num: 1,
        den: 1,
        x: c.x,
        y: c.y,
    };
    a.full_canvas = true;
    let mut now = Screen::new();
    a.ui.track(0, 0);
    now.update(&mut a);
    key(&mut a, Key::Char(b'R'), PLAIN);
    now.update(&mut a);
    if hover {
        a.ui.track(c.x + 200, c.y + 200);
        now.update(&mut a);
    }
    key(&mut a, Key::Left, PLAIN);
    now.update(&mut a);
    assert!(a.preview.is_some(), "the test needs a live preview");
    if hover {
        a.ui.track(c.x + 300, c.y + 300);
        now.update(&mut a);
    }
    let (left, right) = ((c.x + 150) as usize, (c.x + 350) as usize);
    now.0
        .chunks(WINDOW.0 as usize)
        .skip((c.y + 150) as usize)
        .take(200)
        .flat_map(|row| row[left..right].to_vec())
        .collect()
}

#[test]
fn the_outline_leaves_no_trail_over_a_drag_preview() {
    assert!(beside_a_drag_preview(true) == beside_a_drag_preview(false));
}

#[test]
fn drawing_still_stops_playback() {
    let mut a = app(3);
    a.toggle_play();
    let at = screen(&a, 5, 5);
    a.pointer_down(at.0, at.1, Button::Left, PLAIN);
    assert!(!a.playing);
}

#[test]
fn scrubbing_past_the_end_of_the_strip_keeps_going() {
    let mut a = app(80);
    let (x, y) = (a.timeline.strip.x + 10, a.timeline.strip.y + 5);
    a.pointer_down(x, y, Button::Left, PLAIN);
    draw(&mut a);
    let past = a.timeline.strip.right() + 20;
    for _ in 0..80 {
        a.ui.track(past, y);
        a.timeline.next_edge_step = None;
        draw(&mut a);
    }
    assert_eq!(a.editor.frame, 79, "reached the end and stopped");
    let before = a.timeline.strip.x - 20;
    for _ in 0..80 {
        a.ui.track(before, y);
        a.timeline.next_edge_step = None;
        draw(&mut a);
    }
    assert_eq!(a.editor.frame, 0, "and back");
}

#[test]
fn scrubbing_past_the_end_goes_at_a_steady_pace() {
    let mut a = app(80);
    let (x, y) = (a.timeline.strip.x + 10, a.timeline.strip.y + 5);
    a.pointer_down(x, y, Button::Left, PLAIN);
    draw(&mut a);
    let past = a.timeline.strip.right() + 20;
    a.ui.track(past, y);
    draw(&mut a);
    let frame = a.editor.frame;
    for _ in 0..30 {
        a.ui.track(past + 1, y);
        draw(&mut a);
    }
    assert!(
        a.editor.frame <= frame + 1,
        "mouse speed must not set the pace"
    );
}
