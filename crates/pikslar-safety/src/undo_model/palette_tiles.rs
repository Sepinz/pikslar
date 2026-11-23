//! A palette tileset and a tilemap layer placing its tiles, built up step by step in the random
//! runs, so that remaps and Replace colour reach tile images as well as cels.

use super::Model;
use crate::go;
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{ColorMode, Grid, LayerKind, TileCmd, TileSource, TileTool};
use std::sync::Arc;

/// Tiles of 4 by 4 pixels: small enough that a 32 pixel canvas holds a map of 8 by 8 cells.
const SIDE: u16 = 4;
/// Enough tiles that a map can place several, few enough that runs keep moving.
const MOST_TILES: usize = 6;

impl Model {
    /// Makes what the document lacks, each its own step: a palette tileset, some tiles, a
    /// layer of it. Then strokes tiles on that layer.
    pub(super) fn palette_tiles(&mut self) -> Result<(), String> {
        if self.palette_tileset().is_none() {
            self.edit("a palette tileset", |m| {
                go(
                    &mut m.editor,
                    TileCmd::NewTileset {
                        name: "P".into(),
                        mode: ColorMode::Palette,
                        tile_w: SIDE,
                        tile_h: SIDE,
                    },
                )
            })?;
        }
        let Some(set) = self.palette_tileset() else {
            return Ok(());
        };
        let tiles = self.editor.doc().tilesets.get(set).map_or(0, |s| s.len());
        let wanted = if tiles < MOST_TILES && self.rng.chance(3) {
            tiles + 1
        } else {
            tiles.max(2)
        };
        for _ in tiles..wanted {
            self.edit("a palette tile", |m| m.add_palette_tile(set))?;
        }
        if self.map_layer(set).is_none() {
            self.edit("a tilemap layer", |m| {
                let at = m.rng.below(m.editor.doc().layers().len() + 1);
                go(&mut m.editor, TileCmd::NewLayer { at, tileset: set });
            })?;
        }
        let Some(index) = self.map_layer(set) else {
            return Ok(());
        };
        let pushes = self.editor.history().pushes();
        self.edit("placing tiles", |m| m.place_tiles(set, index))?;
        self.counts.map_strokes += u64::from(self.editor.history().pushes() != pushes);
        Ok(())
    }

    fn palette_tileset(&self) -> Option<TilesetId> {
        let sets = &self.editor.doc().tilesets;
        let found = sets.iter().find(|s| s.mode() == ColorMode::Palette);
        found.map(|s| s.id())
    }

    fn map_layer(&self, set: TilesetId) -> Option<usize> {
        let layers = self.editor.doc().layers();
        layers
            .iter()
            .position(|l| l.kind == LayerKind::Tilemap(set))
    }

    /// A tile of ids the table has, so that some of its colours are swatches and some are not.
    fn add_palette_tile(&mut self, set: TilesetId) {
        let colors = self.editor.doc().colors.len();
        let area = usize::from(SIDE) * usize::from(SIDE);
        let cells: Vec<u32> = (0..area).map(|_| self.rng.below(colors) as u32).collect();
        let side = usize::from(SIDE);
        let image = Arc::new(Grid::from_cells(side, side, &cells).into_image());
        let from = TileSource::Image(image);
        go(&mut self.editor, TileCmd::AddTile { tileset: set, from });
    }

    /// Chooses one of the tiles and strokes it across the map layer.
    fn place_tiles(&mut self, set: TilesetId, index: usize) {
        let ids: Vec<TileId> = match self.editor.doc().tilesets.get(set) {
            Some(s) => s.tiles().iter().map(|t| t.id).collect(),
            None => return,
        };
        // The tilemap actions may have removed every tile since they were made.
        if ids.is_empty() {
            return;
        }
        let tile = ids[self.rng.below(ids.len())];
        if self.editor.set_current_tile(set, tile).is_err() {
            return;
        }
        self.editor.layer = index;
        let path = (0..1 + self.rng.below(3)).map(|_| self.point()).collect();
        let tool = [TileTool::Place, TileTool::Line, TileTool::Rect][self.rng.below(3)];
        go(&mut self.editor, TileCmd::Stroke { tool, path });
    }
}
