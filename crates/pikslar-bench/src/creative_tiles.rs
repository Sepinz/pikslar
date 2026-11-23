//! M10's measurements, on the tilemap stress document (`stress_map.rs`): placing tiles by hand,
//! editing a tile placed 4,000 times through the canvas and redrawing after it, compositing the
//! whole map against the same picture held by paint layers, stepping frames, saving, loading,
//! the TMJ export and reading an Aseprite file of the same size.

use crate::gate::{Kind, Metric};
use crate::stress_map::{self, CELLS, EDITED, SIDE, TILE, TILES};
use crate::workloads::{best_of, percentile, push, Canvas};
use pikslar_core::tileset::TileId;
use pikslar_core::{
    ColorMode, Damage, Document, DrawCmd, Editor, Grid, StepCmd, TileCmd, TileTool, Tool,
};
use pikslar_format::{import_ase, piks};
use pikslar_render::{Compositor, PARALLEL_WORK};
use std::hint::black_box;
use std::time::Instant;

#[path = "../../pikslar-format/src/ase_build.rs"]
mod ase_build;

/// Events in one stroke of placing, and strokes timed.
const EVENTS: usize = 2_000;
const STROKES: usize = 9;
/// Tile edits timed; the median is kept.
const EDITS: usize = 21;

fn micros_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e6
}

fn median(mut times: Vec<f64>) -> f64 {
    times.sort_by(f64::total_cmp);
    percentile(&times, 0.5)
}

/// The point of the `i`th event of a stroke along rows of cells, each in a new cell.
fn place_point(i: usize, top: i32) -> (i32, i32) {
    let (col, row) = ((i % CELLS) as i32, (i / CELLS) as i32);
    let t = TILE as i32;
    (col * t + t / 2, top + row * t)
}

/// Each event of a placing stroke on a map layer, presented as the app presents it, and the
/// commit of the stroke, undone before the next.
fn place(doc: &Document, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(doc.clone());
    let mut canvas = Canvas::new(SIDE);
    canvas.present(&mut editor);
    editor.layer = 1;
    let set = doc.layers()[1].tileset().expect("a tilemap layer");
    let tile = TileId::new(3).expect("an id");
    editor.set_current_tile(set, tile).expect("a tile");
    let (mut events, mut commits) = (Vec::with_capacity(EVENTS * STROKES), Vec::new());
    for stroke in 0..STROKES {
        let top = 4 + (stroke % 4) as i32 * 512;
        let (x, y) = place_point(0, top);
        let begin = TileCmd::Begin {
            tool: TileTool::Place,
            x,
            y,
        };
        editor.apply(begin).expect("begins");
        canvas.present(&mut editor);
        for i in 1..EVENTS {
            let (x, y) = place_point(i, top);
            let start = Instant::now();
            editor.apply(TileCmd::To { x, y }).expect("drags");
            canvas.present(&mut editor);
            events.push(micros_since(start));
        }
        let start = Instant::now();
        assert!(editor.apply(TileCmd::Commit).expect("commits").changed);
        commits.push(micros_since(start));
        canvas.present(&mut editor);
        assert!(editor.apply(StepCmd::Undo).expect("undo").changed);
        canvas.present(&mut editor);
    }
    push(out, "tile_place_event_median", Kind::Micros, median(events));
    push(out, "tile_place_commit", Kind::Micros, median(commits));
}

/// A pencil stroke on a cell placing the tile that layer 1 places 4,000 times: the commit,
/// which writes the tile, and the redraw of every canvas tile that shows it.
fn edit_through(doc: &Document, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(doc.clone());
    let mut comp = Compositor::new(SIDE, SIDE);
    let mut changed = Vec::new();
    comp.update(&editor, Damage::All, &mut changed);
    editor.layer = 1;
    let (mut commits, mut redraws) = (Vec::new(), Vec::new());
    for round in 0..EDITS as u32 {
        editor.color = 0xff80_0000 | round;
        editor.take_damage();
        let stroke = DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(2, 3), (5, 3)],
        };
        let start = Instant::now();
        assert!(editor.apply(stroke).expect("paints").changed);
        commits.push(micros_since(start));
        let damage = editor.take_damage();
        assert!(matches!(damage, Damage::Placed(_)), "a tile edit");
        changed.clear();
        let start = Instant::now();
        comp.update(&editor, damage, &mut changed);
        redraws.push(micros_since(start));
        assert!(editor.apply(StepCmd::Undo).expect("undo").changed);
        let undone = editor.take_damage();
        comp.update(&editor, undone, &mut changed);
    }
    push(
        out,
        "tile_edit_commit_stress",
        Kind::Micros,
        median(commits),
    );
    push(
        out,
        "tile_edit_redraw_stress",
        Kind::Micros,
        median(redraws),
    );
}

/// The first frame drawn as four paint layers: each tilemap layer replaced by the pixels it
/// shows. Compositing it is what the paint path costs for the same picture.
pub fn paint_twin(doc: &Document) -> Document {
    let mut twin = Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid size");
    for (index, layer) in doc.layers().iter().enumerate() {
        if index > 0 {
            twin.add_layer(index, ColorMode::Rgba).expect("room");
        }
        let px: Vec<u32> = (0..SIDE * SIDE)
            .map(|i| doc.layer_pixel(layer, 0, (i % SIDE) as i32, (i / SIDE) as i32))
            .collect();
        twin.set_cel(index, 0, Grid::from_cells(SIDE, SIDE, &px))
            .expect("canvas size");
    }
    twin
}

/// One composite of every canvas tile, kept to one thread as when these lines were recorded.
fn full_composite(doc: &Document) -> f64 {
    let editor = Editor::new(doc.clone());
    let mut comp = Compositor::new(SIDE, SIDE);
    comp.set_threads(1, PARALLEL_WORK);
    let mut changed = Vec::new();
    best_of(|| {
        changed.clear();
        comp.update(&editor, Damage::All, &mut changed);
        black_box(comp.pixels());
    })
}

/// The whole map composited, and the same picture from paint layers. The plan's target is a
/// ratio of at most 2; the report prints it.
fn composites(doc: &Document, out: &mut Vec<Metric>) {
    let map = full_composite(doc);
    let paint = full_composite(&paint_twin(doc));
    push(out, "composite_full_stress_map", Kind::Micros, map);
    push(out, "composite_full_stress_paint", Kind::Micros, paint);
    eprintln!(
        "composite of the stress map against paint layers: {:.2}x",
        map / paint
    );
}

/// Stepping through every frame, each drawn whole as a frame change draws it.
fn frame_step(doc: &Document, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(doc.clone());
    let mut comp = Compositor::new(SIDE, SIDE);
    // One thread, as when the line was recorded; `tile_steps.rs` times the program's steps.
    comp.set_threads(1, PARALLEL_WORK);
    let mut changed = Vec::new();
    let frames = doc.frames.len();
    let time = best_of(|| {
        for frame in 0..frames {
            editor.frame = (frame + 1) % frames;
            changed.clear();
            comp.update(&editor, Damage::All, &mut changed);
            black_box(comp.pixels());
        }
    });
    push(
        out,
        "stress_map_frame_step",
        Kind::Micros,
        time / frames as f64,
    );
}

fn files(doc: &Document, out: &mut Vec<Metric>) {
    let bytes = piks::save(doc);
    let save = best_of(|| drop(black_box(piks::save(doc))));
    push(out, "stress_map_save", Kind::Micros, save);
    let load = best_of(|| drop(black_box(piks::load(&bytes).expect("opens"))));
    push(out, "stress_map_load", Kind::Micros, load);
}

/// The TMJ of the first frame and its atlas, written to memory.
fn tmj(doc: &Document, out: &mut Vec<Metric>) {
    let path = std::path::PathBuf::from("stress.tmj");
    let time = best_of(|| {
        let planned = pikslar_export::tmj_outputs(doc, 0, path.clone()).expect("plans");
        for mut output in planned.outputs {
            let mut sink = Vec::new();
            (output.write)(&mut sink).expect("writes");
            black_box(sink);
        }
    });
    push(out, "tmj_export_stress", Kind::Micros, time);
}

/// An Aseprite file the size of the stress map: 2,000 tiles of 8 by 8 after an empty tile 0,
/// and three 32-bit maps of 256 by 256 cells, the stress map's first frame, flips and all.
pub fn stress_ase() -> Vec<u8> {
    use ase_build::{cel_tilemap, tilemap_layer, tileset_chunk, Ase, MASKS_NEW};
    let pixels: Vec<u8> = std::iter::repeat_n(0u32, TILE * TILE)
        .chain((1..=TILES).flat_map(stress_map::tile_pixels))
        .flat_map(|p| p.to_le_bytes())
        .collect();
    let size = (TILE as u16, TILE as u16);
    let mut chunks = vec![tileset_chunk(0, 4, TILES + 1, size, "Stress", &pixels)];
    for layer in 1..=3u16 {
        chunks.push(tilemap_layer(3, 0, &format!("map {layer}"), 0));
    }
    // The stress map's cells keep their flips where Aseprite's newer masks put them.
    for layer in 1..=3usize {
        let values = stress_map::map_cells(layer, 0);
        let side = (CELLS as u16, CELLS as u16);
        let index = (layer - 1) as u16;
        chunks.push(cel_tilemap(index, (0, 0), side, 32, MASKS_NEW, &values));
    }
    Ase::new(SIDE as u16, SIDE as u16, 32)
        .frame(100, chunks)
        .bytes()
}

fn ase_import(out: &mut Vec<Metric>) {
    let bytes = stress_ase();
    let time = best_of(|| {
        let (doc, _) = import_ase(black_box(&bytes)).expect("imports");
        assert_eq!(doc.tilesets.len(), 1);
        black_box(doc);
    });
    push(out, "ase_tilemap_import", Kind::Micros, time);
}

pub fn measure(out: &mut Vec<Metric>) {
    let doc = stress_map::stress_map_document();
    assert_eq!(doc.layers()[1].cel(0).get(0, 0), EDITED);
    place(&doc, out);
    edit_through(&doc, out);
    composites(&doc, out);
    frame_step(&doc, out);
    crate::tile_steps::measure(out);
    files(&doc, out);
    tmj(&doc, out);
    ase_import(out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stress_ase_imports_as_the_stress_maps_first_frame() {
        let (doc, _) = import_ase(&stress_ase()).expect("imports");
        let set = doc.tilesets.iter().next().expect("a tileset");
        assert_eq!((set.len(), set.tile_size()), (TILES as usize, (TILE, TILE)));
        assert_eq!(doc.layers().len(), 3);
        for (index, layer) in doc.layers().iter().enumerate() {
            let cells = stress_map::map_cells(index + 1, 0);
            let cel = layer.cel(0);
            let at = |i: usize| cel.get((i % CELLS) as i32, (i / CELLS) as i32);
            assert!((0..CELLS * CELLS).step_by(97).all(|i| at(i) == cells[i]));
        }
    }

    #[test]
    fn place_points_step_one_cell_at_a_time() {
        assert_eq!(place_point(0, 4), (4, 4));
        assert_eq!(place_point(1, 4), (12, 4));
        assert_eq!(place_point(CELLS, 4), (4, 12));
    }
}
