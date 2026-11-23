//! Tilemaps in the random runs (M10): tilesets of both colour modes and several tile sizes,
//! square and not; tiles added, duplicated, removed, moved and redrawn; tilemap layers, copies
//! of a tileset for one layer, map strokes with every tool and turned stamps, Delete under the
//! selection, pixel strokes through the canvas onto tiles, holds and links between maps, terrain
//! sets and terrain strokes, and saves reopened mid-run. Each is one step or nothing, undoes and
//! redoes to the very document it made, and leaves every rule of the maps kept.

use super::{bad, Model};
use crate::go;
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{
    rgba, CelCmd, CelDrag, ColorMode, DragKind, FrameCmd, Grid, Landing, Mask, SelectOp,
    SelectionCmd, TileCmd, TileSource, TileTool, Tool, Turn,
};
use std::sync::Arc;

/// Tile sides the runs choose from: small, so a 32 pixel canvas holds a map of several cells.
const SIDES: [u16; 4] = [2, 3, 4, 8];
const TURNS: [Turn; 4] = [
    Turn::FlipHorizontal,
    Turn::FlipVertical,
    Turn::Clockwise,
    Turn::CounterClockwise,
];
const MAP_TOOLS: [TileTool; 6] = [
    TileTool::Place,
    TileTool::Erase,
    TileTool::Fill,
    TileTool::Line,
    TileTool::Rect,
    TileTool::Pick,
];
/// Enough tiles that maps can vary, few enough that runs keep moving.
const MOST_TILES: usize = 10;

impl Model {
    /// One tilemap action, chosen at random. Whatever happened, the maps keep their rules.
    pub(super) fn tile_step(&mut self) -> Result<(), String> {
        let pushes = self.editor.history().pushes();
        self.one_tile_action()?;
        self.counts.tiles += u64::from(self.editor.history().pushes() != pushes);
        self.tilemaps_kept("a tilemap action")
    }

    fn one_tile_action(&mut self) -> Result<(), String> {
        match self.rng.below(22) {
            0 | 1 => self.edit("a new tileset", Self::new_tileset),
            2 => self.edit("renaming a tileset", Self::rename_tileset),
            3 => self.edit("removing a tileset", Self::remove_tileset),
            4 | 5 => self.add_tile(),
            6 => self.edit("tile order", Self::reorder_tiles),
            7 => self.edit("removing a tile", Self::remove_tile),
            8 => self.edit("redrawing a tile", Self::set_tile_image),
            9 | 10 => self.edit("a tilemap layer", Self::new_map_layer),
            11 => self.edit("a tileset of its own", Self::copy_tileset),
            12..=14 => self.counted_map_stroke(),
            15 => self.delete_cells(),
            16 | 17 => self.pixels_through_canvas(),
            18 => self.edit("a hold or link on a map", Self::map_frames),
            19 => self.terrain_set_step(),
            20 => self.counted_terrain_stroke(),
            _ => {
                self.check_save()?;
                self.counts.saves += 1;
                Ok(())
            }
        }
    }

    pub(super) fn some_tileset(&mut self) -> Option<TilesetId> {
        let sets = &self.editor.doc().tilesets;
        let ids: Vec<TilesetId> = sets.iter().map(|s| s.id()).collect();
        (!ids.is_empty()).then(|| ids[self.rng.below(ids.len())])
    }

    pub(super) fn some_tile(&mut self, set: TilesetId) -> Option<TileId> {
        let tileset = self.editor.doc().tilesets.get(set)?;
        let ids: Vec<TileId> = tileset.tiles().iter().map(|t| t.id).collect();
        (!ids.is_empty()).then(|| ids[self.rng.below(ids.len())])
    }

    /// A tilemap layer, made active, after making what the document lacks for one, each its own
    /// step: a tileset, two tiles and a layer. Without this, few runs of 80 steps would gather
    /// all three by chance before the map tools came up.
    pub(super) fn ready_map_layer(&mut self) -> Result<Option<usize>, String> {
        if self.editor.doc().tilesets.is_empty() {
            self.edit("a first tileset", Self::new_tileset)?;
        }
        let Some(set) = self.some_tileset() else {
            return Ok(None);
        };
        let tiles = self.editor.doc().tilesets.get(set).map_or(0, |s| s.len());
        for _ in tiles..2 {
            let Some(image) = self.tile_pixels(set) else {
                break;
            };
            let from = TileSource::Image(image);
            self.edit("a first tile", |m| {
                go(&mut m.editor, TileCmd::AddTile { tileset: set, from })
            })?;
        }
        if !self.editor.doc().layers().iter().any(|l| l.is_tilemap()) {
            self.edit("a first tilemap layer", |m| {
                let at = m.editor.doc().layers().len();
                go(&mut m.editor, TileCmd::NewLayer { at, tileset: set });
            })?;
        }
        Ok(self.some_map_layer())
    }

    /// A tilemap layer, made active, if the document has one.
    pub(super) fn some_map_layer(&mut self) -> Option<usize> {
        let layers = self.editor.doc().layers();
        let maps: Vec<usize> = (0..layers.len())
            .filter(|&i| layers[i].is_tilemap())
            .collect();
        let index = *maps.get(self.rng.below(maps.len().max(1)))?;
        self.editor.layer = index;
        Some(index)
    }

    fn new_tileset(&mut self) {
        let mode = [ColorMode::Rgba, ColorMode::Palette][self.rng.below(2)];
        let tile_w = SIDES[self.rng.below(SIDES.len())];
        let tile_h = match self.rng.chance(3) {
            true => SIDES[self.rng.below(SIDES.len())],
            false => tile_w,
        };
        let name = format!("set {}", self.rng.below(9));
        go(
            &mut self.editor,
            TileCmd::NewTileset {
                name,
                mode,
                tile_w,
                tile_h,
            },
        );
    }

    fn rename_tileset(&mut self) {
        let Some(tileset) = self.some_tileset() else {
            return;
        };
        for _ in 0..1 + self.rng.below(3) {
            let name = format!("named {}", self.rng.below(5));
            go(&mut self.editor, TileCmd::RenameTileset { tileset, name });
        }
    }

    fn remove_tileset(&mut self) {
        if let Some(tileset) = self.some_tileset() {
            // Refused while a layer uses it.
            go(&mut self.editor, TileCmd::RemoveTileset { tileset });
        }
    }

    /// Pixels for a tile of `set`, sometimes smaller than the tile, with clear places.
    fn tile_pixels(&mut self, set: TilesetId) -> Option<Arc<pikslar_core::Image>> {
        let tileset = self.editor.doc().tilesets.get(set)?;
        let ((w, h), mode) = (tileset.tile_size(), tileset.mode());
        let (w, h) = (1 + self.rng.below(w), 1 + self.rng.below(h));
        let colors = self.editor.doc().colors.len();
        let cells: Vec<u32> = (0..w * h)
            .map(|_| match (mode, self.rng.below(4)) {
                (_, 0) => 0,
                (ColorMode::Palette, _) => self.rng.below(colors) as u32,
                (ColorMode::Rgba, _) => rgba(self.rng.next_u64() as u8, 90, 200, 255),
            })
            .collect();
        Some(Arc::new(Grid::from_cells(w, h, &cells).into_image()))
    }

    /// A tile from nothing, from pixels, or cut from the selection, which is made first as a
    /// step of its own.
    fn add_tile(&mut self) -> Result<(), String> {
        let Some(tileset) = self.some_tileset() else {
            return Ok(());
        };
        let sets = &self.editor.doc().tilesets;
        if sets.get(tileset).map_or(0, |s| s.len()) >= MOST_TILES {
            return Ok(());
        }
        let from = match self.rng.below(4) {
            0 => TileSource::Blank,
            1 => {
                self.selecting()?;
                TileSource::Selection
            }
            _ => match self.tile_pixels(tileset) {
                Some(image) => TileSource::Image(image),
                None => return Ok(()),
            },
        };
        self.edit("a new tile", |m| {
            go(&mut m.editor, TileCmd::AddTile { tileset, from })
        })
    }

    fn reorder_tiles(&mut self) {
        let Some(tileset) = self.some_tileset() else {
            return;
        };
        let Some(tile) = self.some_tile(tileset) else {
            return;
        };
        let command = match self.rng.below(2) {
            0 => TileCmd::DuplicateTile { tileset, tile },
            _ => TileCmd::MoveTile {
                tileset,
                tile,
                to: self.rng.below(MOST_TILES),
            },
        };
        go(&mut self.editor, command);
    }

    fn remove_tile(&mut self) {
        let Some(tileset) = self.some_tileset() else {
            return;
        };
        if let Some(tile) = self.some_tile(tileset) {
            go(&mut self.editor, TileCmd::RemoveTile { tileset, tile });
        }
    }

    fn set_tile_image(&mut self) {
        let Some(tileset) = self.some_tileset() else {
            return;
        };
        let (Some(tile), Some(image)) = (self.some_tile(tileset), self.tile_pixels(tileset)) else {
            return;
        };
        let command = TileCmd::SetTileImage {
            tileset,
            tile,
            image,
        };
        go(&mut self.editor, command);
    }

    fn new_map_layer(&mut self) {
        if let Some(tileset) = self.some_tileset() {
            let at = self.rng.below(self.editor.doc().layers().len() + 1);
            go(&mut self.editor, TileCmd::NewLayer { at, tileset });
        }
    }

    fn copy_tileset(&mut self) {
        if let Some(index) = self.some_map_layer() {
            let layer = self.editor.layer_id(index).expect("layer");
            go(&mut self.editor, TileCmd::CopyTilesetFor { layer });
        }
    }

    fn select_rect(&mut self) {
        let (x, y) = self.point();
        let (w, h) = (1 + self.rng.below(14) as i32, 1 + self.rng.below(14) as i32);
        let shape = Box::new(Mask::rect(x, y, w, h));
        let op = SelectOp::Replace;
        go(&mut self.editor, SelectionCmd::Select { shape, op });
    }

    /// A selection, as a step of its own.
    pub(super) fn selecting(&mut self) -> Result<(), String> {
        self.edit("selecting", Self::select_rect)
    }

    fn unselecting(&mut self) -> Result<(), String> {
        self.edit("clearing the selection", |m| {
            go(&mut m.editor, SelectionCmd::None)
        })
    }

    /// A stroke of a map tool on a tilemap layer: whole, pressed and dragged, or cancelled.
    fn counted_map_stroke(&mut self) -> Result<(), String> {
        let Some(index) = self.ready_map_layer()? else {
            return Ok(());
        };
        let Some(set) = self.editor.doc().layers()[index].tileset() else {
            return Ok(());
        };
        self.choose_stamp(set)?;
        if self.rng.chance(4) {
            self.selecting()?;
        }
        let tool = MAP_TOOLS[self.rng.below(MAP_TOOLS.len())];
        let pushes = self.editor.history().pushes();
        self.edit("a map stroke", |m| m.drive_stroke(tool))?;
        self.counts.map_strokes += u64::from(self.editor.history().pushes() != pushes);
        Ok(())
    }

    /// Chooses a stamp of the layer's tileset: one tile, a block, or cells captured under a
    /// selection, then turned now and then. Only the selection is a step.
    pub(super) fn choose_stamp(&mut self, set: TilesetId) -> Result<(), String> {
        match self.rng.below(5) {
            0 => {
                let (w, h) = (1 + self.rng.below(3), 1 + self.rng.below(3));
                let tiles: Vec<Option<TileId>> = (0..w * h)
                    .map(|_| match self.rng.chance(4) {
                        true => None,
                        false => self.some_tile(set),
                    })
                    .collect();
                let _ = self.editor.set_tile_block(set, w, h, &tiles);
            }
            1 => {
                self.selecting()?;
                let _ = self.editor.capture_tile_stamp();
                self.unselecting()?;
            }
            _ => match self.some_tile(set) {
                Some(tile) => drop(self.editor.set_current_tile(set, tile)),
                None => self.editor.clear_tile_stamp(),
            },
        }
        for _ in 0..self.rng.below(3) {
            let turn = TURNS[self.rng.below(TURNS.len())];
            let _ = self.editor.turn_tile_stamp(turn);
        }
        Ok(())
    }

    /// Runs a map stroke of `tool` along a few random points, one of three ways.
    pub(super) fn drive_stroke(&mut self, tool: TileTool) {
        let path: Vec<(i32, i32)> = (0..1 + self.rng.below(4)).map(|_| self.point()).collect();
        if self.rng.chance(2) {
            go(&mut self.editor, TileCmd::Stroke { tool, path });
            return;
        }
        let (x, y) = path[0];
        if self.editor.apply(TileCmd::Begin { tool, x, y }).is_err() {
            return;
        }
        for &(x, y) in &path[1..] {
            go(&mut self.editor, TileCmd::To { x, y });
        }
        let end = match self.rng.chance(5) {
            true => TileCmd::Cancel,
            false => TileCmd::Commit,
        };
        go(&mut self.editor, end);
    }

    fn delete_cells(&mut self) -> Result<(), String> {
        if self.ready_map_layer()?.is_none() {
            return Ok(());
        }
        self.selecting()?;
        self.edit("clearing selected cells", |m| {
            go(&mut m.editor, TileCmd::DeleteSelected)
        })?;
        self.unselecting()
    }

    /// Pixel tools on a tilemap layer, which draw on the tiles its cells place.
    fn pixels_through_canvas(&mut self) -> Result<(), String> {
        let Some(index) = self.ready_map_layer()? else {
            return Ok(());
        };
        if self.editor.doc().layers()[index]
            .cel(self.editor.frame)
            .is_blank()
        {
            // Pixels land only on placed cells, so a blank map first gets some.
            self.fill_map(index)?;
        }
        let tool = [Tool::Pencil, Tool::Eraser, Tool::Line, Tool::Rect][self.rng.below(4)];
        let pushes = self.editor.history().pushes();
        self.edit("drawing on tiles", |m| m.stroke(tool))?;
        self.counts.tile_pixels += u64::from(self.editor.history().pushes() != pushes);
        Ok(())
    }

    /// Fills the map layer's cel with one of its tiles, as a step of its own.
    fn fill_map(&mut self, index: usize) -> Result<(), String> {
        let Some(set) = self.editor.doc().layers()[index].tileset() else {
            return Ok(());
        };
        let Some(tile) = self.some_tile(set) else {
            return Ok(());
        };
        if self.editor.set_current_tile(set, tile).is_err() {
            return Ok(());
        }
        let path = vec![self.point()];
        self.edit("a filled map", |m| {
            go(
                &mut m.editor,
                TileCmd::Stroke {
                    tool: TileTool::Fill,
                    path,
                },
            )
        })
    }

    /// A hold on a map layer, a new frame that copies it, or a link to another map layer.
    fn map_frames(&mut self) {
        let Some(index) = self.some_map_layer() else {
            return;
        };
        let layer = self.editor.layer_id(index).expect("layer");
        let frames = self.editor.doc().frames.len();
        let (a, b) = (self.rng.below(frames), self.rng.below(frames));
        let at = self.editor.frame;
        let (from, to) = (a.min(b), a.max(b));
        match self.rng.below(3) {
            0 => go(
                &mut self.editor,
                FrameCmd::HoldAcross {
                    layer,
                    source: from,
                    from,
                    to,
                },
            ),
            1 => go(&mut self.editor, FrameCmd::Add { at, copy: true }),
            _ => self.link_maps(index),
        }
    }

    /// Links a cel of the map layer to another layer, as an Alt drag does; refused unless both
    /// are maps of one tileset.
    fn link_maps(&mut self, index: usize) {
        let layers = self.editor.doc().layers().len();
        let frames = self.editor.doc().frames.len();
        let drag = CelDrag {
            layers: (index, index),
            frames: (self.editor.frame, self.editor.frame),
            to_layer: self.rng.below(layers),
            to_frame: self.rng.below(frames),
            kind: DragKind::Link,
            landing: Landing::Overwrite,
        };
        go(&mut self.editor, CelCmd::Drag(drag));
    }

    /// The rules every document's maps keep (T1 to T6), as an error naming the step.
    pub(super) fn tilemaps_kept(&self, step: &str) -> Result<(), String> {
        match self.editor.doc().check_tilemaps() {
            Ok(()) => Ok(()),
            Err(fault) => bad(format!("after {step}: tilemaps {fault:?}")),
        }
    }
}
