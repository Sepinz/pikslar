//! Moving large selections: when the preview runs, and that the art ends up right.

use crate::app::{App, Gesture, ToolChoice};
use pikslar_core::Tool;
use pikslar_core::{DrawCmd, FloatingCmd, SelectionCmd};
use pikslar_core::{Mask, SelectOp};
use pikslar_platform::{Button, Mods};
use pikslar_render::{Rect, Surface};

const WINDOW: (i32, i32) = (800, 600);
const LARGE: usize = 512;
const SMALL: usize = 32;
const INK: (i32, i32) = (200, 200);
/// Screen distance of a drag, at least a cell at any zoom these tests use.
const STEP: i32 = 60;

fn app(size: usize) -> App {
    let mut app = App::launch(None, Some(size), None);
    app.resized(WINDOW.0, WINDOW.1, 1);
    app.editor
        .apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x: INK.0,
            y: INK.1,
        })
        .expect("begin");
    app.editor.apply(DrawCmd::Commit).expect("commit");
    app.select_change(SelectionCmd::All);
    app.set_tool(ToolChoice::Move);
    frame(&mut app);
    app
}

/// Runs what the app runs for one screen update of the canvas.
fn frame(a: &mut App) -> Vec<Rect> {
    let mut px = vec![0u32; (WINDOW.0 * WINDOW.1) as usize];
    let mut surface = Surface {
        px: &mut px,
        w: WINDOW.0,
        h: WINDOW.1,
    };
    let mut dirty = Vec::new();
    a.sync_selection();
    a.draw_canvas(&mut surface, &mut dirty);
    dirty
}

fn inside(a: &App) -> (i32, i32) {
    let c = a.layout.canvas;
    (c.x + c.w / 2, c.y + c.h / 2)
}

fn grab(a: &mut App) -> (i32, i32) {
    let at = inside(a);
    a.pointer_down(at.0, at.1, Button::Left, Mods::default());
    a.canvas_move(at.0 + STEP, at.1);
    assert!(matches!(a.gesture, Gesture::Move { .. }));
    at
}

#[test]
fn a_large_drag_previews_and_leaves_the_document_alone() {
    let mut a = app(LARGE);
    grab(&mut a);
    frame(&mut a);
    assert!(a.preview.is_some());
    assert!(a.editor.floating_is_stale(), "no pixel work while dragging");
    assert_eq!(
        a.editor.color_at(INK.0, INK.1) >> 24,
        255,
        "art not moved yet"
    );
}

#[test]
fn a_small_drag_goes_through_the_compositor() {
    let mut a = app(SMALL);
    grab(&mut a);
    frame(&mut a);
    assert!(a.preview.is_none());
    assert!(!a.editor.floating_is_stale());
}

#[test]
fn applying_after_a_preview_lands_the_art_and_is_one_undo_step() {
    let mut a = app(LARGE);
    let depth = a.editor.history().undo_depth();
    let at = grab(&mut a);
    frame(&mut a);
    a.pointer_up(at.0 + STEP, at.1, Button::Left);
    frame(&mut a);
    assert!(
        a.preview.is_some(),
        "stays cheap until something needs the pixels"
    );
    let (cells, _) = (
        a.view.to_doc(at.0 + STEP, at.1).0 - a.view.to_doc(at.0, at.1).0,
        0,
    );
    a.settle();
    frame(&mut a);
    assert!(a.preview.is_none());
    assert!(
        !a.full_canvas,
        "the frame after a preview repaints the whole canvas"
    );
    assert_eq!(a.editor.color_at(INK.0 + cells, INK.1) >> 24, 255);
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    a.undo();
    assert_eq!(a.editor.color_at(INK.0, INK.1) >> 24, 255);
}

#[test]
fn anything_else_that_changes_the_picture_ends_the_preview() {
    let mut a = app(LARGE);
    grab(&mut a);
    frame(&mut a);
    a.toggle_onion();
    let dirty = frame(&mut a);
    assert!(a.preview.is_none());
    assert!(!a.editor.floating_is_stale(), "the document has caught up");
    assert_eq!(dirty.first().copied(), Some(a.layout.canvas));
}

#[test]
fn zooming_during_a_drag_rebuilds_the_preview_for_the_new_view() {
    let mut a = app(LARGE);
    let at = grab(&mut a);
    frame(&mut a);
    a.zoom(1, at.0, at.1);
    frame(&mut a);
    assert!(
        a.editor.floating_is_stale(),
        "no document-sized work for a zoom"
    );
    a.canvas_move(at.0 + 40, at.1);
    frame(&mut a);
    let epoch = a.editor.floating_view().map_or(0, |f| f.epoch);
    assert!(a
        .preview
        .as_ref()
        .is_some_and(|p| p.is_for(epoch, a.view, a.layout.canvas)));
}

#[test]
fn escape_during_a_previewed_drag_puts_everything_back() {
    let mut a = app(LARGE);
    let bounds = a.editor.selection().map(|m| m.bounds());
    grab(&mut a);
    frame(&mut a);
    a.cancel_gesture();
    frame(&mut a);
    assert_eq!(a.editor.selection().map(|m| m.bounds()), bounds);
    assert!(!a.editor.is_floating());
    assert!(a.preview.is_none());
    assert_eq!(a.editor.color_at(INK.0, INK.1) >> 24, 255);
}

#[test]
fn the_outline_follows_a_move_without_being_recomputed() {
    let mut a = app(LARGE);
    let shape = a.editor.rect_shape((100, 100), (420, 420));
    a.select_change(SelectionCmd::Select {
        shape: Box::new(shape),
        op: pikslar_core::SelectOp::Replace,
    });
    frame(&mut a);
    a.editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    a.editor
        .apply(FloatingCmd::Move {
            dx: 7,
            dy: -3,
            join: true,
        })
        .expect("move");
    a.editor
        .apply(FloatingCmd::Move {
            dx: -2,
            dy: 5,
            join: true,
        })
        .expect("move");
    a.sync_selection();
    let expected = a.editor.selection().map(|m| m.outline());
    assert_eq!(Some(a.outline.clone()), expected);
}

#[test]
fn the_outline_is_recomputed_when_the_shape_changes() {
    let mut a = app(LARGE);
    frame(&mut a);
    let shape = a.editor.rect_shape((10, 10), (20, 30));
    a.select_change(SelectionCmd::Select {
        shape: Box::new(shape),
        op: pikslar_core::SelectOp::Replace,
    });
    a.sync_selection();
    assert_eq!(
        Some(a.outline.clone()),
        a.editor.selection().map(|m| m.outline())
    );
}

#[test]
fn a_selection_replaced_while_moved_is_outlined_correctly() {
    let mut a = app(LARGE);
    a.editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    a.editor
        .apply(FloatingCmd::Move {
            dx: 3,
            dy: 3,
            join: true,
        })
        .expect("move");
    a.sync_selection();
    a.editor.apply(FloatingCmd::Revert).expect("revert");
    a.sync_selection();
    assert_eq!(
        Some(a.outline.clone()),
        a.editor.selection().map(|m| m.outline())
    );
}

#[test]
fn a_paste_replacing_a_previewed_selection_gets_its_own_preview() {
    let mut a = app(LARGE);
    grab(&mut a);
    frame(&mut a);
    let clip = a.editor.copy_selected().expect("selected");
    a.editor.apply(FloatingCmd::Anchor).expect("anchor");
    a.editor.take_damage();
    a.editor
        .apply(FloatingCmd::Paste {
            clip: std::sync::Arc::new(clip.clone()),
            x: 0,
            y: 0,
        })
        .expect("fits");
    a.editor
        .apply(FloatingCmd::Move {
            dx: 1,
            dy: 1,
            join: true,
        })
        .expect("move");
    frame(&mut a);
    let epoch = a.editor.floating_view().map(|f| f.epoch);
    assert!(a.preview.as_ref().is_none_or(|p| Some(p.epoch()) == epoch));
}

fn handle_point_of(a: &App, handle: crate::select_ui::Handle) -> (i32, i32) {
    let r = a.handles_rect().expect("handles");
    crate::select_ui::handle_point(r, handle)
}

fn press_handle(a: &mut App, handle: crate::select_ui::Handle) -> (i32, i32) {
    frame(a);
    let at = handle_point_of(a, handle);
    a.pointer_down(at.0, at.1, Button::Left, Mods::default());
    assert!(matches!(a.gesture, Gesture::Scale { .. }));
    at
}

#[test]
fn nudging_a_large_selection_previews_it() {
    let mut a = app(LARGE);
    a.nudge(3, 0, false);
    frame(&mut a);
    assert!(a.preview.is_some());
    assert!(a.editor.floating_is_stale(), "no pixel work for a nudge");
    a.nudge(1, 1, false);
    frame(&mut a);
    assert!(
        a.preview.is_some(),
        "the same preview serves the next nudge"
    );
    a.settle();
    assert_eq!(a.editor.color_at(INK.0 + 4, INK.1 + 1) >> 24, 255);
    assert_eq!(a.editor.color_at(INK.0, INK.1), 0);
}

#[test]
fn nudging_a_small_selection_goes_through_the_compositor() {
    let mut a = app(SMALL);
    a.nudge(3, 0, false);
    frame(&mut a);
    assert!(a.preview.is_none());
}

#[test]
fn scaling_a_large_selection_leaves_the_pixels_alone_until_release() {
    let mut a = app(LARGE);
    let depth = a.editor.history().undo_depth();
    let bounds = a.editor.selection().map(|m| m.bounds());
    let at = press_handle(&mut a, (1, 1));
    a.canvas_move(at.0 - 90, at.1 - 60);
    assert!(a.scale_target.is_some());
    frame(&mut a);
    assert!(a.preview.is_some());
    assert_eq!(
        a.editor.selection().map(|m| m.bounds()),
        bounds,
        "nothing scaled yet"
    );
    let shown = a.shown_bounds();
    assert_ne!(shown, bounds, "but it looks scaled");
    a.pointer_up(at.0 - 90, at.1 - 60, Button::Left);
    assert!(a.scale_target.is_none());
    assert_eq!(a.editor.selection().map(|m| m.bounds()), shown);
    a.settle();
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    a.undo();
    assert_eq!(a.editor.color_at(INK.0, INK.1) >> 24, 255);
}

#[test]
fn escape_during_a_large_scale_changes_nothing() {
    let mut a = app(LARGE);
    let bounds = a.editor.selection().map(|m| m.bounds());
    let at = press_handle(&mut a, (1, 1));
    a.canvas_move(at.0 - 90, at.1 - 60);
    frame(&mut a);
    a.cancel_gesture();
    frame(&mut a);
    assert!(a.scale_target.is_none());
    assert!(a.preview.is_none());
    assert_eq!(a.editor.selection().map(|m| m.bounds()), bounds);
    assert!(!a.editor.is_floating());
    assert_eq!(a.editor.color_at(INK.0, INK.1) >> 24, 255);
}

#[test]
fn escape_during_a_large_scale_of_moved_pixels_keeps_the_move() {
    let mut a = app(LARGE);
    a.nudge(4, 4, false);
    frame(&mut a);
    let bounds = a.editor.selection().map(|m| m.bounds());
    let at = press_handle(&mut a, (1, 1));
    a.canvas_move(at.0 - 70, at.1);
    frame(&mut a);
    let version = a.editor.selection_version();
    a.cancel_gesture();
    assert_eq!(a.editor.selection_version(), version, "no pixel work");
    assert!(a.editor.is_floating());
    assert_eq!(a.editor.selection().map(|m| m.bounds()), bounds);
}

#[test]
fn a_small_scale_still_applies_as_it_goes() {
    let mut a = app(SMALL);
    let at = press_handle(&mut a, (1, 1));
    a.canvas_move(at.0 - 90, at.1 - 60);
    assert!(a.scale_target.is_none());
    assert!(a.editor.selection().is_some_and(|m| m.bounds().2 < 32));
    a.pointer_up(at.0 - 90, at.1 - 60, Button::Left);
}

#[test]
fn the_outline_of_a_previewed_scale_matches_the_applied_one() {
    let mut a = app(LARGE);
    let at = press_handle(&mut a, (1, 1));
    a.canvas_move(at.0 + 40, at.1 + 25);
    a.sync_selection();
    a.sync_scale_outline();
    let previewed = a.scale_outline.clone();
    a.pointer_up(at.0 + 40, at.1 + 25, Button::Left);
    a.sync_selection();
    assert_eq!(previewed, a.outline);
}

#[test]
fn saving_during_a_large_scale_uses_the_size_shown() {
    let mut a = app(LARGE);
    let at = press_handle(&mut a, (1, 1));
    a.canvas_move(at.0 - 90, at.1 - 60);
    let shown = a.shown_bounds();
    a.settle();
    assert_eq!(a.gesture, Gesture::None);
    assert!(a.scale_target.is_none());
    let within = shown.map(|(x, y, w, h)| (x, y, w.min(512), h.min(512)));
    assert_eq!(a.editor.selection().map(|m| m.bounds()), within);
}

#[test]
fn a_large_scale_previews_even_after_the_pixels_had_been_placed() {
    let mut a = app(LARGE);
    a.nudge(1, 0, false);
    frame(&mut a);
    let _ = a.editor.take_damage();
    assert!(!a.editor.floating_is_stale());
    let bounds = a.editor.selection().map(|m| m.bounds());
    let at = press_handle(&mut a, (1, 1));
    a.canvas_move(at.0 - 90, at.1 - 60);
    frame(&mut a);
    assert!(a.preview.is_some());
    assert_eq!(a.editor.selection().map(|m| m.bounds()), bounds);
}

#[test]
fn opening_another_document_forgets_a_scale_in_progress() {
    let mut a = app(LARGE);
    let at = press_handle(&mut a, (1, 1));
    a.canvas_move(at.0 - 90, at.1 - 60);
    assert!(a.scale_target.is_some());
    let doc = a.editor.doc().clone();
    a.replace_document(doc, None);
    assert!(a.scale_target.is_none());
}

#[test]
fn escape_restores_a_scale_that_grew_into_a_preview_part_way() {
    let mut a = app(LARGE);
    a.select_change(SelectionCmd::Select {
        shape: Box::new(Mask::rect(150, 150, 120, 120)),
        op: SelectOp::Replace,
    });
    a.nudge(1, 1, false);
    frame(&mut a);
    let bounds = a.editor.selection().map(|m| m.bounds());
    let at = press_handle(&mut a, (1, 1));
    a.canvas_move(at.0 + 30, at.1 + 30);
    assert!(a.scale_target.is_none(), "still small, applied as it goes");
    assert_ne!(a.editor.selection().map(|m| m.bounds()), bounds);
    a.canvas_move(at.0 + 6000, at.1 + 6000);
    assert!(a.scale_target.is_some(), "large enough to preview");
    a.sync_selection();
    a.sync_scale_outline();
    let (x, y, w, h) = a.scale_target.expect("target");
    let right = a
        .scale_outline
        .iter()
        .map(|e| e.x + if e.horizontal { e.len } else { 0 });
    let bottom = a
        .scale_outline
        .iter()
        .map(|e| e.y + if e.horizontal { 0 } else { e.len });
    assert_eq!(right.max(), Some(x + w));
    assert_eq!(bottom.max(), Some(y + h));
    a.cancel_gesture();
    assert_eq!(a.editor.selection().map(|m| m.bounds()), bounds);
}

#[test]
fn an_arrow_key_during_a_scale_finishes_it_first() {
    let mut a = app(LARGE);
    let at = press_handle(&mut a, (1, 1));
    a.canvas_move(at.0 - 90, at.1 - 60);
    let (x, y, w, h) = a.shown_bounds().expect("selected");
    a.arrow((4, 0), pikslar_platform::Mods::default(), false);
    assert_eq!(a.gesture, Gesture::None);
    assert_eq!(a.shown_bounds(), Some((x + 4, y, w, h)));
    assert!(a.scale_target.is_none());
}

#[test]
fn zooming_during_a_scale_keeps_showing_the_size_being_dragged() {
    let mut a = app(LARGE);
    let at = press_handle(&mut a, (1, 1));
    a.canvas_move(at.0 - 90, at.1 - 60);
    frame(&mut a);
    a.zoom(1, 200, 200);
    a.canvas_move(at.0 - 91, at.1 - 60);
    frame(&mut a);
    assert!(a.preview.is_some(), "rebuilt, never composited");
    assert!(a
        .editor
        .selection()
        .is_some_and(|m| m.bounds().2 == LARGE as i32));
}
