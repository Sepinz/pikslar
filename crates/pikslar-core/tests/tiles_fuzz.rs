//! Random commands on documents with tilemap layers of two tilesets, one RGBA with square tiles
//! and one palette with oblong ones. After every step the rules of the tilemaps (T1 to T6) and
//! of the links hold and nothing panicked; undoing walks back through exactly the documents the
//! steps made, then redo walks forward to the last again.
//!
//! A modest number of seeds runs with the suite; `many_random_sessions_keep_the_tilemaps` runs
//! 2,000, or `PIKSLAR_TILE_SEEDS`, when asked for with `--ignored`.

use pikslar_core::autotile::TerrainKind;
use pikslar_core::command::CanvasCmd;
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::*;
use std::collections::HashMap;
use std::sync::Arc;

const STEPS: usize = 40;
const QUICK_SEEDS: u64 = 60;
const LONG_SEEDS: u64 = 2_000;
const KINDS: [TerrainKind; 3] = [
    TerrainKind::Edge4,
    TerrainKind::Corner4,
    TerrainKind::Blob47,
];
const MAP_TOOLS: [TileTool; 6] = [
    TileTool::Place,
    TileTool::Erase,
    TileTool::Fill,
    TileTool::Line,
    TileTool::Rect,
    TileTool::Pick,
];
const TURNS: [Turn; 4] = [
    Turn::FlipHorizontal,
    Turn::FlipVertical,
    Turn::Clockwise,
    Turn::CounterClockwise,
];

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
        (self.next() % n.max(1) as u64) as usize
    }

    fn coin(&mut self) -> bool {
        self.next() & 1 == 0
    }

    fn point(&mut self) -> (i32, i32) {
        (self.below(28) as i32 - 2, self.below(20) as i32 - 2)
    }
}

/// A tile image of the set's size and mode, with clear places.
fn tile_image(doc: &Document, set: TilesetId, r: &mut Rng) -> Arc<Image> {
    let tileset = doc.tilesets.get(set).expect("a tileset");
    let ((w, h), mode) = (tileset.tile_size(), tileset.mode());
    let cells: Vec<u32> = (0..w * h)
        .map(|_| match (mode, r.below(4)) {
            (_, 0) => 0,
            (ColorMode::Palette, _) => r.below(doc.colors.len()) as u32,
            _ => rgba(r.next() as u8, 80, 160, 255),
        })
        .collect();
    Arc::new(Grid::from_cells(w, h, &cells).into_image())
}

fn new_set(e: &mut Editor, mode: ColorMode, size: (u16, u16)) -> TilesetId {
    let made = e
        .apply(TileCmd::NewTileset {
            name: format!("{mode:?}"),
            mode,
            tile_w: size.0,
            tile_h: size.1,
        })
        .expect("room");
    TilesetId::new(made.created.expect("an id"))
}

/// A paint layer and three map layers, two of the square RGBA set and one of the oblong
/// palette set, over three frames, with a few tiles placed on each map.
fn session() -> Editor {
    let doc = Document::new(24, 16, ColorMode::Rgba).expect("valid");
    let mut e = Editor::new(doc);
    let mut r = Rng::new(7);
    let square = new_set(&mut e, ColorMode::Rgba, (4, 4));
    let oblong = new_set(&mut e, ColorMode::Palette, (4, 2));
    for set in [square, square, square, oblong, oblong] {
        let image = tile_image(e.doc(), set, &mut r);
        let from = TileSource::Image(image);
        e.apply(TileCmd::AddTile { tileset: set, from })
            .expect("fits");
    }
    for (at, set) in [(1, square), (2, square), (3, oblong)] {
        e.apply(TileCmd::NewLayer { at, tileset: set })
            .expect("room");
    }
    for at in 1..3 {
        e.apply(FrameCmd::Add { at, copy: false }).expect("room");
    }
    for layer in 1..4 {
        e.layer = layer;
        let set = e.doc().layers()[layer].tileset().expect("a map");
        let first = e.doc().tilesets.get(set).expect("set").tiles()[0].id;
        e.set_current_tile(set, first).expect("a tile");
        let path = vec![(0, 0), (12, 8)];
        let tool = TileTool::Rect;
        e.apply(TileCmd::Stroke { tool, path }).expect("placed");
    }
    e
}

fn some_set(e: &Editor, r: &mut Rng) -> Option<TilesetId> {
    let ids: Vec<TilesetId> = e.doc().tilesets.iter().map(|s| s.id()).collect();
    (!ids.is_empty()).then(|| ids[r.below(ids.len())])
}

fn some_tile(e: &Editor, set: TilesetId, r: &mut Rng) -> Option<TileId> {
    let ids: Vec<TileId> = e
        .doc()
        .tilesets
        .get(set)?
        .tiles()
        .iter()
        .map(|t| t.id)
        .collect();
    (!ids.is_empty()).then(|| ids[r.below(ids.len())])
}

fn some_terrain(e: &Editor, set: TilesetId, r: &mut Rng) -> Option<u16> {
    let ids: Vec<u16> = e
        .doc()
        .tilesets
        .get(set)?
        .terrains()
        .iter()
        .map(|t| t.id())
        .collect();
    (!ids.is_empty()).then(|| ids[r.below(ids.len())])
}

/// A change to the tilesets themselves.
fn tileset_cmd(e: &Editor, set: TilesetId, r: &mut Rng) -> Command {
    let tile = some_tile(e, set, r);
    match (r.below(8), tile) {
        (0, _) => new_set_cmd(r),
        (1, _) => TileCmd::RenameTileset {
            tileset: set,
            name: format!("set {}", r.below(4)),
        }
        .into(),
        (2, _) => TileCmd::RemoveTileset { tileset: set }.into(),
        (3, Some(tile)) => TileCmd::DuplicateTile { tileset: set, tile }.into(),
        (4, Some(tile)) => TileCmd::RemoveTile { tileset: set, tile }.into(),
        (5, Some(tile)) => TileCmd::MoveTile {
            tileset: set,
            tile,
            to: r.below(8),
        }
        .into(),
        (6, Some(tile)) => TileCmd::SetTileImage {
            tileset: set,
            tile,
            image: tile_image(e.doc(), set, r),
        }
        .into(),
        _ => {
            let from = match r.coin() {
                true => TileSource::Blank,
                false => TileSource::Image(tile_image(e.doc(), set, r)),
            };
            TileCmd::AddTile { tileset: set, from }.into()
        }
    }
}

fn new_set_cmd(r: &mut Rng) -> Command {
    let side = [2u16, 3, 4][r.below(3)];
    TileCmd::NewTileset {
        name: "new".into(),
        mode: [ColorMode::Rgba, ColorMode::Palette][r.below(2)],
        tile_w: side,
        tile_h: [side, 2][r.below(2)],
    }
    .into()
}

fn terrain_cmd(e: &Editor, set: TilesetId, r: &mut Rng) -> Command {
    let slot = r.below(47) as u8;
    match (r.below(5), some_terrain(e, set, r)) {
        (_, None) | (0, _) => TerrainCmd::New {
            tileset: set,
            kind: KINDS[r.below(3)],
            name: "ground".into(),
            template: r.coin(),
        }
        .into(),
        (1, Some(id)) => TerrainCmd::Remove {
            tileset: set,
            set: id,
        }
        .into(),
        (2, Some(id)) => TerrainCmd::Clear {
            tileset: set,
            set: id,
            slot,
        }
        .into(),
        (3, Some(id)) => TerrainCmd::Rename {
            tileset: set,
            set: id,
            name: "t".into(),
        }
        .into(),
        (_, Some(id)) => match some_tile(e, set, r) {
            Some(tile) => TerrainCmd::Assign {
                tileset: set,
                set: id,
                slot,
                tile,
            }
            .into(),
            None => TerrainCmd::Clear {
                tileset: set,
                set: id,
                slot,
            }
            .into(),
        },
    }
}

/// A stroke on the active layer: a map tool with a chosen and turned stamp, a terrain tool, or
/// a pixel tool that draws through the cells onto the tiles.
fn stroke_cmd(e: &mut Editor, r: &mut Rng) -> Command {
    let path: Vec<(i32, i32)> = (0..1 + r.below(3)).map(|_| r.point()).collect();
    let Some(set) = e.doc().layers()[e.layer].tileset() else {
        return DrawCmd::Stroke {
            tool: Tool::Pencil,
            path,
        }
        .into();
    };
    match r.below(4) {
        0 => DrawCmd::Stroke {
            tool: [Tool::Pencil, Tool::Eraser, Tool::Line][r.below(3)],
            path,
        }
        .into(),
        1 => match some_terrain(e, set, r) {
            Some(id) => {
                let tool = TerrainTool {
                    tileset: set,
                    set: id,
                    shape: [TerrainShape::Brush, TerrainShape::Rect, TerrainShape::Fill]
                        [r.below(3)],
                    clear: r.below(3) == 0,
                    corners: [Corners::Cell, Corners::Vertex][r.below(2)],
                };
                let tool = TileTool::Terrain(tool);
                TileCmd::Stroke { tool, path }.into()
            }
            None => terrain_cmd(e, set, r),
        },
        _ => {
            if let Some(tile) = some_tile(e, set, r) {
                let _ = e.set_current_tile(set, tile);
                for _ in 0..r.below(3) {
                    let _ = e.turn_tile_stamp(TURNS[r.below(4)]);
                }
            }
            let tool = MAP_TOOLS[r.below(MAP_TOOLS.len())];
            TileCmd::Stroke { tool, path }.into()
        }
    }
}

/// A change to layers, frames or cels that maps take part in.
fn structure_cmd(e: &Editor, r: &mut Rng) -> Command {
    let (layers, frames) = (e.doc().layers().len(), e.doc().frames.len());
    let layer = e.layer_id(e.layer).expect("the active layer");
    let (a, b) = (r.below(frames), r.below(frames));
    let (from, to) = (a.min(b), a.max(b));
    match r.below(12) {
        0 => LayerCmd::Duplicate { layer }.into(),
        1 => LayerCmd::Remove { layer }.into(),
        2 => LayerCmd::MergeDown { layer }.into(),
        3 => LayerCmd::Step {
            layer,
            up: r.coin(),
        }
        .into(),
        4 => TileCmd::CopyTilesetFor { layer }.into(),
        5 => FrameCmd::Add {
            at: r.below(frames + 1),
            copy: r.coin(),
        }
        .into(),
        6 => FrameCmd::Remove { from, to }.into(),
        7 => FrameCmd::HoldAcross {
            layer,
            source: from,
            from,
            to,
        }
        .into(),
        8 => FrameCmd::Duplicate { from, to }.into(),
        9 => CanvasCmd::Resize {
            width: 8 + r.below(24),
            height: 8 + r.below(16),
            dx: r.below(9) as i32 - 4,
            dy: r.below(9) as i32 - 4,
        }
        .into(),
        _ => CelCmd::Drag(CelDrag {
            layers: (e.layer, e.layer),
            frames: (e.frame, e.frame),
            to_layer: r.below(layers),
            to_frame: r.below(frames),
            kind: [DragKind::Link, DragKind::Copy, DragKind::Move][r.below(3)],
            landing: Landing::Overwrite,
        })
        .into(),
    }
}

fn random_cmd(e: &mut Editor, r: &mut Rng) -> Command {
    e.layer = r.below(e.doc().layers().len());
    e.frame = r.below(e.doc().frames.len());
    let Some(set) = some_set(e, r) else {
        return new_set_cmd(r);
    };
    match r.below(10) {
        0 | 1 => tileset_cmd(e, set, r),
        2 => terrain_cmd(e, set, r),
        3 => TileCmd::NewLayer {
            at: r.below(e.doc().layers().len() + 1),
            tileset: set,
        }
        .into(),
        4..=6 => stroke_cmd(e, r),
        _ => structure_cmd(e, r),
    }
}

fn rules_hold(doc: &Document, after: &str) -> Result<(), String> {
    doc.check_tilemaps()
        .map_err(|f| format!("{f} after {after}"))?;
    doc.check_links().map_err(|f| format!("{f} after {after}"))
}

/// Runs one random command; true when it became a step.
fn step(
    e: &mut Editor,
    r: &mut Rng,
    states: &mut HashMap<usize, Document>,
) -> Result<bool, String> {
    let command = random_cmd(e, r);
    let pushes = e.history().pushes();
    let what = format!("{command:?}");
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = e.apply(command);
    }));
    ran.map_err(|_| format!("panicked on {what}"))?;
    rules_hold(e.doc(), &what)?;
    states.insert(e.history().undo_depth(), e.doc().clone());
    Ok(e.history().pushes() != pushes)
}

/// Undoes every step, each document matching the one made at that depth, then redoes them all.
fn walk_back_and_forth(e: &mut Editor, states: &HashMap<usize, Document>) -> Result<(), String> {
    let last = e.doc().clone();
    let start = states.keys().min().copied().unwrap_or(0);
    while e.history().undo_depth() > start {
        if e.apply(StepCmd::Undo).map(|o| o.changed) != Ok(true) {
            break;
        }
        rules_hold(e.doc(), "undo")?;
        let depth = e.history().undo_depth();
        if states.get(&depth) != Some(e.doc()) {
            return Err(format!("undo to depth {depth} differs"));
        }
    }
    while e.apply(StepCmd::Redo).map(|o| o.changed) == Ok(true) {
        rules_hold(e.doc(), "redo")?;
    }
    match e.doc() == &last {
        true => Ok(()),
        false => Err("redoing everything differs from the last document".into()),
    }
}

/// One session; how many of its commands became steps.
fn run_seed(seed: u64) -> Result<usize, String> {
    let mut r = Rng::new(seed);
    let mut e = session();
    rules_hold(e.doc(), "the session was made")?;
    let mut states = HashMap::new();
    states.insert(e.history().undo_depth(), e.doc().clone());
    let mut made = 0;
    for n in 0..STEPS {
        let stepped = step(&mut e, &mut r, &mut states);
        made += usize::from(stepped.map_err(|f| format!("seed {seed} step {n}: {f}"))?);
    }
    walk_back_and_forth(&mut e, &states).map_err(|f| format!("seed {seed}: {f}"))?;
    Ok(made)
}

fn run_seeds(count: u64) {
    let runs: Vec<Result<usize, String>> = (1..=count).map(run_seed).collect();
    let made: usize = runs.iter().flatten().sum();
    // Most commands are allowed, so a change that makes them all refused fails here.
    assert!(made as u64 > count * STEPS as u64 / 3, "only {made} steps");
    let failures: Vec<&String> = runs.iter().filter_map(|r| r.as_ref().err()).collect();
    assert!(
        failures.is_empty(),
        "{} of {count} seeds failed, first: {:?}",
        failures.len(),
        failures.first()
    );
}

#[test]
fn random_sessions_keep_the_tilemaps() {
    run_seeds(QUICK_SEEDS);
}

#[test]
#[ignore = "long: 2,000 sessions, or PIKSLAR_TILE_SEEDS"]
fn many_random_sessions_keep_the_tilemaps() {
    let count = std::env::var("PIKSLAR_TILE_SEEDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(LONG_SEEDS);
    run_seeds(count);
}
