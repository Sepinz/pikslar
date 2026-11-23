//! A timing of placing tiles by hand on a stress-sized map, with and without terrain sets, run
//! on request: `cargo test --release -p pikslar-core --lib place_timing -- --ignored --nocapture`.
//! It prints; it asserts only that the strokes placed. The pinned bench lines are 11.5's.
//!
//! Windows' clock counts in 100 ns, finer than one event, so an event is timed as the mean of a
//! stroke's events, and the median is taken over strokes.

use crate::autotile::TerrainKind;
use crate::command::{TileCmd, TileTool};
use crate::document::{ColorMode, Document};
use crate::editor::Editor;
use crate::grid::Grid;
use crate::layer::Cel;
use crate::tileset::{TileId, TilesetId};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CELLS: usize = 256;
const TILES: u32 = 2_000;
const EVENTS: i32 = 2_000;
const STROKES: i32 = 21;

/// 256 by 256 cells of 8 by 8 tiles on one tilemap layer, every cell a tile of 2,000, and
/// `terrains` Blob47 sets that fill all their slots with the first tiles.
fn map_with(terrains: usize) -> (Document, TilesetId) {
    let side = CELLS * 8;
    let mut doc = Document::new(side, side, ColorMode::Rgba).expect("size");
    let set = doc.add_tileset("S", ColorMode::Rgba, (8, 8)).expect("room");
    for n in 0..TILES {
        let image = Grid::from_cells(8, 8, &[0xff00_0000 | (n + 1); 64]).into_image();
        doc.add_tile(set, Arc::new(image)).expect("fits");
    }
    doc.add_tilemap_layer(1, set).expect("room");
    let cells: Vec<u32> = (0..CELLS * CELLS)
        .map(|i| 1 + (i as u32 * 7) % TILES)
        .collect();
    doc.put_cel(1, 0, Cel::new(Grid::from_cells(CELLS, CELLS, &cells)))
        .expect("fits");
    for t in 0..terrains {
        let id = doc
            .add_terrain(set, TerrainKind::Blob47, "T", false)
            .expect("room");
        for slot in 0..47 {
            let tile = TileId::new(1 + (t * 47 + slot) as u32).expect("an id");
            doc.set_terrain_slot(set, id, slot, Some(tile))
                .expect("slot");
        }
    }
    (doc, set)
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort_unstable();
    times[times.len() / 2]
}

/// The medians over strokes of the press, the mean event and the commit, placing tile 3 (a
/// terrain tile when there are sets) along rows of cells.
fn time_place(terrains: usize) -> (Duration, Duration, Duration) {
    let (doc, set) = map_with(terrains);
    let mut e = Editor::new(doc);
    e.layer = 1;
    e.set_current_tile(set, TileId::new(3).expect("an id"))
        .expect("tile");
    let (mut presses, mut events, mut commits) = (Vec::new(), Vec::new(), Vec::new());
    for round in 0..STROKES {
        let top = 4 + (round % 4) * 64;
        let start = Instant::now();
        let tool = TileTool::Place;
        e.apply(TileCmd::Begin { tool, x: 4, y: top })
            .expect("begins");
        presses.push(start.elapsed());
        let start = Instant::now();
        for i in 1..EVENTS {
            let (x, y) = (4 + (i % 256) * 8, top + (i / 256) * 8);
            e.apply(TileCmd::To { x, y }).expect("drags");
        }
        events.push(start.elapsed() / (EVENTS - 1) as u32);
        let start = Instant::now();
        assert!(e.apply(TileCmd::Commit).expect("commits").changed);
        commits.push(start.elapsed());
        assert!(e.undo());
    }
    (median(presses), median(events), median(commits))
}

/// Terrain on the same map: the mean event and commit of a brush stroke over 100 cells, and a
/// fill of all 65,536 cells, each the median of several.
#[test]
#[ignore = "a timing, run on request in release"]
fn terrain_timing() {
    let (mut doc, set) = map_with(8);
    let side = CELLS;
    doc.put_cel(
        1,
        0,
        Cel::new(Grid::from_cells(side, side, &vec![0; side * side])),
    )
    .expect("fits");
    let mut e = Editor::new(doc);
    e.layer = 1;
    let tool = |shape| {
        let corners = crate::command::Corners::Cell;
        let t = crate::command::TerrainTool {
            tileset: set,
            set: 1,
            shape,
            clear: false,
            corners,
        };
        TileTool::Terrain(t)
    };
    let (mut events, mut commits, mut fills) = (Vec::new(), Vec::new(), Vec::new());
    for round in 0..STROKES {
        let top = 4 + (round % 4) * 64;
        let brush = tool(crate::command::TerrainShape::Brush);
        e.apply(TileCmd::Begin {
            tool: brush,
            x: 4,
            y: top,
        })
        .expect("begins");
        let start = Instant::now();
        for i in 1..100 {
            e.apply(TileCmd::To {
                x: 4 + i * 8,
                y: top,
            })
            .expect("drags");
        }
        events.push(start.elapsed() / 99);
        let start = Instant::now();
        assert!(e.apply(TileCmd::Commit).expect("commits").changed);
        commits.push(start.elapsed());
        assert!(e.undo());
    }
    for _ in 0..5 {
        let fill = tool(crate::command::TerrainShape::Fill);
        let start = Instant::now();
        assert!(
            e.apply(TileCmd::Begin {
                tool: fill,
                x: 4,
                y: 4
            })
            .expect("fills")
            .changed
        );
        fills.push(start.elapsed());
        assert!(e.undo());
    }
    println!(
        "terrain: event median {:?}, 100-cell commit median {:?}, 65,536-cell fill median {:?}",
        median(events),
        median(commits),
        median(fills)
    );
}

#[test]
#[ignore = "a timing, run on request in release"]
fn place_timing() {
    for terrains in [0, 8, 0, 8] {
        let (press, event, commit) = time_place(terrains);
        println!(
            "place with {terrains} terrain sets: press median {press:?}, event median {event:?}, commit median {commit:?}"
        );
    }
}
