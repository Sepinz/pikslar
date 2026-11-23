//! M11's measurements, on the tilemap stress document (`stress_map.rs`) with eight Blob47 sets
//! whose slots are filled with its first tiles: each event of a terrain brush stroke as the app
//! presents it, the commit of a stroke over 100 cells, a fill of all 65,536 cells of an empty
//! map, and how much the sets slow placing tiles by hand.
//!
//! The last is a ratio, kept as a percentage: the mean placing event with the eight sets over
//! the mean without, the median of 12 rounds that each time both, taking turns to go first.
//! Windows' clock counts in 100 ns, finer than one placing event, so an event there is the
//! mean of a stroke's events.

use crate::gate::{Kind, Metric};
use crate::stress_map::{self, CELLS, SIDE, TILE};
use crate::workloads::{percentile, push, Canvas};
use pikslar_core::autotile::TerrainKind;
use pikslar_core::command::{Corners, TerrainShape, TerrainTool};
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{Document, Editor, StepCmd, TileCmd, TileTool};
use std::time::Instant;

/// Terrain sets on the tileset, and the slots of each.
const SETS: usize = 8;
const SLOTS: usize = 47;
/// Cells of one brush stroke, and strokes timed.
const BRUSH_CELLS: i32 = 100;
const STROKES: usize = 21;
const FILLS: usize = 5;
/// Placing events in one stroke, and strokes of each kind in the ratio.
const PLACE_EVENTS: usize = 2_000;
const PLACE_ROUNDS: usize = 12;
const PLACE_STROKES: usize = 9;

fn micros_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e6
}

fn median(mut times: Vec<f64>) -> f64 {
    times.sort_by(f64::total_cmp);
    percentile(&times, 0.5)
}

/// The stress document with `sets` Blob47 sets filled from its first tiles, and an empty map
/// layer of the same tileset on top for terrain to be painted on.
fn with_terrains(sets: usize) -> (Document, TilesetId) {
    let mut doc = stress_map::stress_map_document();
    let tileset = doc.layers()[1].tileset().expect("a tilemap layer");
    for n in 0..sets {
        let id = doc
            .add_terrain(tileset, TerrainKind::Blob47, "", false)
            .expect("room");
        for slot in 0..SLOTS {
            let tile = TileId::new(1 + (n * SLOTS + slot) as u32).expect("an id");
            doc.set_terrain_slot(tileset, id, slot, Some(tile))
                .expect("a slot");
        }
    }
    let top = doc.layers().len();
    doc.add_tilemap_layer(top, tileset).expect("room");
    (doc, tileset)
}

fn terrain(tileset: TilesetId, shape: TerrainShape) -> TileTool {
    TileTool::Terrain(TerrainTool {
        tileset,
        set: 1,
        shape,
        clear: false,
        corners: Corners::Cell,
    })
}

/// Brush strokes along a row of 100 cells on the empty map, presented as the app presents
/// them: every event, and each commit; each stroke undone before the next.
fn brush(doc: &Document, tileset: TilesetId, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(doc.clone());
    let mut canvas = Canvas::new(SIDE);
    editor.layer = doc.layers().len() - 1;
    canvas.present(&mut editor);
    let t = TILE as i32;
    let (mut events, mut commits) = (Vec::new(), Vec::new());
    for stroke in 0..STROKES {
        let y = t / 2 + (stroke % 8) as i32 * 32 * t;
        let tool = terrain(tileset, TerrainShape::Brush);
        let begin = TileCmd::Begin { tool, x: t / 2, y };
        editor.apply(begin).expect("begins");
        canvas.present(&mut editor);
        for i in 1..BRUSH_CELLS {
            let start = Instant::now();
            let x = t / 2 + i * t;
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
    push(out, "terrain_event_median", Kind::Micros, median(events));
    push(out, "terrain_commit_100", Kind::Micros, median(commits));
}

/// A terrain fill of the whole empty map, 65,536 cells, as one step; undone before the next.
fn fill(doc: &Document, tileset: TilesetId, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(doc.clone());
    editor.layer = doc.layers().len() - 1;
    let mut times = Vec::new();
    for _ in 0..FILLS {
        let tool = terrain(tileset, TerrainShape::Fill);
        let start = Instant::now();
        assert!(
            editor
                .apply(TileCmd::Begin { tool, x: 4, y: 4 })
                .expect("fills")
                .changed
        );
        times.push(micros_since(start));
        let cel = editor.doc().layers()[editor.layer].cel(0);
        assert_ne!(cel.get(0, 0), 0, "the fill reached the corner");
        let last = CELLS as i32 - 1;
        assert_ne!(cel.get(last, last), 0, "the fill reached the far corner");
        assert!(editor.apply(StepCmd::Undo).expect("undo").changed);
    }
    push(out, "terrain_fill_65536", Kind::Micros, median(times));
}

/// The median over strokes of the mean placing event, placing tile 3 (a terrain tile when there
/// are sets, which pins it) along rows of layer 1's cells.
fn place_event(doc: &Document, tileset: TilesetId) -> f64 {
    let mut editor = Editor::new(doc.clone());
    editor.layer = 1;
    let tile = TileId::new(3).expect("an id");
    editor.set_current_tile(tileset, tile).expect("a tile");
    let t = TILE as i32;
    let point = |i: usize, top: i32| {
        let (col, row) = ((i % CELLS) as i32, (i / CELLS) as i32);
        (col * t + t / 2, top + row * t)
    };
    let mut means = Vec::new();
    for stroke in 0..PLACE_STROKES {
        let top = 4 + (stroke % 4) as i32 * 512;
        let (x, y) = point(0, top);
        let tool = TileTool::Place;
        editor.apply(TileCmd::Begin { tool, x, y }).expect("begins");
        let start = Instant::now();
        for i in 1..PLACE_EVENTS {
            let (x, y) = point(i, top);
            editor.apply(TileCmd::To { x, y }).expect("drags");
        }
        means.push(micros_since(start) / (PLACE_EVENTS - 1) as f64);
        assert!(editor.apply(TileCmd::Commit).expect("commits").changed);
        assert!(editor.apply(StepCmd::Undo).expect("undo").changed);
    }
    median(means)
}

/// Placing by hand with eight terrain sets over placing with none, in percent: 100 is no cost.
fn place_ratio(plain: &Document, sets: &Document, tileset: TilesetId, out: &mut Vec<Metric>) {
    let mut ratios = Vec::with_capacity(PLACE_ROUNDS);
    for round in 0..PLACE_ROUNDS {
        // Which goes first alternates, so that warming up favours neither.
        let (with, without) = match round % 2 {
            0 => (place_event(sets, tileset), place_event(plain, tileset)),
            _ => {
                let without = place_event(plain, tileset);
                (place_event(sets, tileset), without)
            }
        };
        ratios.push(100.0 * with / without);
    }
    push(
        out,
        "manual_place_with_terrains",
        Kind::Percent,
        median(ratios),
    );
}

pub fn measure(out: &mut Vec<Metric>) {
    let (sets, tileset) = with_terrains(SETS);
    let (plain, _) = with_terrains(0);
    brush(&sets, tileset, out);
    fill(&sets, tileset, out);
    place_ratio(&plain, &sets, tileset, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sets_hold_their_slots_and_the_top_map_is_empty() {
        let (doc, tileset) = with_terrains(SETS);
        let set = doc.tileset(tileset).expect("a tileset");
        assert_eq!(set.terrains().len(), SETS);
        assert!(set
            .terrains()
            .iter()
            .all(|t| t.slots().iter().all(Option::is_some)));
        let top = doc.layers().len() - 1;
        assert_eq!(doc.layers()[top].tileset(), Some(tileset));
        assert_eq!(doc.layers()[top].cel(0).image().drawn_bounds(), None);
    }
}
