//! Properties of terrain strokes on random maps: any order of the same painted points gives one
//! result, pinned cells that are not painted never change, and undo and redo give back the maps.

use super::terrain_fixture::{by_vertex, terrain, terrain_doc, CELLS, OTHER, TILE};
use super::PINNED;
use crate::autotile::TerrainKind;
use crate::command::{TerrainShape, TileCmd, TileTool};
use crate::document::Document;
use crate::editor::Editor;
use crate::grid::Grid;
use crate::layer::Cel;

const SEEDS: u32 = 60;

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn below(&mut self, n: u32) -> i32 {
        (self.next() % n) as i32
    }
}

/// A random map: empty cells, members of set 1 (some pinned) and another tile.
fn random_editor(kind: TerrainKind, rng: &mut Rng) -> Editor {
    let (mut doc, _) = terrain_doc(kind);
    let slots = kind.slot_count() as u32;
    let cells: Vec<u32> = (0..CELLS * CELLS)
        .map(|_| match rng.below(8) {
            0..=2 => 0,
            3..=5 => 1 + rng.next() % slots,
            6 => OTHER,
            _ => (1 + rng.next() % slots) | PINNED,
        })
        .collect();
    let side = CELLS as usize;
    doc.put_cel(1, 0, Cel::new(Grid::from_cells(side, side, &cells)))
        .expect("fits");
    assert_eq!(doc.check_tilemaps(), Ok(()));
    let mut e = Editor::new(doc);
    e.layer = 1;
    e
}

/// The rectangle's points in four orders: by rows and by columns, each way, snaking so every
/// step is to a neighbour and a stroke paints exactly these points.
fn orders((x0, y0, x1, y1): (i32, i32, i32, i32)) -> Vec<Vec<(i32, i32)>> {
    let mut rows = Vec::new();
    for (i, y) in (y0..=y1).enumerate() {
        let xs: Vec<i32> = (x0..=x1).collect();
        let xs: Box<dyn Iterator<Item = &i32>> = match i % 2 {
            0 => Box::new(xs.iter()),
            _ => Box::new(xs.iter().rev()),
        };
        rows.extend(xs.map(|&x| (x, y)).collect::<Vec<_>>());
    }
    let mut cols = Vec::new();
    for (i, x) in (x0..=x1).enumerate() {
        let ys: Vec<i32> = (y0..=y1).collect();
        let ys: Box<dyn Iterator<Item = &i32>> = match i % 2 {
            0 => Box::new(ys.iter()),
            _ => Box::new(ys.iter().rev()),
        };
        cols.extend(ys.map(|&y| (x, y)).collect::<Vec<_>>());
    }
    let back = |v: &Vec<(i32, i32)>| v.iter().rev().copied().collect();
    vec![back(&rows), back(&cols), rows, cols]
}

/// A random walk of steps to neighbours, kept on the canvas.
fn walk(rng: &mut Rng, high: i32) -> Vec<(i32, i32)> {
    let mut at = (rng.below(high as u32 + 1), rng.below(high as u32 + 1));
    let mut path = vec![at];
    for _ in 0..30 {
        let step = (rng.below(3) - 1, rng.below(3) - 1);
        at = (
            (at.0 + step.0).clamp(0, high),
            (at.1 + step.1).clamp(0, high),
        );
        path.push(at);
    }
    path
}

/// The tools tried: each kind, painting and clearing, and Corner4 by its corners too.
fn tools(e: &Editor, kind: TerrainKind) -> Vec<(TileTool, bool)> {
    let set = e.doc().layers()[1].tileset().expect("a map");
    let mut out = Vec::new();
    for clear in [false, true] {
        let tool = terrain(set, TerrainShape::Brush, clear);
        out.push((tool, false));
        if kind == TerrainKind::Corner4 {
            out.push((by_vertex(tool), true));
        }
    }
    out
}

/// Strokes through points: cell middles, or for vertices the cell corners.
fn stroke(tool: TileTool, vertices: bool, points: &[(i32, i32)]) -> TileCmd {
    let to_pixel = |&(x, y): &(i32, i32)| match vertices {
        true => (x * TILE, y * TILE),
        false => (x * TILE + 1, y * TILE + 1),
    };
    TileCmd::Stroke {
        tool,
        path: points.iter().map(to_pixel).collect(),
    }
}

/// Runs a stroke, checking pins, the maps' rules and undo and redo, and returns the document.
fn run(e: &mut Editor, tool: TileTool, vertices: bool, points: &[(i32, i32)]) -> Document {
    let before = e.doc().clone();
    let changed = e
        .apply(stroke(tool, vertices, points))
        .expect("paints")
        .changed;
    let after = e.doc().clone();
    assert_eq!(after.check_tilemaps(), Ok(()));
    let (old, new) = (
        before.layers()[1].cel(0).image(),
        after.layers()[1].cel(0).image(),
    );
    for y in -1..=CELLS {
        for x in -1..=CELLS {
            let painted = !vertices && points.contains(&(x, y));
            if old.get(x, y) & PINNED != 0 && !painted {
                assert_eq!(new.get(x, y), old.get(x, y), "pinned cell ({x}, {y})");
            }
        }
    }
    if changed {
        assert!(e.undo());
        assert_eq!(e.doc(), &before);
        assert!(e.redo());
        assert_eq!(e.doc(), &after);
    }
    after
}

fn kinds() -> [TerrainKind; 3] {
    [
        TerrainKind::Edge4,
        TerrainKind::Corner4,
        TerrainKind::Blob47,
    ]
}

#[test]
fn every_order_of_a_rectangle_of_points_gives_one_map() {
    for seed in 1..=SEEDS {
        for kind in kinds() {
            let mut rng = Rng(seed * 7919 + kind.slot_count() as u32);
            let start = random_editor(kind, &mut rng);
            for (tool, vertices) in tools(&start, kind) {
                let (a, b) = (rng.below(CELLS as u32), rng.below(CELLS as u32));
                let (c, d) = (rng.below(CELLS as u32), rng.below(CELLS as u32));
                let rect = (a.min(c), b.min(d), a.max(c), b.max(d));
                let results: Vec<Document> = orders(rect)
                    .iter()
                    .map(|order| {
                        let mut e = Editor::new(start.doc().clone());
                        e.layer = 1;
                        run(&mut e, tool, vertices, order)
                    })
                    .collect();
                for (i, r) in results.iter().enumerate().skip(1) {
                    assert!(
                        r == &results[0],
                        "seed {seed}, {kind:?}, {tool:?}, order {i}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_walk_and_the_same_walk_backwards_give_one_map() {
    for seed in 1..=SEEDS {
        for kind in kinds() {
            let mut rng = Rng(seed * 104_729 + kind.slot_count() as u32);
            let start = random_editor(kind, &mut rng);
            for (tool, vertices) in tools(&start, kind) {
                let high = if vertices { CELLS } else { CELLS - 1 };
                let path = walk(&mut rng, high);
                let back: Vec<_> = path.iter().rev().copied().collect();
                let mut ahead = Editor::new(start.doc().clone());
                let mut behind = Editor::new(start.doc().clone());
                ahead.layer = 1;
                behind.layer = 1;
                let one = run(&mut ahead, tool, vertices, &path);
                let two = run(&mut behind, tool, vertices, &back);
                assert!(one == two, "seed {seed}, {kind:?}, {tool:?}");
            }
        }
    }
}

#[test]
fn strokes_one_after_another_keep_pins_and_undo_to_the_start() {
    for seed in 1..=SEEDS / 2 {
        for kind in kinds() {
            let mut rng = Rng(seed * 31 + kind.slot_count() as u32);
            let mut e = random_editor(kind, &mut rng);
            let start = e.doc().clone();
            let tools = tools(&e, kind);
            let mut steps = 0;
            for _ in 0..6 {
                let (tool, vertices) = tools[rng.below(tools.len() as u32) as usize];
                let high = if vertices { CELLS } else { CELLS - 1 };
                let before = e.history().undo_depth();
                run(&mut e, tool, vertices, &walk(&mut rng, high));
                steps += e.history().undo_depth() - before;
            }
            for _ in 0..steps {
                assert!(e.undo());
            }
            assert!(e.doc() == &start, "seed {seed}, {kind:?}");
        }
    }
}
