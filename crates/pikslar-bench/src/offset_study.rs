//! `pikslar-bench offset-study`: what cel offsets cost, against the limits in
//! `docs/cel-offsets.md` section 4. These stay out of the gate's baseline, which stays comparable
//! with earlier builds. Each line gives the measurement, its limit and a verdict.

use crate::stress::{large_document, stress_document};
use crate::workloads::best_of;
use pikslar_core::{
    lint_all, lint_cel, rgba, CelCmd, ColorMode, Damage, Document, DrawCmd, Editor, StepCmd, Tool,
    Turn,
};
use pikslar_render::{Compositor, DragPreview, Rect, View};
use std::hint::black_box;
use std::time::Instant;

const ROUNDS: usize = 101;
/// A part-tile offset: every canvas tile is assembled from four image tiles.
const PART: (i32, i32) = (5, 9);
const ALIGNED: (i32, i32) = (16, 32);

fn report(name: &str, value: f64, unit: &str, limit: Option<(f64, &str)>) {
    let verdict = match limit {
        Some((most, what)) if value <= most => format!("within {what} {most}"),
        Some((most, what)) => format!("OVER {what} {most}"),
        None => String::new(),
    };
    println!("{name:<44} {value:>12.2}  {unit:<6} {verdict}");
}

fn median(mut times: Vec<f64>) -> f64 {
    times.sort_by(f64::total_cmp);
    times[times.len() / 2]
}

fn timed(f: impl FnOnce()) -> f64 {
    let start = Instant::now();
    f();
    start.elapsed().as_secs_f64() * 1e6
}

/// The same document with every cel moved by `by`. Holds move once, with their first frame.
fn moved(doc: &Document, by: (i32, i32)) -> Editor {
    let mut editor = Editor::new(doc.clone());
    for layer in 0..doc.layers().len() {
        let id = editor.layer_id(layer).expect("layer");
        let links: Vec<u32> = doc.layers()[layer].links().collect();
        for frame in 0..links.len() {
            let first = links[frame] == 0 || links[..frame].iter().all(|&l| l != links[frame]);
            if first {
                let (dx, dy) = by;
                editor
                    .apply(CelCmd::Move {
                        layer: id,
                        frame,
                        dx,
                        dy,
                    })
                    .expect("in range");
                editor.apply(StepCmd::EndDrag).expect("end");
            }
        }
    }
    editor
}

fn composite(editor: &Editor) -> f64 {
    let doc = editor.doc();
    let mut comp = Compositor::new(doc.width, doc.height);
    let mut changed = Vec::new();
    best_of(|| {
        changed.clear();
        comp.update(editor, Damage::All, &mut changed);
        black_box(comp.pixels());
    })
}

fn compositing(label: &str, doc: &Document) {
    let zero = composite(&Editor::new(doc.clone()));
    let part = composite(&moved(doc, PART));
    let aligned = composite(&moved(doc, ALIGNED));
    report(&format!("composite_{label}_offset_0"), zero, "us", None);
    report(&format!("composite_{label}_part_tile"), part, "us", None);
    report(&format!("composite_{label}_aligned"), aligned, "us", None);
    let ratio = Some((1.5, "ratio"));
    report(
        &format!("composite_{label}_part_tile_ratio"),
        part / zero,
        "x",
        ratio,
    );
}

/// A 40-point stroke committed, then undone outside the timing.
fn stroke_commit(editor: &mut Editor) -> f64 {
    editor.color = rgba(250, 10, 10, 255);
    let path: Vec<(i32, i32)> = (0..40).map(|i| (300 + i * 3, 400 + i)).collect();
    median(
        (0..ROUNDS)
            .map(|_| {
                let path = path.clone();
                let time = timed(|| {
                    editor
                        .apply(DrawCmd::Stroke {
                            tool: Tool::Pencil,
                            path,
                        })
                        .expect("room");
                });
                editor.apply(StepCmd::Undo).expect("undo");
                time
            })
            .collect(),
    )
}

fn strokes(doc: &Document) {
    let zero = stroke_commit(&mut Editor::new(doc.clone()));
    let part = stroke_commit(&mut moved(doc, PART));
    report("stroke_commit_2048x3040_offset_0", zero, "us", None);
    report("stroke_commit_2048x3040_part_tile", part, "us", None);
    report(
        "stroke_commit_part_tile_ratio",
        part / zero,
        "x",
        Some((2.0, "ratio")),
    );
}

/// Fills the whole of a blank 2048x3040 canvas, the largest region a fill can make.
fn whole_fill(by: (i32, i32)) -> f64 {
    let doc = Document::new(2048, 3040, ColorMode::Rgba).expect("valid");
    let mut editor = moved(&doc, by);
    editor.color = rgba(20, 200, 90, 255);
    median(
        (0..15)
            .map(|_| {
                let time = timed(|| {
                    editor
                        .apply(DrawCmd::Begin {
                            tool: Tool::Fill,
                            x: 1000,
                            y: 1500,
                        })
                        .expect("fill");
                });
                editor.apply(StepCmd::Undo).expect("undo");
                time
            })
            .collect(),
    )
}

fn fills() {
    let zero = whole_fill((0, 0));
    let part = whole_fill(PART);
    report("fill_whole_2048x3040_offset_0", zero / 1000.0, "ms", None);
    report(
        "fill_whole_2048x3040_part_tile",
        part / 1000.0,
        "ms",
        Some((100.0, "ms")),
    );
    report(
        "fill_whole_part_tile_ratio",
        part / zero,
        "x",
        Some((2.5, "ratio")),
    );
}

/// Moving a cel: a joined drag step, a new step, the allocations, and the undo entry's size.
fn moves(doc: &Document) {
    let mut editor = Editor::new(doc.clone());
    let layer = editor.layer_id(0).expect("layer");
    let step = |editor: &mut Editor, dx| {
        editor
            .apply(CelCmd::Move {
                layer,
                frame: 0,
                dx,
                dy: 1,
            })
            .expect("move");
    };
    step(&mut editor, 1);
    // The timings' room is made first, so only the moves are counted.
    let mut times = Vec::with_capacity(ROUNDS);
    let allocs = crate::total_allocs();
    for i in 0..ROUNDS {
        times.push(timed(|| step(&mut editor, if i % 2 == 0 { 3 } else { -3 })));
    }
    let joined_allocs = crate::total_allocs() - allocs;
    let joined = median(times);
    report("cel_move_joined", joined, "us", Some((1.0, "us")));
    report(
        "cel_move_joined_allocs",
        joined_allocs as f64,
        "allocs",
        Some((0.0, "allocs")),
    );
    let mut fresh = Vec::new();
    let mut entry = Vec::new();
    for _ in 0..ROUNDS {
        editor.apply(StepCmd::EndDrag).expect("end");
        let before = editor.history().memory_used();
        fresh.push(timed(|| step(&mut editor, 2)));
        entry.push((editor.history().memory_used() - before) as f64);
    }
    report("cel_move_new_step", median(fresh), "us", Some((1.0, "us")));
    report(
        "cel_move_undo_entry",
        median(entry),
        "bytes",
        Some((68.0, "bytes")),
    );
}

fn turns(doc: &Document) {
    for (label, by) in [("offset_0", (0, 0)), ("part_tile", PART)] {
        let mut editor = moved(doc, by);
        let layer = editor.layer_id(0).expect("layer");
        let times = (0..21)
            .map(|_| {
                let turn = Turn::Clockwise;
                let time = timed(|| {
                    let command = CelCmd::Turn {
                        layer,
                        frame: 0,
                        turn,
                    };
                    editor.apply(command).expect("turn");
                });
                editor.apply(StepCmd::Undo).expect("undo");
                time
            })
            .collect();
        report(
            &format!("turn_whole_2048x3040_{label}"),
            median(times) / 1000.0,
            "ms",
            None,
        );
    }
}

fn lints(doc: &Document) {
    let editor = moved(doc, PART);
    let cel = editor.doc().layers()[0].cel(0);
    let (w, h) = (doc.width, doc.height);
    let cells = cel.to_cells(w, h);
    let plain = best_of(|| {
        black_box(lint_all(&cells, w, h, |c| c));
    });
    let margin = best_of(|| {
        black_box(lint_cel(cel, w, h, |c| c));
    });
    report("lint_canvas_only_2048x3040", plain / 1000.0, "ms", None);
    report(
        "lint_cel_with_margin_2048x3040",
        margin / 1000.0,
        "ms",
        None,
    );
    report("lint_cel_margin_ratio", margin / plain, "x", None);
}

/// What the program pays per frame on a large canvas: the outline's extent, found again after
/// every change, and a frame of a cel being dragged.
fn program(doc: &Document) {
    let mut editor = moved(doc, PART);
    let bounds = best_of(|| {
        black_box(editor.doc().layers()[0].cel(0).drawn_bounds());
    });
    report("drawn_bounds_full_2048x3040", bounds, "us", None);
    let area = Rect::new(0, 0, 1600, 1000);
    let view = View::fit(doc.width as i32, doc.height as i32, area);
    let mut comp = Compositor::new(doc.width, doc.height);
    let build = timed(|| {
        black_box(DragPreview::build_cel(&mut comp, &editor, view, area));
    });
    let mut preview = DragPreview::build_cel(&mut comp, &editor, view, area).expect("in view");
    let layer = editor.layer_id(0).expect("layer");
    let frames: Vec<f64> = (0..ROUNDS)
        .map(|i| {
            let dx = if i % 2 == 0 { 7 } else { -7 };
            let (frame, dy) = (0, 0);
            editor
                .apply(CelCmd::Move {
                    layer,
                    frame,
                    dx,
                    dy,
                })
                .expect("move");
            timed(|| {
                black_box(preview.render_cel(&editor));
            })
        })
        .collect();
    report("cel_preview_build_2048x3040", build / 1000.0, "ms", None);
    report("cel_preview_frame_2048x3040", median(frames), "us", None);
}

pub fn run() {
    let large = large_document();
    compositing("2048x3040", &large);
    compositing("stress_128", &stress_document());
    strokes(&large);
    fills();
    moves(&stress_document());
    turns(&large);
    lints(&large);
    program(&large);
}
