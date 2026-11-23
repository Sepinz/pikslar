//! Tests for drawing, undo and palette edits on the editor.

use super::*;
use crate::color::rgba;
use crate::document::{ColorMode, Document};
use crate::history::History;
use crate::{PaletteCmd, StepCmd};
use std::sync::Arc;

const RED: u32 = rgba(255, 0, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);

fn editor(mode: ColorMode) -> Editor {
    let mut e = Editor::new(Document::new(40, 40, mode).expect("valid"));
    e.color = RED;
    e.pixel_perfect = false;
    e
}

fn stroke(e: &mut Editor, tool: Tool, pts: &[(i32, i32)]) {
    e.begin(tool, pts[0].0, pts[0].1).expect("begin");
    for p in &pts[1..] {
        e.drag(p.0, p.1);
    }
    e.commit();
}

fn both_modes() -> [ColorMode; 2] {
    [ColorMode::Palette, ColorMode::Rgba]
}

#[test]
fn pencil_paints_in_both_modes() {
    for mode in both_modes() {
        let mut e = editor(mode);
        stroke(&mut e, Tool::Pencil, &[(1, 1), (5, 1)]);
        assert_eq!(e.color_at(3, 1), RED, "{mode:?}");
        assert_eq!(e.color_at(3, 2), 0);
    }
}

#[test]
fn nothing_commits_until_the_stroke_ends() {
    let mut e = editor(ColorMode::Rgba);
    e.begin(Tool::Pencil, 2, 2).expect("begin");
    assert_eq!(e.color_at(2, 2), 0);
    e.commit();
    assert_eq!(e.color_at(2, 2), RED);
}

#[test]
fn cancel_leaves_the_document_alone() {
    let mut e = editor(ColorMode::Rgba);
    e.begin(Tool::Pencil, 2, 2).expect("begin");
    e.cancel();
    e.commit();
    assert_eq!(e.color_at(2, 2), 0);
    assert!(!e.history.can_undo());
}

#[test]
fn undo_and_redo_restore_exact_pixels() {
    for mode in both_modes() {
        let mut e = editor(mode);
        stroke(&mut e, Tool::Pencil, &[(0, 0), (30, 30)]);
        let painted = e.doc.layers[0].cel(0).to_cells(e.doc.width, e.doc.height);
        assert!(e.undo());
        assert!(e.doc.layers[0]
            .cel(0)
            .to_cells(e.doc.width, e.doc.height)
            .iter()
            .all(|&p| p == 0));
        assert!(e.redo());
        assert_eq!(
            e.doc.layers[0].cel(0).to_cells(e.doc.width, e.doc.height),
            painted
        );
    }
}

#[test]
fn a_stroke_that_changes_nothing_makes_no_undo_step() {
    let mut e = editor(ColorMode::Rgba);
    stroke(&mut e, Tool::Eraser, &[(3, 3)]);
    assert!(!e.history.can_undo());
}

#[test]
fn eraser_clears_in_both_modes() {
    for mode in both_modes() {
        let mut e = editor(mode);
        stroke(&mut e, Tool::Pencil, &[(4, 4)]);
        stroke(&mut e, Tool::Eraser, &[(4, 4)]);
        assert_eq!(e.color_at(4, 4), 0);
    }
}

#[test]
fn fill_stops_at_a_wall() {
    for mode in both_modes() {
        let mut e = editor(mode);
        stroke(&mut e, Tool::Line, &[(10, 0), (10, 39)]);
        e.color = BLUE;
        stroke(&mut e, Tool::Fill, &[(2, 2)]);
        assert_eq!(e.color_at(9, 39), BLUE);
        assert_eq!(e.color_at(10, 5), RED);
        assert_eq!(e.color_at(11, 5), 0);
    }
}

#[test]
fn fill_outside_the_canvas_is_harmless() {
    let mut e = editor(ColorMode::Rgba);
    stroke(&mut e, Tool::Fill, &[(-3, 900)]);
    assert!(!e.history.can_undo());
}

#[test]
fn line_preview_replaces_itself() {
    let mut e = editor(ColorMode::Rgba);
    stroke(&mut e, Tool::Line, &[(0, 0), (20, 0), (0, 20)]);
    assert_eq!(e.color_at(20, 0), 0);
    assert_eq!(e.color_at(0, 20), RED);
}

#[test]
fn rect_draws_an_outline() {
    let mut e = editor(ColorMode::Rgba);
    stroke(&mut e, Tool::Rect, &[(2, 2), (8, 6)]);
    assert_eq!(e.color_at(8, 6), RED);
    assert_eq!(e.color_at(5, 4), 0);
}

#[test]
fn pixel_perfect_removes_corners() {
    let mut e = editor(ColorMode::Rgba);
    e.pixel_perfect = true;
    stroke(&mut e, Tool::Pencil, &[(5, 5), (6, 5), (6, 6)]);
    assert_eq!(e.color_at(5, 5), RED);
    assert_eq!(e.color_at(6, 5), 0);
    assert_eq!(e.color_at(6, 6), RED);
}

#[test]
fn picker_reads_the_active_layer_in_both_modes() {
    for mode in both_modes() {
        let mut e = editor(mode);
        stroke(&mut e, Tool::Pencil, &[(1, 1)]);
        e.color = BLUE;
        stroke(&mut e, Tool::Picker, &[(1, 1)]);
        assert_eq!(e.color, RED);
    }
}

#[test]
fn recoloring_the_palette_updates_palette_layers_only() {
    let mut e = editor(ColorMode::Palette);
    e.transact(|d| d.add_layer(1, ColorMode::Rgba))
        .expect("room");
    e.layer = 0;
    stroke(&mut e, Tool::Pencil, &[(1, 1)]);
    e.layer = 1;
    stroke(&mut e, Tool::Pencil, &[(2, 2)]);
    let id = e.doc.layers[0].cel(0).get(1, 1);
    e.transact(|d| {
        Arc::make_mut(&mut d.colors).set_color(id, BLUE);
        Ok(())
    })
    .expect("ok");
    e.layer = 0;
    assert_eq!(e.color_at(1, 1), BLUE);
    e.layer = 1;
    assert_eq!(e.color_at(2, 2), RED);
}

#[test]
fn structural_changes_undo() {
    let mut e = editor(ColorMode::Rgba);
    e.transact(|d| d.add_frame(1, false)).expect("room");
    assert_eq!(e.doc.frames.len(), 2);
    e.undo();
    assert_eq!(e.doc.frames.len(), 1);
}

#[test]
fn undo_does_not_disturb_a_shared_frame() {
    let mut e = editor(ColorMode::Rgba);
    stroke(&mut e, Tool::Pencil, &[(1, 1)]);
    e.transact(|d| d.add_frame(1, true)).expect("room");
    e.frame = 1;
    stroke(&mut e, Tool::Pencil, &[(9, 9)]);
    assert_eq!(
        e.doc.layers[0].cel(0).get(9, 9),
        0,
        "editing frame 1 must not change frame 0"
    );
}

#[test]
fn a_recolor_drag_is_one_undo_step() {
    let mut e = editor(ColorMode::Palette);
    stroke(&mut e, Tool::Pencil, &[(1, 1)]);
    let id = e.doc.layers[0].cel(0).get(1, 1);
    let depth = e.history.undo_depth();
    e.apply(StepCmd::EndDrag).expect("end");
    e.apply(PaletteCmd::Recolor { id, color: BLUE })
        .expect("recolor");
    e.apply(PaletteCmd::Recolor {
        id,
        color: rgba(0, 255, 0, 255),
    })
    .expect("recolor");
    assert_eq!(e.history.undo_depth(), depth + 1);
    e.undo();
    assert_eq!(e.color_at(1, 1), RED);
}

#[test]
fn scrubbing_reaches_any_point_in_history() {
    let mut e = editor(ColorMode::Rgba);
    for i in 0..5 {
        stroke(&mut e, Tool::Pencil, &[(i, 0)]);
    }
    e.scrub_to(2);
    assert_eq!((e.color_at(1, 0), e.color_at(2, 0)), (RED, 0));
    e.scrub_to(5);
    assert_eq!(e.color_at(4, 0), RED);
}

#[test]
fn wide_brush_paints_a_round_stamp() {
    let mut e = editor(ColorMode::Rgba);
    e.brush_size = 5;
    stroke(&mut e, Tool::Pencil, &[(10, 10)]);
    assert_eq!(e.color_at(12, 10), RED);
    assert_eq!(e.color_at(8, 8), 0, "corners stay empty");
}

#[test]
fn budget_drops_the_oldest_steps() {
    let mut e = editor(ColorMode::Rgba);
    e.history = History::with_budget(4_000);
    for i in 0..40 {
        stroke(&mut e, Tool::Pencil, &[(i, 0), (i, 39)]);
    }
    assert!(e.history.memory_used() <= 4_000 + 4_000);
    assert!(e.history.can_undo());
}
