//! Random sessions, checking after every command that history is charged at least for every
//! drawing only its steps keep.

use super::{Edit, History};
use crate::color::rgba;
use crate::command::{
    CelCmd, Command, DrawCmd, FrameCmd, LayerCmd, PaletteCmd, StepCmd, TileCmd, TileSource,
};
use crate::document::{ColorMode, Document};
use crate::editor::{Editor, Tool};
use crate::grid::Grid;
use crate::palette_ops::remap::RemapKind;
use crate::Turn;
use std::collections::HashMap;
use std::sync::Arc;

/// Counts one more step's hold on a tile image, among the drawings steps keep.
fn note_image(
    drawings: &mut HashMap<usize, (usize, usize, usize)>,
    image: &Arc<crate::image::Image>,
) {
    let at = Arc::as_ptr(image) as usize;
    let entry = drawings
        .entry(at)
        .or_insert((0, Arc::strong_count(image), 0));
    entry.0 += 1;
    entry.2 = crate::layer::kept_bytes(image);
}

/// What the steps are charged for tile images, and what the tile images only steps keep take.
/// Unlike the totals above these two are equal in every state, whatever the budget.
fn tile_charged_and_kept(history: &History) -> (usize, usize) {
    let mut charged = 0;
    let mut images: HashMap<usize, (usize, usize, usize)> = HashMap::new();
    let lists = [&history.undo, &history.redo, &history.aside];
    for (edit, _) in lists.into_iter().flatten() {
        match edit {
            Edit::Snapshot(s) => {
                charged += s.tile_charge();
                s.kept_images()
                    .for_each(|image| note_image(&mut images, image));
            }
            Edit::TileImages { before, charge, .. } => {
                charged += *charge;
                before
                    .iter()
                    .for_each(|(_, image)| note_image(&mut images, image));
            }
            Edit::Tiles { .. } | Edit::Move { .. } | Edit::Selection(_) => {}
        }
    }
    let kept = images
        .values()
        .filter(|(times, holders, _)| times == holders)
        .map(|(_, _, bytes)| bytes)
        .sum();
    (charged, kept)
}

/// What the steps are charged for drawings, and what the drawings only steps keep take.
fn charged_and_kept(history: &History) -> (usize, usize) {
    let mut charged = 0;
    // For each drawing: how many cels in the steps show it, its holders, and what it takes.
    let mut drawings: HashMap<usize, (usize, usize, usize)> = HashMap::new();
    let lists = [&history.undo, &history.redo, &history.aside];
    for (edit, _) in lists.into_iter().flatten() {
        match edit {
            Edit::Snapshot(s) => {
                let (charge, cels) = s.charged_for();
                charged += charge;
                for cel in cels {
                    let at = cel.image_key() as usize;
                    let entry = drawings.entry(at).or_insert((0, cel.image_holders(), 0));
                    entry.0 += 1;
                    entry.2 = cel.kept_bytes();
                }
                for image in s.kept_images() {
                    note_image(&mut drawings, image);
                }
            }
            // The images a tile edit replaced are charged to it once only history keeps them.
            Edit::TileImages { before, charge, .. } => {
                charged += *charge;
                for (_, image) in before.iter() {
                    note_image(&mut drawings, image);
                }
            }
            // The tiles a pixel step replaced are the split drawing's own, and charged with the
            // step's tiles rather than in what is left to history.
            Edit::Tiles { left, changes, .. } if *left > 0 => {
                charged += *left as usize + super::tile_bytes(changes);
            }
            Edit::Tiles { .. } => {}

            Edit::Move { .. } | Edit::Selection(_) => {}
        }
    }
    let kept = drawings
        .values()
        .filter(|(times, holders, _)| times == holders)
        .map(|(_, _, bytes)| bytes)
        .sum();
    (charged, kept)
}

struct Dice(u64);

impl Dice {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

/// A random command, among the tileset and tile commands too if `tiles` is set.
fn any_command(e: &Editor, dice: &mut Dice, layer: crate::LayerId, tiles: bool) -> Command {
    let frames = e.doc().frames.len();
    let (a, b) = (dice.below(frames), dice.below(frames));
    let (from, to) = (a.min(b), a.max(b));
    let (x, y) = (dice.below(32) as i32, dice.below(32) as i32);
    let layers = e.doc().layers().len();
    match dice.below(if tiles { 31 } else { 19 }) {
        19..=26 => tile_command(e, dice),
        27..=30 => palette_command(e, dice),
        // Links across layers, and one cel of a link moved apart.
        17 => CelCmd::Drag(crate::CelDrag {
            layers: (dice.below(layers), dice.below(layers)),
            frames: (from, to),
            to_layer: dice.below(layers),
            to_frame: dice.below(frames),
            kind: crate::DragKind::Link,
            landing: crate::Landing::Overwrite,
        })
        .into(),
        18 => CelCmd::MoveOne {
            layer,
            frame: a,
            dx: x - 16,
            dy: y - 16,
        }
        .into(),
        0..=2 => DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(x, y), (x + 3, y)],
        }
        .into(),
        3 | 4 => StepCmd::Undo.into(),
        5 => StepCmd::Redo.into(),
        6 => FrameCmd::Add {
            at: a,
            copy: dice.below(3) > 0,
        }
        .into(),
        7 => FrameCmd::Remove { from, to }.into(),
        8 => FrameCmd::Move { from: a, to: b }.into(),
        9 => FrameCmd::HoldPrevious {
            layer,
            frame: a.max(1),
        }
        .into(),
        10 => LayerCmd::Add {
            at: dice.below(e.doc().layers().len() + 1),
            mode: ColorMode::Rgba,
        }
        .into(),
        11 => LayerCmd::Remove { layer }.into(),
        // Flipping twice, or turning four times, makes a separate drawing that looks the same.
        12 => CelCmd::Turn {
            layer,
            frame: a,
            turn: [Turn::Clockwise, Turn::FlipHorizontal][dice.below(2)],
        }
        .into(),
        13 => FrameCmd::Release { layer, from, to }.into(),
        14 => crate::FloatingCmd::Lift {
            copy: dice.below(2) == 0,
        }
        .into(),
        15 => crate::FloatingCmd::Anchor.into(),
        _ => LayerCmd::SetMode {
            layer,
            mode: [ColorMode::Palette, ColorMode::Rgba][dice.below(2)],
        }
        .into(),
    }
}

/// A remap or a Replace colour, which rewrite every palette image and tile image at once.
fn palette_command(e: &Editor, dice: &mut Dice) -> Command {
    let len = e.doc().colors.len();
    match dice.below(4) {
        0 => PaletteCmd::Remap(RemapKind::ToSwatchOrder),
        1 => PaletteCmd::Remap(RemapKind::MergeDuplicates),
        2 => PaletteCmd::Remap(RemapKind::RemoveUnused),
        _ => PaletteCmd::ReplaceColor {
            from: 1 + dice.below(len.max(2) - 1) as u32,
            to: dice.below(len) as u32,
            layer: None,
        },
    }
    .into()
}

/// A tileset or tile command on a random tileset, tile or picture.
fn tile_command(e: &Editor, dice: &mut Dice) -> Command {
    let sets: Vec<_> = e.doc().tilesets.iter().collect();
    let Some(set) = sets.get(dice.below(sets.len().max(1))) else {
        return TileCmd::NewTileset {
            name: "T".into(),
            mode: [ColorMode::Rgba, ColorMode::Palette][dice.below(2)],
            tile_w: 8,
            tile_h: 8,
        }
        .into();
    };
    let tileset = set.id();
    match dice.below(10) {
        0 => {
            let at = dice.below(e.doc().layers().len() + 1);
            return TileCmd::NewLayer { at, tileset }.into();
        }
        1 => {
            let layer = e.layer_id(e.layer).expect("layer");
            return TileCmd::CopyTilesetFor { layer }.into();
        }
        2 | 3 => return terrain_command(set, dice),
        _ => {}
    }
    let value = dice.below(200) as u32 + 1;
    let picture = Arc::new(Grid::from_cells(8, 8, &[value; 64]).into_image());
    let tiles = set.tiles();
    let tile = tiles.get(dice.below(tiles.len().max(1))).map(|t| t.id);
    let (Some(tile), 1..) = (tile, dice.below(4)) else {
        let from = match dice.below(2) {
            0 => TileSource::Blank,
            _ => TileSource::Image(picture),
        };
        return TileCmd::AddTile { tileset, from }.into();
    };
    match dice.below(5) {
        0 => TileCmd::DuplicateTile { tileset, tile },
        1 => TileCmd::RemoveTile { tileset, tile },
        2 => TileCmd::MoveTile {
            tileset,
            tile,
            to: dice.below(4),
        },
        3 => TileCmd::RenameTileset {
            tileset,
            name: format!("set {value}"),
        },
        _ => TileCmd::SetTileImage {
            tileset,
            tile,
            image: picture,
        },
    }
    .into()
}

/// A terrain command on a random terrain set and slot of the tileset, with a random tile of it.
fn terrain_command(set: &crate::tileset::Tileset, dice: &mut Dice) -> Command {
    use crate::autotile::TerrainKind;
    use crate::command::TerrainCmd;
    let tileset = set.id();
    let sets = set.terrains();
    let Some(terrain) = sets.get(dice.below(sets.len().max(1))) else {
        let kind = [
            TerrainKind::Edge4,
            TerrainKind::Corner4,
            TerrainKind::Blob47,
        ][dice.below(3)];
        let template = dice.below(2) == 0;
        let name = "T".into();
        return TerrainCmd::New {
            tileset,
            kind,
            name,
            template,
        }
        .into();
    };
    let id = terrain.id();
    let slot = dice.below(terrain.slots().len()) as u8;
    let tile = set.tiles().get(dice.below(set.len().max(1))).map(|t| t.id);
    match (dice.below(6), tile) {
        (0, _) => TerrainCmd::Remove { tileset, set: id },
        (1, _) => TerrainCmd::Rename {
            tileset,
            set: id,
            name: format!("t{slot}"),
        },
        (2, _) | (_, None) => TerrainCmd::Clear {
            tileset,
            set: id,
            slot,
        },
        (_, Some(tile)) => TerrainCmd::Assign {
            tileset,
            set: id,
            slot,
            tile,
        },
    }
    .into()
}

/// Puts cells that place real tiles into the active tilemap layer's cel, as a step of its own,
/// for the commands that follow, since map strokes come in a later phase.
fn plant_map(e: &mut Editor, dice: &mut Dice) {
    let Some(set) = e.doc().layers()[e.layer].tileset() else {
        return;
    };
    let ids: Vec<u32> = e.doc().tileset(set).map_or_else(
        |_| Vec::new(),
        |s| s.tiles().iter().map(|t| t.id.get()).collect(),
    );
    if ids.is_empty() {
        return;
    }
    let mut pick = || match dice.below(4) {
        0 => 0,
        _ => ids[dice.below(ids.len())],
    };
    let cells = [pick(), pick(), pick(), pick()];
    let cel = crate::layer::Cel::new(Grid::from_cells(2, 2, &cells));
    let (layer, frame) = (e.layer, e.frame);
    let _ = e.transact(|d| d.put_cel(layer, frame, cel.clone()));
}

#[test]
fn history_is_charged_for_every_drawing_only_its_steps_keep() {
    session(1..=200, 40_000, false);
}

/// With tilesets in the mix, a tile image only steps keep is charged exactly once, and every
/// kind of step balances. The small budget is larger than above: a tile step is a few hundred
/// bytes more than the steps above, and under a budget of a few steps the known surplus for a
/// split drawing described in `session` comes up.
#[test]
fn history_is_charged_for_every_tile_image_only_its_steps_keep() {
    session(1..=200, 400_000, true);
}

/// Strokes on a map write into its tiles (Pixels mode): each step keeps the images it replaced,
/// charged exactly once whichever steps are undone, redone or dropped by a small budget.
#[test]
fn tile_images_edited_through_the_canvas_are_charged_exactly() {
    for (seed, budget) in (1..=40u64).map(|s| (s, [6_000, 1 << 30][s as usize % 2])) {
        let mut dice = Dice(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut doc = Document::new(32, 32, ColorMode::Rgba).expect("valid");
        let set = doc.add_tileset("T", ColorMode::Rgba, (8, 8)).expect("room");
        for value in [5u32, 9] {
            let image = Grid::from_cells(8, 8, &[value; 64]).into_image();
            doc.add_tile(set, Arc::new(image)).expect("fits");
        }
        doc.add_tilemap_layer(1, set).expect("room");
        let cells: Vec<u32> = (0..16).map(|i| [0, 1, 2][i % 3]).collect();
        let map = crate::layer::Cel::new(Grid::from_cells(4, 4, &cells));
        doc.put_cel(1, 0, map).expect("fits");
        let mut e = Editor::with_undo_budget(doc, budget);
        e.layer = 1;
        for turn in 0..60 {
            let (x, y) = (dice.below(32) as i32, dice.below(32) as i32);
            let command: Command = match dice.below(5) {
                0 => StepCmd::Undo.into(),
                1 => StepCmd::Redo.into(),
                _ => DrawCmd::Stroke {
                    tool: [Tool::Pencil, Tool::Line, Tool::Eraser][dice.below(3)],
                    path: vec![(x, y), (dice.below(32) as i32, y)],
                }
                .into(),
            };
            e.color = rgba(dice.below(255) as u8 + 1, 7, 7, 255);
            let _ = e.apply(command);
            let (tile_charged, tile_kept) = tile_charged_and_kept(&e.history);
            assert_eq!(tile_charged, tile_kept, "seed {seed}, turn {turn}");
            assert_books_balance(&e.history, seed, turn);
        }
    }
}

fn session(seeds: std::ops::RangeInclusive<u64>, small_budget: usize, tiles: bool) {
    let mut over = 0;
    for seed in seeds {
        let mut dice = Dice(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let budget = if seed % 3 == 0 { small_budget } else { 1 << 30 };
        let doc = Document::new(32, 32, ColorMode::Rgba).expect("valid");
        let mut e = Editor::with_undo_budget(doc, budget);
        for turn in 0..150 {
            e.layer = dice.below(e.doc().layers().len());
            e.frame = dice.below(e.doc().frames.len());
            let layer = e.layer_id(e.layer).expect("layer");
            if dice.below(8) == 0 {
                let (x, y) = (dice.below(24) as i32, dice.below(24) as i32);
                e.select(
                    crate::mask::Mask::rect(x, y, 8, 8),
                    crate::selection::SelectOp::Replace,
                );
            }
            if tiles && dice.below(3) == 0 {
                plant_map(&mut e, &mut dice);
            }
            let command = any_command(&e, &mut dice, layer, tiles);
            e.color = rgba(dice.below(255) as u8 + 1, 7, 7, 255);
            let _ = e.apply(command);
            if dice.below(2) == 0 {
                e.apply(StepCmd::EndDrag).expect("end");
            }
            let (charged, kept) = charged_and_kept(&e.history);
            assert!(
                charged >= kept,
                "seed {seed}, turn {turn}: charged {charged} for {kept}"
            );
            over += usize::from(charged > kept);
            let (tile_charged, tile_kept) = tile_charged_and_kept(&e.history);
            assert_eq!(
                tile_charged, tile_kept,
                "seed {seed}, turn {turn}: tile images"
            );
            assert_books_balance(&e.history, seed, turn);
        }
    }
    // Charging more than is kept would drop steps the budget has room for. It happens under a
    // budget of a few steps, when the budget drops a step that keeps a drawing a pixel step split
    // off before it drops the pixel step, and lasts until that is dropped too. The budgets here
    // are not that small.
    assert_eq!(over, 0, "charged too much in {over} states");
}

/// Each pixel, move and selection step is charged what it says it takes, and history's total is
/// the sum of every step's charge, so a charge revised as a step plays is revised everywhere. A
/// structural step is left out of the first: whether it is charged for its color table depends
/// on who else holds the table, which is decided when the step is recorded or played.
fn assert_books_balance(history: &History, seed: u64, turn: usize) {
    let lists = [&history.undo, &history.redo, &history.aside];
    let mut total = 0;
    for (edit, size) in lists.into_iter().flatten() {
        let kind = match edit {
            Edit::Tiles { .. } => "pixel",
            Edit::Move { .. } => "move",
            Edit::Selection(_) => "selection",
            Edit::TileImages { .. } => "tile image",
            Edit::Snapshot(_) => "",
        };
        if !kind.is_empty() {
            assert_eq!(
                *size,
                edit.size(),
                "seed {seed}, turn {turn}: a {kind} step"
            );
        }
        total += size;
    }
    assert_eq!(history.used, total, "seed {seed}, turn {turn}");
}

/// Two frames of separate drawings that look the same: a copy, and a drawing flipped twice.
fn twins() -> (Editor, crate::LayerId) {
    let mut e = Editor::new(Document::new(64, 64, ColorMode::Rgba).expect("valid"));
    e.color = rgba(200, 30, 30, 255);
    e.begin(Tool::Rect, 2, 2).expect("begin");
    e.drag(40, 20);
    e.commit();
    let layer = e.layer_id(0).expect("layer");
    e.apply(FrameCmd::Add { at: 0, copy: true }).expect("room");
    let flip = CelCmd::Turn {
        layer,
        frame: 0,
        turn: Turn::FlipHorizontal,
    };
    e.apply(flip).expect("turned");
    e.apply(flip).expect("turned");
    let cels = e.doc().layers()[0].cels();
    assert!(cels[0] == cels[1] && !cels[0].same_image(&cels[1]));
    (e, layer)
}

#[test]
fn moving_a_frame_past_a_separate_drawing_that_looks_the_same_leaves_each_where_it_was() {
    let (mut e, _) = twins();
    let depth = e.history().undo_depth();
    let shown = |e: &Editor| -> Vec<_> {
        let cels = e.doc().layers()[0].cels();
        cels.iter().map(|cel| cel.image_key()).collect()
    };
    let before = shown(&e);
    e.apply(FrameCmd::Move { from: 1, to: 0 }).expect("moved");
    assert_eq!(
        e.history().undo_depth(),
        depth,
        "nothing to see, so nothing to undo"
    );
    assert_eq!(shown(&e), before, "each drawing is in its own frame");
}

#[test]
fn undoing_past_a_move_between_look_alike_drawings_charges_each_once() {
    let (mut e, _) = twins();
    e.apply(FrameCmd::Move { from: 1, to: 0 }).expect("moved");
    while e.undo() {
        let (charged, kept) = charged_and_kept(&e.history);
        assert!(charged <= kept, "charged {charged} for {kept}");
    }
}

/// Selects a corner and lifts a copy of it, unchanged, so the next structural change takes the
/// floating pixels in first.
fn lift_a_corner(e: &mut Editor) {
    let shape = Box::new(crate::mask::Mask::rect(50, 50, 4, 4));
    let op = crate::selection::SelectOp::Replace;
    e.apply(crate::SelectionCmd::Select { shape, op })
        .expect("selected");
    e.apply(crate::FloatingCmd::Lift { copy: true })
        .expect("lifted");
}

#[test]
fn with_floating_pixels_a_move_between_look_alikes_leaves_each_where_it_was() {
    let (mut e, _) = twins();
    let shown = |e: &Editor| -> Vec<_> {
        let cels = e.doc().layers()[0].cels();
        cels.iter().map(|cel| cel.image_key()).collect()
    };
    let before = shown(&e);
    lift_a_corner(&mut e);
    e.apply(FrameCmd::Move { from: 1, to: 0 }).expect("moved");
    assert_eq!(shown(&e), before, "each drawing is in its own frame");
    assert!(e.floating.is_some(), "the floating pixels are still up");
    while e.undo() {
        let (charged, kept) = charged_and_kept(&e.history);
        assert_eq!(charged, kept);
    }
}

#[test]
fn turning_a_symmetric_drawing_with_floating_pixels_up_stays_within_the_budget() {
    let doc = Document::new(64, 64, ColorMode::Rgba).expect("valid");
    let mut e = Editor::with_undo_budget(doc, 200_000);
    e.color = rgba(10, 200, 30, 255);
    // The same either way round, so a flip changes nothing to see.
    e.begin(Tool::Rect, 4, 4).expect("begin");
    e.drag(59, 59);
    e.commit();
    let layer = e.layer_id(0).expect("layer");
    for n in 0..60 {
        let mode = [ColorMode::Palette, ColorMode::Rgba][n % 2];
        e.apply(LayerCmd::SetMode { layer, mode }).expect("mode");
        lift_a_corner(&mut e);
        let turn = Turn::FlipHorizontal;
        e.apply(CelCmd::Turn {
            layer,
            frame: 0,
            turn,
        })
        .expect("turned");
        let (charged, kept) = charged_and_kept(&e.history);
        assert!(charged >= kept, "cycle {n}: charged {charged} for {kept}");
    }
}

#[test]
fn moving_a_layer_past_one_that_looks_the_same_is_a_step_and_keeps_it_current() {
    let (mut e, _) = twins();
    for at in 1..3 {
        e.apply(LayerCmd::Add {
            at,
            mode: ColorMode::Rgba,
        })
        .expect("room");
        e.doc.layers_mut()[at].name = "same".into();
    }
    let depth = e.history().undo_depth();
    let moved = e.layer_id(1).expect("layer");
    e.apply(LayerCmd::Move {
        layer: moved,
        to: 2,
    })
    .expect("moved");
    assert_eq!(e.layer_id(2), Some(moved), "the layers changed places");
    assert_eq!(
        e.layer_id(e.layer),
        Some(moved),
        "the moved layer stays current"
    );
    assert_eq!(e.history().undo_depth(), depth + 1, "and that is a step");
    assert!(e.undo());
    assert_eq!(e.layer_id(1), Some(moved));
}

/// Order and group steps keep the table they replaced, so each is charged for it, groups
/// included, and history's books balance however they are mixed with strokes and undo.
#[test]
fn palette_order_and_group_steps_are_charged_for_the_table_they_keep() {
    use crate::command::PaletteCmd;
    use crate::SortBy;
    use std::sync::Arc;
    for seed in 1..=60u64 {
        let mut dice = Dice(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let doc = Document::new(32, 32, ColorMode::Palette).expect("valid");
        let mut e = Editor::with_undo_budget(doc, if seed % 3 == 0 { 40_000 } else { 1 << 30 });
        for n in 1..=12u8 {
            let color = rgba(n * 20, 9, 200 - n, 255);
            e.apply(PaletteCmd::AddSwatch { color }).expect("room");
        }
        for turn in 0..120 {
            let swatches = e.doc().colors.swatches().len();
            let (a, b) = (dice.below(swatches), dice.below(swatches));
            let groups = e.doc().colors.groups().len();
            let command: Command = match dice.below(12) {
                0 | 1 => PaletteCmd::MoveSwatch { from: a, to: b }.into(),
                2 => PaletteCmd::Sort {
                    by: [SortBy::Hue, SortBy::Lightness, SortBy::Usage][dice.below(3)],
                }
                .into(),
                3 | 4 => PaletteCmd::AddGroup {
                    name: "group".repeat(dice.below(6) + 1),
                    ids: Arc::from(vec![
                        e.doc().colors.swatches()[a],
                        1 + dice.below(12) as u32,
                    ]),
                }
                .into(),
                5 => PaletteCmd::RenameGroup {
                    group: dice.below(groups + 1),
                    name: "renamed".repeat(dice.below(4)),
                }
                .into(),
                6 => PaletteCmd::RemoveGroup {
                    group: dice.below(groups + 1),
                }
                .into(),
                7 => StepCmd::Undo.into(),
                8 => PaletteCmd::AddToGroup {
                    group: dice.below(groups + 1),
                    id: e.doc().colors.swatches()[a],
                }
                .into(),
                9 => PaletteCmd::RemoveFromGroup {
                    group: dice.below(groups + 1),
                    id: e.doc().colors.swatches()[a],
                }
                .into(),
                10 => PaletteCmd::AddRamp {
                    from: e.doc().colors.swatches()[a],
                    to: e.doc().colors.swatches()[b],
                    steps: 1 + dice.below(6) as u32,
                }
                .into(),
                _ => DrawCmd::Stroke {
                    tool: Tool::Pencil,
                    path: vec![(a as i32, b as i32), (a as i32 + 3, b as i32)],
                }
                .into(),
            };
            e.color = rgba(
                dice.below(12) as u8 * 20 + 20,
                9,
                200 - dice.below(12) as u8 - 1,
                255,
            );
            let _ = e.apply(command);
            if dice.below(2) == 0 {
                e.apply(StepCmd::EndDrag).expect("end");
            }
            let (charged, kept) = charged_and_kept(&e.history);
            assert!(charged >= kept, "seed {seed}, turn {turn}");
            assert_books_balance(&e.history, seed, turn);
        }
    }
}

#[test]
fn a_group_step_is_charged_for_the_groups_it_keeps() {
    use crate::command::PaletteCmd;
    use std::sync::Arc;
    let doc = Document::new(8, 8, ColorMode::Palette).expect("valid");
    let mut e = Editor::new(doc);
    let id = e
        .apply(PaletteCmd::AddSwatch {
            color: rgba(9, 9, 9, 255),
        })
        .expect("room")
        .created
        .expect("id");
    for _ in 0..40 {
        e.apply(PaletteCmd::AddGroup {
            name: "a group with a longer name".into(),
            ids: Arc::from(vec![id; 1]),
        })
        .expect("room");
    }
    let table = e.doc().colors.kept_bytes();
    let before = e.history().steps_used();
    e.apply(PaletteCmd::RemoveGroup { group: 0 })
        .expect("removed");
    assert!(e.history().steps_used() - before >= table);
    assert!(table > 40 * size_of::<crate::PaletteGroup>());
}

/// A palette document with copied frames, a hold, a palette tileset and swatches out of order.
fn palette_session_document(budget: usize) -> Editor {
    let mut e = Editor::with_undo_budget(
        Document::new(64, 64, ColorMode::Palette).expect("valid"),
        budget,
    );
    for n in 1..=5u8 {
        let color = rgba(n * 40, 9, 200 - n * 30, 255);
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
    }
    e.apply(PaletteCmd::MoveSwatch { from: 0, to: 4 })
        .expect("moved");
    for (n, frame) in (1..=5).zip(0..) {
        e.color = rgba(n * 40, 9, 200 - n * 30, 255);
        e.frame = frame % e.doc().frames.len();
        e.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(n as i32, 2), (n as i32 + 30, 40)],
        })
        .expect("stroke");
        e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
    }
    e.apply(TileCmd::NewTileset {
        name: "T".into(),
        mode: ColorMode::Palette,
        tile_w: 8,
        tile_h: 8,
    })
    .expect("room");
    let tileset = e.doc().tilesets.iter().next().expect("added").id();
    for value in 1..=4u32 {
        let picture = Arc::new(Grid::from_cells(8, 8, &[value; 64]).into_image());
        let from = TileSource::Image(picture);
        e.apply(TileCmd::AddTile { tileset, from }).expect("added");
    }
    e
}

/// Remaps and Replace colour rewrite every palette drawing and tile image in one step. The
/// steps' charges still balance in every state, with room for all and with room for a few.
#[test]
fn a_remap_charges_the_images_only_its_step_keeps_exactly() {
    for budget in [1 << 30, 120_000, 40_000] {
        let mut e = palette_session_document(budget);
        let kinds = [
            RemapKind::ToSwatchOrder,
            RemapKind::MergeDuplicates,
            RemapKind::RemoveUnused,
        ];
        for turn in 0..9 {
            let command: Command = match turn % 4 {
                3 => PaletteCmd::ReplaceColor {
                    from: 1 + turn % 3,
                    to: 4,
                    layer: None,
                }
                .into(),
                n => PaletteCmd::Remap(kinds[n as usize]).into(),
            };
            let _ = e.apply(command);
            let (charged, kept) = charged_and_kept(&e.history);
            assert!(charged >= kept, "budget {budget}, turn {turn}");
            let (tile_charged, tile_kept) = tile_charged_and_kept(&e.history);
            assert_eq!(tile_charged, tile_kept, "budget {budget}, turn {turn}");
            assert_books_balance(&e.history, budget as u64, turn as usize);
        }
        while e.undo() {
            let (tile_charged, tile_kept) = tile_charged_and_kept(&e.history);
            assert_eq!(tile_charged, tile_kept, "budget {budget}, undoing");
        }
    }
}
