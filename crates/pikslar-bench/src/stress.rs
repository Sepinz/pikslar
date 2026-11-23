//! The heaviest documents the gate holds Pikslar to, in the bench and in the real program.

use crate::gate::{Kind, Metric};
use crate::live_bytes;
use crate::workloads::{best_of, indexed_or_plain, push, ramp, sprite_layer, Rng};
use pikslar_core::{ColorMode, Damage, Document, Editor, Grid};
use pikslar_format::piks;
use pikslar_render::Compositor;
use std::hint::black_box;
use std::path::Path;

const STRESS_FRAMES: usize = 400;
const STRESS_LAYERS: usize = 12;
const STRESS_SIDE: usize = 128;
const BLOBS: u32 = 6;
const LARGE_WIDTH: usize = 2048;
const LARGE_HEIGHT: usize = 3040;
/// How many frames the stepping measurement visits.
const STEPS: usize = 40;

/// A few shapes that drift from frame to frame, so no two drawings are shared.
fn moving_cel(layer: u32, frame: u32, rng: &mut Rng) -> Vec<u32> {
    let side = STRESS_SIDE as u32;
    let mut px = vec![0u32; STRESS_SIDE * STRESS_SIDE];
    for blob in 0..BLOBS {
        let seed = layer * 31 + blob * 7 + 3;
        let (bw, bh) = (10 + seed % 20, 8 + seed * 3 % 24);
        let (x0, y0) = (
            (seed * 17 + frame * (1 + blob % 3)) % side,
            seed * 29 % side,
        );
        for y in y0..(y0 + bh).min(side) {
            for x in x0..(x0 + bw).min(side) {
                let step = (x - x0) * 3 / bw + rng.range(2) as u32;
                px[(y * side + x) as usize] = ramp(layer % 8, step);
            }
        }
    }
    px
}

/// 400 frames of 12 layers, every drawing different, half palette and half RGBA.
pub fn stress_document() -> Document {
    let mut doc = Document::new(STRESS_SIDE, STRESS_SIDE, ColorMode::Palette).expect("valid size");
    for layer in 1..STRESS_LAYERS {
        let mode = if layer % 2 == 0 {
            ColorMode::Palette
        } else {
            ColorMode::Rgba
        };
        doc.add_layer(layer, mode).expect("room");
    }
    for frame in 1..STRESS_FRAMES {
        doc.add_frame(frame, false).expect("room");
    }
    let mut rng = Rng(11);
    for layer in 0..STRESS_LAYERS {
        for frame in 0..STRESS_FRAMES {
            let cells = moving_cel(layer as u32, frame as u32, &mut rng);
            let mode = doc.layers()[layer].mode;
            let stored = indexed_or_plain(&mut doc, mode, cells);
            let grid = Grid::from_cells(STRESS_SIDE, STRESS_SIDE, &stored);
            doc.set_cel(layer, frame, grid).expect("canvas size");
        }
    }
    doc
}

/// One full 2048 x 3040 RGBA layer with nothing empty, as a large finished sheet would be.
pub fn large_document() -> Document {
    full_document(LARGE_WIDTH, LARGE_HEIGHT)
}

/// One full RGBA layer of this size with nothing empty.
pub fn full_document(width: usize, height: usize) -> Document {
    let mut doc = Document::new(width, height, ColorMode::Rgba).expect("valid size");
    let mut rng = Rng(7);
    let mut px = sprite_layer(width, height, &mut rng);
    for (index, pixel) in px.iter_mut().enumerate().filter(|(_, p)| **p == 0) {
        *pixel = ramp(1, index as u32 / 97 % 4);
    }
    let grid = Grid::from_cells(width, height, &px);
    doc.set_cel(0, 0, grid).expect("canvas size");
    doc
}

pub fn write_project(doc: &Document, path: &Path) -> Result<(), String> {
    std::fs::write(path, piks::save(doc)).map_err(|e| format!("{}: {e}", path.display()))
}

fn frame_stepping(doc: &Document, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(doc.clone());
    let mut comp = Compositor::new(doc.width, doc.height);
    let mut changed = Vec::new();
    let stride = doc.frames.len() / STEPS;
    let time = best_of(|| {
        for step in 0..STEPS {
            editor.frame = step * stride;
            changed.clear();
            comp.update(&editor, Damage::All, &mut changed);
            black_box(comp.pixels());
        }
    });
    push(out, "stress_frame_step", Kind::Micros, time / STEPS as f64);
}

fn structural_edits(doc: &Document, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(doc.clone());
    let middle = doc.frames.len() / 2;
    let time = best_of(|| {
        editor
            .apply(pikslar_core::FrameCmd::Add {
                at: middle,
                copy: true,
            })
            .expect("room");
        editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
    });
    push(out, "stress_add_frame_and_undo", Kind::Micros, time);
    let mut editor = Editor::new(doc.clone());
    let before = live_bytes();
    for _ in 0..100 {
        editor
            .apply(pikslar_core::TagCmd::Add {
                name: "x".into(),
                from: 0,
                to: 1,
            })
            .expect("room");
    }
    let retained = live_bytes().saturating_sub(before);
    push(
        out,
        "stress_100_steps_retained",
        Kind::Bytes,
        retained as f64,
    );
}

/// Adds the stress document's numbers to the relative gate.
pub fn measure(out: &mut Vec<Metric>) {
    let before = live_bytes();
    let doc = stress_document();
    let retained = live_bytes().saturating_sub(before);
    push(out, "stress_retained", Kind::Bytes, retained as f64);
    let bytes = piks::save(&doc);
    push(out, "stress_file_size", Kind::Bytes, bytes.len() as f64);
    let save = best_of(|| drop(black_box(piks::save(&doc))));
    push(out, "stress_save", Kind::Micros, save);
    let load = best_of(|| drop(black_box(piks::load(&bytes).expect("valid"))));
    push(out, "stress_load", Kind::Micros, load);
    frame_stepping(&doc, out);
    structural_edits(&doc, out);
}
