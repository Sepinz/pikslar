//! `pikslar-bench image-study`: what bounded images cost on the cases they were built for.
//!
//! Scattered art, one sprite on a huge canvas, tables that have to grow and strokes on held
//! frames. These stay out of the gate's baseline, which stays comparable with earlier builds.

use crate::stress::{large_document, stress_document};
use pikslar_core::{rgba, Cel, ColorMode, Document, DrawCmd, Editor, Grid, StepCmd, Tool};
use std::collections::HashSet;
use std::hint::black_box;
use std::time::Instant;

const INK: u32 = rgba(200, 40, 40, 255);
const TILE: usize = 16;
const ROUNDS: usize = 201;

fn report(name: &str, value: f64, unit: &str) {
    println!("{name:<44} {value:>14.1}  {unit}");
}

/// The cel a canvas of this size holds when `drawn` picks its pixels.
fn drawn(width: usize, height: usize, inked: impl Fn(usize, usize) -> bool) -> Cel {
    let mut cells = vec![0; width * height];
    for y in 0..height {
        for x in (0..width).filter(|&x| inked(x, y)) {
            cells[y * width + x] = INK;
        }
    }
    Cel::new(Grid::from_cells(width, height, &cells))
}

fn report_cel(name: &str, cel: &Cel) {
    report(
        &format!("{name}_cel_bytes"),
        cel.heap_size() as f64,
        "bytes",
    );
    report(
        &format!("{name}_table_bytes"),
        cel.table_bytes() as f64,
        "bytes",
    );
}

fn memory_of_shapes() {
    const SIDE: usize = 4096;
    report_cel(
        "scattered_every_4th_tile_4096",
        &drawn(SIDE, SIDE, |x, y| {
            x % (TILE * 4) == 3 && y % (TILE * 4) == 3
        }),
    );
    report_cel(
        "scattered_every_tile_4096",
        &drawn(SIDE, SIDE, |x, y| x % TILE == 3 && y % TILE == 3),
    );
    report_cel(
        "one_64px_sprite_8192",
        &drawn(8192, 8192, |x, y| {
            (4000..4064).contains(&x) && (4000..4064).contains(&y)
        }),
    );
    report_cel("blank_8192", &Cel::new(Grid::new(8192, 8192)));
}

/// Every image once, however many frames show it.
fn report_document(name: &str, doc: &Document) {
    let (mut table, mut all) = (0, 0);
    let mut seen = HashSet::new();
    for cel in doc.layers().iter().flat_map(|l| l.cels()) {
        if seen.insert(cel.image_id()) {
            table += cel.table_bytes();
            all += cel.heap_size();
        }
    }
    report(&format!("{name}_table_bytes"), table as f64, "bytes");
    report(&format!("{name}_tile_bytes"), (all - table) as f64, "bytes");
}

fn median_micros(mut round: impl FnMut() -> f64) -> f64 {
    let mut times: Vec<f64> = (0..ROUNDS).map(|_| round()).collect();
    times.sort_by(f64::total_cmp);
    times[ROUNDS / 2]
}

fn dot(editor: &mut Editor, x: i32, y: i32) {
    editor
        .apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(x, y)],
        })
        .expect("room");
}

/// A dot far from the only other drawing, then undoing it: the table grows, then shrinks.
fn growth_at(name: &str, width: usize, height: usize) {
    let mut doc = Document::new(width, height, ColorMode::Rgba).expect("valid");
    doc.put_cel(0, 0, drawn(width, height, |x, y| x < 4 && y < 4))
        .expect("fits");
    let mut editor = Editor::new(doc);
    editor.color = INK;
    let (x, y) = (width as i32 - 1, height as i32 - 1);
    let mut undo = Vec::with_capacity(ROUNDS);
    let grow = median_micros(|| {
        let start = Instant::now();
        dot(&mut editor, x, y);
        let grew = start.elapsed().as_secs_f64() * 1e6;
        let start = Instant::now();
        editor.apply(StepCmd::Undo).expect("undo");
        undo.push(start.elapsed().as_secs_f64() * 1e6);
        grew
    });
    undo.sort_by(f64::total_cmp);
    report(&format!("{name}_growth_commit"), grow, "us");
    report(&format!("{name}_growth_undo"), undo[ROUNDS / 2], "us");
}

/// One dot on a full 2048x3040 frame, in a hold of 64 frames if `held`.
fn stroke_on_a_full_canvas(held: bool) -> f64 {
    const FRAMES: usize = 64;
    let mut doc = Document::new(2048, 3040, ColorMode::Rgba).expect("valid");
    (1..FRAMES).for_each(|at| doc.add_frame(at, false).expect("room"));
    doc.put_cel(0, 0, drawn(2048, 3040, |_, _| true))
        .expect("fits");
    if held {
        doc.hold_across(0, 0, 0, FRAMES - 1).expect("frames");
    }
    let mut editor = Editor::new(doc);
    editor.color = rgba(1, 2, 3, 255);
    let mut x = 0;
    median_micros(|| {
        x = (x + 1) % 2048;
        let start = Instant::now();
        dot(&mut editor, x, 100);
        black_box(start.elapsed().as_secs_f64() * 1e6)
    })
}

pub fn run() {
    memory_of_shapes();
    report_document("stress", &stress_document());
    report_document("large", &large_document());
    growth_at("canvas_2048x3040", 2048, 3040);
    growth_at("largest_8192", 8192, 8192);
    let plain = stroke_on_a_full_canvas(false);
    let held = stroke_on_a_full_canvas(true);
    report("stroke_full_2048x3040", plain, "us");
    report("stroke_held_64_frames_2048x3040", held, "us");
    report("stroke_held_extra", held - plain, "us");
}
