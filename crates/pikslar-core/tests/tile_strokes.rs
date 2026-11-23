//! Random map strokes, stamps, fills and deletes on documents with tilemap layers, in both colour
//! modes and with square and oblong tiles. After every command the rules of the links and of the
//! tilemap layers hold, each stroke is one undo step, a stroke given as events equals the same
//! stroke given whole, drawings that frames hold or layers link show one picture, and undoing
//! walks back through exactly the states the steps went through, then redo walks forward again.
//!
//! A modest number of seeds runs with the suite; `many_random_tile_sessions` runs 2,000, or
//! `PIKSLAR_TILE_SEEDS`, when asked for with `--ignored`.

use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::*;
use std::collections::HashMap;
use std::sync::Arc;

const STEPS: usize = 40;
const QUICK_SEEDS: u64 = 60;
const LONG_SEEDS: u64 = 2_000;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn between(&mut self, a: i32, b: i32) -> i32 {
        a + (self.next() % (b - a + 1) as u64) as i32
    }
}

/// Two editors given the same commands: one is given each stroke whole, the other as events.
struct Pair {
    whole: Editor,
    events: Editor,
}

/// A 32 by 32 canvas of three frames: a paint layer, and tilemap layers 1 and 2 of one tileset
/// of five tiles, with frames 0 and 1 of layer 1 held and frame 2 of both layers linked.
fn make(mode: ColorMode, tile: (usize, usize)) -> (Editor, TilesetId, Vec<TileId>) {
    let mut doc = Document::new(32, 32, mode).expect("size");
    doc.add_frame(1, false).expect("room");
    doc.add_frame(2, false).expect("room");
    let set = doc.add_tileset("Set", mode, tile).expect("room");
    let tiles: Vec<TileId> = (0..5)
        .map(|_| doc.add_tile(set, Arc::new(Image::blank())).expect("fits"))
        .collect();
    doc.add_tilemap_layer(1, set).expect("room");
    doc.add_tilemap_layer(2, set).expect("room");
    let mut e = Editor::new(doc);
    e.layer = 1;
    e.set_current_tile(set, tiles[0]).expect("a tile");
    (e, set, tiles)
}

fn hold_and_link(e: &mut Editor) {
    let a = e.layer_id(1).expect("layer");
    e.apply(FrameCmd::HoldAcross {
        layer: a,
        source: 0,
        from: 0,
        to: 1,
    })
    .expect("holds");
    let drag = CelDrag {
        layers: (1, 1),
        frames: (2, 2),
        to_layer: 2,
        to_frame: 2,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    };
    // Whether the document takes the link is for the document to say; the test goes on either way.
    let _ = e.apply(CelCmd::Drag(drag));
}

fn tool(rng: &mut Rng) -> TileTool {
    [
        TileTool::Place,
        TileTool::Place,
        TileTool::Erase,
        TileTool::Fill,
        TileTool::Line,
        TileTool::Rect,
        TileTool::Pick,
    ][rng.below(7)]
}

fn path(rng: &mut Rng) -> Vec<(i32, i32)> {
    let count = 1 + rng.below(6);
    (0..count)
        .map(|_| (rng.between(-6, 40), rng.between(-6, 40)))
        .collect()
}

/// Something both editors are given the same way.
fn same(pair: &mut Pair, command: impl Into<Command> + Clone) {
    let a = pair.whole.apply(command.clone());
    let b = pair.events.apply(command);
    assert_eq!(a, b);
}

fn stamp_op(pair: &mut Pair, rng: &mut Rng, set: TilesetId, tiles: &[TileId]) {
    let pick = |rng: &mut Rng| tiles[rng.below(tiles.len())];
    match rng.below(5) {
        0 => {
            let tile = pick(rng);
            let a = pair.whole.set_current_tile(set, tile);
            assert_eq!(a, pair.events.set_current_tile(set, tile));
        }
        1 => {
            let (w, h) = (1 + rng.below(3), 1 + rng.below(3));
            let cells: Vec<Option<TileId>> = (0..w * h)
                .map(|_| (rng.below(4) != 0).then(|| pick(rng)))
                .collect();
            let a = pair.whole.set_tile_block(set, w, h, &cells);
            assert_eq!(a, pair.events.set_tile_block(set, w, h, &cells));
        }
        2 => {
            let turn = [
                Turn::FlipHorizontal,
                Turn::FlipVertical,
                Turn::Clockwise,
                Turn::CounterClockwise,
            ][rng.below(4)];
            let a = pair.whole.turn_tile_stamp(turn);
            assert_eq!(a, pair.events.turn_tile_stamp(turn));
        }
        3 => {
            let a = pair.whole.capture_tile_stamp();
            assert_eq!(a, pair.events.capture_tile_stamp());
        }
        _ => {
            pair.whole.clear_tile_stamp();
            pair.events.clear_tile_stamp();
        }
    }
    assert_eq!(pair.whole.tiles(), pair.events.tiles());
}

/// One stroke, whole on one editor and as events on the other.
fn stroke_op(pair: &mut Pair, rng: &mut Rng) {
    let (tool, path) = (tool(rng), path(rng));
    let a = pair.whole.apply(TileCmd::Stroke {
        tool,
        path: path.clone(),
    });
    let &(x, y) = &path[0];
    let b = pair.events.apply(TileCmd::Begin { tool, x, y });
    if b.is_ok() {
        for &(x, y) in &path[1..] {
            pair.events.apply(TileCmd::To { x, y }).expect("to");
        }
        // A fill and a pick are over at the press, which `Commit` then finds nothing to do for.
        pair.events.apply(TileCmd::Commit).expect("commit");
    }
    assert_eq!(a.is_ok(), b.is_ok(), "{tool:?} {path:?}: {a:?} {b:?}");
    assert!(!pair.whole.is_stroking() && !pair.events.is_stroking());
}

fn select_op(pair: &mut Pair, rng: &mut Rng) {
    match rng.below(3) {
        0 => same(pair, SelectionCmd::None),
        _ => {
            let (x, y) = (rng.between(-4, 24), rng.between(-4, 24));
            let (w, h) = (rng.between(1, 20), rng.between(1, 20));
            let shape = Box::new(Mask::rect(x, y, w, h));
            same(
                pair,
                SelectionCmd::Select {
                    shape,
                    op: SelectOp::Replace,
                },
            );
        }
    }
}

/// Every drawing that a hold or a link shares shows one picture: cels with one link number, on
/// any layer, hold equal images.
fn assert_links_write_through(doc: &Document) {
    let mut by_link: HashMap<u32, &Image> = HashMap::new();
    for layer in doc.layers() {
        for cel in layer.cels().iter().filter(|c| c.link() != 0) {
            let first = by_link.entry(cel.link()).or_insert_with(|| cel.image());
            assert_eq!(*first, cel.image(), "link {}", cel.link());
        }
    }
}

fn assert_sound(e: &Editor, after: &str) {
    assert_eq!(e.doc().check_links(), Ok(()), "{after}");
    assert_eq!(e.doc().check_tilemaps(), Ok(()), "{after}");
    assert_links_write_through(e.doc());
}

/// What a session did, so the test can show it was not idle.
#[derive(Default)]
struct Stats {
    steps: usize,
    cells: usize,
    linked: bool,
}

fn session(seed: u64) -> Stats {
    let mut rng = Rng::new(seed);
    let mode = if seed % 2 == 0 {
        ColorMode::Rgba
    } else {
        ColorMode::Palette
    };
    let tile = if seed / 2 % 2 == 0 { (4, 4) } else { (4, 2) };
    let (mut a, set, tiles) = make(mode, tile);
    let (mut b, _, _) = make(mode, tile);
    hold_and_link(&mut a);
    hold_and_link(&mut b);
    let mut pair = Pair {
        whole: a,
        events: b,
    };
    let mut states = vec![pair.whole.doc().clone()];
    for step in 0..STEPS {
        pair.whole.layer = 1 + rng.below(2);
        pair.whole.frame = rng.below(3);
        pair.events.layer = pair.whole.layer;
        pair.events.frame = pair.whole.frame;
        let depth = pair.whole.history().undo_depth();
        match rng.below(10) {
            0..=4 => stroke_op(&mut pair, &mut rng),
            5 | 6 => stamp_op(&mut pair, &mut rng, set, &tiles),
            7 => select_op(&mut pair, &mut rng),
            8 => same(&mut pair, TileCmd::DeleteSelected),
            _ => same(
                &mut pair,
                TileCmd::Stroke {
                    tool: TileTool::Fill,
                    path: path(&mut rng),
                },
            ),
        }
        let after = format!("seed {seed} step {step}");
        assert_eq!(
            pair.whole.doc(),
            pair.events.doc(),
            "{after}: events differ from whole"
        );
        let now = pair.whole.history().undo_depth();
        assert!(
            now == depth || now == depth + 1,
            "{after}: {depth} to {now}"
        );
        assert_eq!(now, pair.events.history().undo_depth(), "{after}");
        if now == depth + 1 {
            states.push(pair.whole.doc().clone());
        }
        assert_sound(&pair.whole, &after);
    }
    walk_history(&mut pair.whole, &states, seed);
    let mut stats = Stats {
        steps: states.len() - 1,
        ..Stats::default()
    };
    stats.linked = pair.whole.doc().layers()[2].cel(2).link() != 0;
    stats.cells = placed_cells(pair.whole.doc());
    stats
}

/// How many cells of the maps, over every layer and frame, hold a tile. Counted before the
/// walk undoes it all, so the caller calls this on the document at the end.
fn placed_cells(doc: &Document) -> usize {
    let mut count = 0;
    for layer in doc.layers().iter().filter(|l| l.tileset().is_some()) {
        for cel in layer.cels() {
            let at = |x, y| cel.image().get(x, y);
            count += (0..16)
                .flat_map(|y| (0..16).map(move |x| (x, y)))
                .filter(|&(x, y)| at(x, y) != 0)
                .count();
        }
    }
    count
}

/// Undoes every step, checking each state on the way down, then redoes them all and checks
/// each state on the way up. `states[k]` is the document with `k` steps made after the setup.
fn walk_history(e: &mut Editor, states: &[Document], seed: u64) {
    let steps = states.len() - 1;
    for k in (0..steps).rev() {
        assert!(e.apply(StepCmd::Undo).expect("undo").changed, "seed {seed}");
        assert_sound(e, &format!("seed {seed} undo to {k}"));
        assert_eq!(e.doc(), &states[k], "seed {seed}: undone to {k}");
    }
    for (k, want) in states.iter().enumerate().skip(1) {
        assert!(e.apply(StepCmd::Redo).expect("redo").changed, "seed {seed}");
        assert_sound(e, &format!("seed {seed} redo to {k}"));
        assert_eq!(e.doc(), want, "seed {seed}: redone to {k}");
    }
}

#[test]
fn random_tile_sessions_keep_every_rule() {
    let mut total = Stats::default();
    let mut linked = 0;
    for seed in 0..QUICK_SEEDS {
        let stats = session(seed);
        total.steps += stats.steps;
        total.cells += stats.cells;
        linked += usize::from(stats.linked);
    }
    // The sessions made steps and placed tiles, and the cross-layer link took in most.
    assert!(
        total.steps > QUICK_SEEDS as usize * 8,
        "{} steps",
        total.steps
    );
    assert!(total.cells > 0, "no tile was ever placed");
    assert!(linked > 0, "the link across layers never took");
}

#[test]
#[ignore = "2,000 sessions; run with --ignored"]
fn many_random_tile_sessions() {
    let seeds = std::env::var("PIKSLAR_TILE_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(LONG_SEEDS);
    for seed in 0..seeds {
        let _ = session(seed);
    }
}
