//! Workloads for the selection system, Pixel Lint and Line Rhythm.

use crate::gate::{Kind, Metric};
use crate::live_bytes;
use crate::workloads::{best_of, percentile, push, Canvas, Rng};
use pikslar_core::{lint_all, rgba, ColorMode, Document, Editor, SelectOp, Tool};
use pikslar_core::{DrawCmd, FloatingCmd, SelectionCmd};
use std::f64::consts::TAU;
use std::hint::black_box;
use std::time::Instant;

const ACTIVE_LAYER: usize = 2;
const LASSO_POINTS: usize = 400;
const DRAG_EVENTS: usize = 1000;
const LARGE_DRAG_EVENTS: usize = 200;
/// An empty cell, so the wand floods the largest region the art has.
const WAND_CELL: (i32, i32) = (3, 3);

fn editor_on(doc: &Document) -> Editor {
    let mut editor = Editor::new(doc.clone());
    editor.layer = ACTIVE_LAYER;
    editor
}

/// Selects the middle half of the canvas in each direction.
fn select_middle(editor: &mut Editor, size: i32) {
    let shape = editor.rect_shape((size / 4, size / 4), (size * 3 / 4, size * 3 / 4));
    editor
        .apply(SelectionCmd::Select {
            shape: Box::new(shape),
            op: SelectOp::Replace,
        })
        .expect("select");
}

/// A wobbly ring, which is about the most awkward outline a hand draws.
fn lasso_ring(size: i32) -> Vec<(i32, i32)> {
    (0..LASSO_POINTS)
        .map(|i| {
            let angle = i as f64 * TAU / LASSO_POINTS as f64;
            let radius = size as f64 * (0.35 + 0.1 * (angle * 5.0).sin());
            (
                size / 2 + (radius * angle.cos()) as i32,
                size / 2 + (radius * angle.sin()) as i32,
            )
        })
        .collect()
}

fn selection_shapes(doc: &Document, out: &mut Vec<Metric>) {
    let editor = editor_on(doc);
    let ring = lasso_ring(doc.width as i32);
    let (x, y) = WAND_CELL;
    let lasso = best_of(|| drop(black_box(editor.lasso_shape(&ring))));
    push(out, "select_lasso_shape", Kind::Micros, lasso);
    let flood = best_of(|| drop(black_box(editor.wand_shape(x, y, true))));
    push(out, "select_wand_contiguous", Kind::Micros, flood);
    let global = best_of(|| drop(black_box(editor.wand_shape(x, y, false))));
    push(out, "select_wand_global", Kind::Micros, global);
}

/// Dragging lifted pixels has to feel the same as drawing.
fn floating_drag(doc: &Document, label: &str, events: usize, out: &mut Vec<Metric>) {
    let size = doc.width as i32;
    let mut editor = editor_on(doc);
    let mut canvas = Canvas::new(doc.width);
    canvas.present(&mut editor);
    if label.is_empty() {
        select_middle(&mut editor, size);
    } else {
        editor.apply(SelectionCmd::All).expect("all");
    }
    canvas.present(&mut editor);
    let (before, pick_up) = (live_bytes(), Instant::now());
    editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    let mut preview = None;
    canvas.present_drag(&mut editor, &mut preview);
    let pick_up = pick_up.elapsed().as_secs_f64() * 1e6;
    let held = live_bytes().saturating_sub(before);
    let mut outline = editor
        .selection()
        .map_or_else(Vec::new, |mask| mask.outline());
    let mut rng = Rng(23);
    let mut times = Vec::with_capacity(events);
    for _ in 0..events {
        let (dx, dy) = (rng.range(5) - 2, rng.range(5) - 2);
        let start = Instant::now();
        editor
            .apply(FloatingCmd::Move { dx, dy, join: true })
            .expect("move");
        // The app shifts the marching ants' outline after every move.
        for edge in &mut outline {
            edge.x += dx;
            edge.y += dy;
        }
        black_box(&outline);
        canvas.present_drag(&mut editor, &mut preview);
        times.push(start.elapsed().as_secs_f64() * 1e6);
    }
    times.sort_by(f64::total_cmp);
    let (median, p99) = (percentile(&times, 0.5), percentile(&times, 0.99));
    push(
        out,
        &format!("float_drag{label}_pick_up"),
        Kind::Micros,
        pick_up,
    );
    push(
        out,
        &format!("float_drag{label}_held"),
        Kind::Bytes,
        held as f64,
    );
    push(
        out,
        &format!("float_drag{label}_event_median"),
        Kind::Micros,
        median,
    );
    push(
        out,
        &format!("float_drag{label}_event_p99"),
        Kind::Micros,
        p99,
    );
}

/// Dragging a corner handle of a whole large layer, the way the app does it.
fn floating_scale_drag(doc: &Document, out: &mut Vec<Metric>) {
    const EVENTS: usize = 60;
    let mut editor = editor_on(doc);
    let mut canvas = Canvas::new(doc.width);
    canvas.present(&mut editor);
    editor.apply(SelectionCmd::All).expect("all");
    canvas.present(&mut editor);
    editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    let mut preview = None;
    canvas.present_drag(&mut editor, &mut preview);
    let from = editor.selection().map_or((0, 0, 1, 1), |m| m.bounds());
    let outline = editor.selection().map_or_else(Vec::new, |m| m.outline());
    let mut rng = Rng(29);
    let mut times = Vec::with_capacity(EVENTS);
    for _ in 0..EVENTS {
        let grow = rng.range(33) - 16;
        let to = (from.0, from.1, from.2 + grow, from.3 + grow);
        let start = Instant::now();
        black_box(pikslar_core::scale_edges(&outline, from, to));
        canvas.present_scale(&editor, &mut preview, to);
        times.push(start.elapsed().as_secs_f64() * 1e6);
    }
    times.sort_by(f64::total_cmp);
    let (median, p99) = (percentile(&times, 0.5), percentile(&times, 0.99));
    push(out, "float_scale_1024_event_median", Kind::Micros, median);
    push(out, "float_scale_1024_event_p99", Kind::Micros, p99);
    let release = best_of(|| {
        editor
            .apply(FloatingCmd::Scale {
                x: from.0,
                y: from.1,
                w: from.2 + 5,
                h: from.3 + 5,
            })
            .expect("scale");
        editor
            .apply(FloatingCmd::Scale {
                x: from.0,
                y: from.1,
                w: from.2,
                h: from.3,
            })
            .expect("scale");
    });
    push(out, "float_scale_1024_release", Kind::Micros, release / 2.0);
}

fn floating_transforms(doc: &Document, out: &mut Vec<Metric>) {
    let size = doc.width as i32;
    let mut editor = editor_on(doc);
    let mut canvas = Canvas::new(doc.width);
    canvas.present(&mut editor);
    select_middle(&mut editor, size);
    editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    let flip = best_of(|| {
        editor
            .apply(FloatingCmd::FlipHorizontal)
            .expect("fliphorizontal");
        canvas.present(&mut editor);
    });
    push(out, "float_flip", Kind::Micros, flip);
    let rotate = best_of(|| {
        editor
            .apply(FloatingCmd::Rotate { clockwise: true })
            .expect("rotate");
        canvas.present(&mut editor);
    });
    push(out, "float_rotate", Kind::Micros, rotate);
    let scale = best_of(|| {
        for width in [size * 3 / 4, size / 3] {
            editor
                .apply(FloatingCmd::Scale {
                    x: size / 8,
                    y: size / 8,
                    w: width,
                    h: size / 2,
                })
                .expect("scale");
            canvas.present(&mut editor);
        }
    });
    push(out, "float_scale", Kind::Micros, scale);
}

fn lift_and_clipboard(doc: &Document, out: &mut Vec<Metric>) {
    let size = doc.width as i32;
    let mut editor = editor_on(doc);
    let whole = best_of(|| {
        select_middle(&mut editor, size);
        editor
            .apply(FloatingCmd::Lift { copy: false })
            .expect("lift");
        editor
            .apply(FloatingCmd::Move {
                dx: 1,
                dy: 1,
                join: true,
            })
            .expect("move");
        editor.apply(FloatingCmd::Anchor).expect("anchor");
        editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
        editor.take_damage();
    });
    push(out, "float_lift_move_anchor_undo", Kind::Micros, whole);
    select_middle(&mut editor, size);
    let copy = best_of(|| drop(black_box(editor.copy_selected())));
    push(out, "copy_selected", Kind::Micros, copy);
    let Some(clip) = editor.copy_selected() else {
        panic!("the middle of the canvas is selected");
    };
    let clip = std::sync::Arc::new(clip);
    let paste = best_of(|| {
        editor
            .apply(FloatingCmd::Paste {
                clip: clip.clone(),
                x: 8,
                y: 8,
            })
            .expect("the clip's colors fit");
        editor.apply(FloatingCmd::Revert).expect("revert");
        editor.take_damage();
    });
    push(out, "paste_and_cancel", Kind::Micros, paste);
}

fn checked(doc: &Document, layer: usize) -> usize {
    let cells = doc.layers()[layer].cel(0).to_cells(doc.width, doc.height);
    let palette = doc.layers()[layer].mode == ColorMode::Palette;
    let shown = |c: u32| if palette { doc.colors.color(c) } else { c };
    lint_all(&cells, doc.width, doc.height, shown).len()
}

fn pixel_lint(doc: &Document, label: &str, out: &mut Vec<Metric>) {
    let time = best_of(|| {
        black_box(checked(doc, ACTIVE_LAYER));
    });
    push(out, &format!("lint_{label}"), Kind::Micros, time);
}

/// A slow shallow line with a small tremor, the case Line Rhythm exists for.
fn tremor_path(size: i32) -> Vec<(i32, i32)> {
    let mut rng = Rng(31);
    (0..3000)
        .map(|i| {
            let wobble = (rng.range(3) - 1) * (i % 3 / 2);
            ((i / 6).min(size - 1), (i / 15 + wobble).clamp(0, size - 1))
        })
        .collect()
}

fn line_rhythm(doc: &Document, out: &mut Vec<Metric>) {
    let mut editor = editor_on(doc);
    let mut canvas = Canvas::new(doc.width);
    canvas.present(&mut editor);
    editor.rhythm = true;
    editor.color = rgba(240, 220, 120, 255);
    let path = tremor_path(doc.width as i32);
    editor
        .apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x: path[0].0,
            y: path[0].1,
        })
        .expect("begin");
    canvas.present(&mut editor);
    let mut times = Vec::with_capacity(path.len());
    for &(x, y) in &path[1..] {
        let start = Instant::now();
        editor.apply(DrawCmd::To { x, y }).expect("drag");
        canvas.present(&mut editor);
        times.push(start.elapsed().as_secs_f64() * 1e6);
    }
    editor.apply(DrawCmd::Commit).expect("commit");
    times.sort_by(f64::total_cmp);
    let (median, p99) = (percentile(&times, 0.5), percentile(&times, 0.99));
    push(out, "rhythm_event_median", Kind::Micros, median);
    push(out, "rhythm_event_p99", Kind::Micros, p99);
}

/// Runs every workload in this module on a 512 document and lint on a larger one too.
pub fn measure(doc: &Document, large: &Document, out: &mut Vec<Metric>) {
    selection_shapes(doc, out);
    floating_drag(doc, "", DRAG_EVENTS, out);
    floating_drag(large, "_1024_all", LARGE_DRAG_EVENTS, out);
    floating_transforms(doc, out);
    floating_scale_drag(large, out);
    lift_and_clipboard(doc, out);
    pixel_lint(doc, "512", out);
    pixel_lint(large, "1024", out);
    line_rhythm(doc, out);
}
