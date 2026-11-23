//! A timing of editing a tile through the canvas on a stress-sized map, run on request:
//! `cargo test --release -p pikslar-core write_through_timing -- --ignored --nocapture`.
//! It prints; it asserts only that the edit happened. The pinned bench lines are 10.10's.

use crate::document::{ColorMode, Document};
use crate::editor::{Damage, Editor, Tool};
use crate::grid::Grid;
use crate::layer::Cel;
use crate::DrawCmd;
use std::sync::Arc;
use std::time::Instant;

const CELLS: usize = 256;
const TILES: u32 = 2_000;
const EDITED: u32 = 7;
const PLACED: usize = 4_000;

/// 256 by 256 cells of 8 by 8 tiles (a 2,048 square canvas), three tilemap layers sharing one
/// tileset of 2,000 tiles over a paint layer. Layer 1 places the edited tile 4,000 times, in
/// cells 16 apart, and every other cell of every map a random other tile.
fn stress_map() -> Document {
    let side = CELLS * 8;
    let mut doc = Document::new(side, side, ColorMode::Rgba).expect("size");
    let set = doc.add_tileset("S", ColorMode::Rgba, (8, 8)).expect("room");
    for n in 0..TILES {
        let image = Grid::from_cells(8, 8, &[0xff00_0000 | (n + 1); 64]).into_image();
        doc.add_tile(set, Arc::new(image)).expect("fits");
    }
    let mut seed = 0x2545_f491_u32;
    for layer in 1..=3 {
        doc.add_tilemap_layer(layer, set).expect("room");
        let cells: Vec<u32> = (0..CELLS * CELLS)
            .map(|i| match (layer, i % 16, i / 16) {
                (1, 0, n) if n < PLACED => EDITED,
                _ => {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let id = 1 + seed % TILES;
                    if id == EDITED {
                        EDITED + 1
                    } else {
                        id
                    }
                }
            })
            .collect();
        let map = Cel::new(Grid::from_cells(CELLS, CELLS, &cells));
        doc.put_cel(layer, 0, map).expect("fits");
    }
    doc
}

#[test]
#[ignore = "a timing, run on request in release"]
fn write_through_timing() {
    let mut e = Editor::new(stress_map());
    e.layer = 1;
    let drawn = e.doc().screen_layers();
    let (mut commits, mut damages) = (Vec::new(), Vec::new());
    for round in 0..41u32 {
        e.color = 0xff80_0000 | round;
        e.take_damage();
        let start = Instant::now();
        let stroke = DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(2, 3), (5, 3)],
        };
        assert!(e.apply(stroke).expect("paints").changed);
        let committed = start.elapsed();
        let Damage::Placed(edit) = e.take_damage() else {
            panic!("a tile edit");
        };
        let start = Instant::now();
        let mut marked = 0usize;
        e.doc()
            .placed_canvas_tiles(&edit, 0, &drawn, |_| marked += 1);
        let found = start.elapsed();
        assert!(marked >= PLACED, "{marked} canvas tiles");
        commits.push(committed);
        damages.push(found);
        assert!(e.undo());
    }
    commits.sort_unstable();
    damages.sort_unstable();
    let mid = commits.len() / 2;
    println!(
        "write-through on {PLACED} placements: commit median {:?} (best {:?}), damage median {:?} (best {:?}), sum of medians {:?}",
        commits[mid], commits[0], damages[mid], damages[0], commits[mid] + damages[mid]
    );
}
