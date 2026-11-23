//! Tilemap commands (M10): tilesets and their tiles. Each is one undo step, and none when
//! nothing changes. Tilemap layers and map strokes arrive in phases 10.2 and 10.5.

use super::{did, LayerId, Outcome};
use crate::document::{ColorMode, LimitError};
use crate::editor::Editor;
use crate::history::Edit;
use crate::image::Image;
use crate::steps::Dragged;
use crate::tileset::{clean_name, TileEntry, TileId, TilesetId};
use std::sync::Arc;

/// What a map stroke does to the cells it passes over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileTool {
    /// Places the stamp, along the path of cells.
    Place,
    /// Clears cells along the path.
    Erase,
    /// Fills the region of cells equal to the one pressed, with the stamp as a pattern.
    Fill,
    /// The stamp, along a straight line from the press.
    Line,
    /// The stamp, around the rectangle of cells from the press.
    Rect,
    /// Makes the tile under the press, and how it is turned, the stamp. Changes no map.
    Pick,
    /// Paints or clears a terrain set's membership; the tiles follow from the set's rules.
    Terrain(TerrainTool),
}

/// A terrain stroke: which set, what shape, and whether it paints or clears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainTool {
    pub tileset: TilesetId,
    /// The set's id within the tileset.
    pub set: u16,
    pub shape: TerrainShape,
    /// Clears membership (or corners) instead of setting it.
    pub clear: bool,
    /// How a Corner4 set's corners are painted; other kinds ignore it.
    pub corners: Corners,
}

/// The cells a terrain stroke paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainShape {
    /// Along the path of the pointer (B, and E with `clear`).
    Brush,
    /// The filled rectangle from the press to the pointer (U).
    Rect,
    /// The 4-connected region of cells equal to the one pressed, pins aside (G). Finished at once.
    Fill,
    /// Re-autotiles the one cell pressed, unpinning it, and changes no other (Alt-click). Finished
    /// at once.
    Retile,
}

/// How a Corner4 stroke sets corners.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Corners {
    /// A painted cell has all four corners set (or cleared), which its 8 neighbours share.
    #[default]
    Cell,
    /// The stroke sets the cell corner nearest the pointer, shared by the four cells around it.
    /// A fill paints cells.
    Vertex,
}

/// Where a new tile's pixels come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TileSource {
    /// A tile with nothing drawn.
    Blank,
    /// A tile showing this image, shared and not copied. Its pixels are in the tile's own
    /// coordinates and must lie within the tileset's tile size.
    Image(Arc<Image>),
    /// The selected pixels of the active paint layer, cut into tiles of the tileset's size from
    /// the selection's corner. A tile that is clear, or the same as one the tileset has or one
    /// cut before it, is left out. The outcome's `created` is the first new tile, and nothing
    /// happens if there is none.
    Selection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TileCmd {
    /// Adds an empty tileset of tiles `tile_w` by `tile_h`, each 1 to 256 pixels. Its id is the
    /// outcome's `created`.
    NewTileset {
        name: String,
        mode: ColorMode,
        tile_w: u16,
        tile_h: u16,
    },
    /// Typing joins one step until `StepCmd::EndDrag`.
    RenameTileset { tileset: TilesetId, name: String },
    /// Removes a tileset that no layer uses, with its tiles.
    RemoveTileset { tileset: TilesetId },
    /// Adds a tile at the end of the tileset. Its id is the outcome's `created`.
    AddTile {
        tileset: TilesetId,
        from: TileSource,
    },
    /// Adds a tile showing the same image right after the tile. The copy's id is `created`.
    DuplicateTile { tileset: TilesetId, tile: TileId },
    /// Removes a tile. Its id is never given again.
    RemoveTile { tileset: TilesetId, tile: TileId },
    /// Moves a tile to a place in the panel's order, or to the end if that is past it.
    MoveTile {
        tileset: TilesetId,
        tile: TileId,
        to: usize,
    },
    /// Inserts a tilemap layer of the tileset, every frame empty, and makes it the active one.
    /// It takes the tileset's colour mode.
    NewLayer { at: usize, tileset: TilesetId },
    /// Adds an empty tileset of tiles `tile_w` by `tile_h` and a tilemap layer of it at `at`,
    /// made the active one, as one step. The tileset's id is the outcome's `created`.
    NewMapLayer {
        at: usize,
        name: String,
        mode: ColorMode,
        tile_w: u16,
        tile_h: u16,
    },
    /// Makes the layer use a copy of its tileset: the same tile ids, images shared until
    /// edited. The copy's id is the outcome's `created`. The layer leaves any link to a
    /// layer of the old tileset.
    CopyTilesetFor { layer: LayerId },
    /// Gives a tile other pixels. Every placement of the tile shows them.
    SetTileImage {
        tileset: TilesetId,
        tile: TileId,
        image: Arc<Image>,
    },
    /// Presses a map tool on the canvas, at a canvas position, on the active tilemap layer. A
    /// fill is finished at once, a pick only chooses the stamp, and other strokes go on until
    /// `Commit`, which is one undo step. Cells are placed where their rectangle meets the
    /// canvas and, with a selection, where it holds the cell's centre.
    Begin { tool: TileTool, x: i32, y: i32 },
    /// Continues the stroke in progress.
    To { x: i32, y: i32 },
    /// Puts the stroke in progress into the map as one step.
    Commit,
    /// Throws the stroke in progress away.
    Cancel,
    /// A whole stroke, given all at once.
    Stroke {
        tool: TileTool,
        path: Vec<(i32, i32)>,
    },
    /// Clears every cell of the active map whose centre the selection holds.
    DeleteSelected,
}

/// What one tile's place in a tileset's list and index take, for the table a step copies.
pub(super) const BYTES_PER_TILE: usize = std::mem::size_of::<TileEntry>() + 24;

impl Editor {
    pub(super) fn apply_tile(&mut self, command: TileCmd) -> Result<Outcome, LimitError> {
        match command {
            TileCmd::NewTileset {
                name,
                mode,
                tile_w,
                tile_h,
            } => self.new_tileset(&name, mode, (tile_w.into(), tile_h.into())),
            TileCmd::RenameTileset { tileset, name } => self.rename_tileset(tileset, &name),
            TileCmd::RemoveTileset { tileset } => {
                self.transact(|d| d.remove_tileset(tileset))?;
                Ok(Outcome::CHANGED)
            }
            TileCmd::NewLayer { at, tileset } => self.new_tile_layer(at, tileset),
            TileCmd::NewMapLayer {
                at,
                name,
                mode,
                tile_w,
                tile_h,
            } => self.new_map_layer(at, &name, mode, (tile_w.into(), tile_h.into())),
            TileCmd::CopyTilesetFor { layer } => self.copy_tileset_for_layer(layer),
            TileCmd::AddTile { tileset, from } => self.add_tile_from(tileset, from),
            TileCmd::DuplicateTile { tileset, tile } => self.duplicate_tile_cmd(tileset, tile),
            TileCmd::RemoveTile { tileset, tile } => {
                self.transact(|d| d.remove_tile(tileset, tile).map(|_| ()))?;
                Ok(Outcome::CHANGED)
            }
            TileCmd::MoveTile { tileset, tile, to } => {
                let moved = self.transact(|d| d.move_tile(tileset, tile, to))?;
                Ok(did(moved))
            }
            TileCmd::SetTileImage {
                tileset,
                tile,
                image,
            } => self.set_tile_image(tileset, tile, image),
            TileCmd::Begin { tool, x, y } => self.map_stroke(|e| e.begin_tiles(tool, x, y)),
            TileCmd::To { x, y } => self.map_step(|e| e.drag_tiles(x, y)),
            TileCmd::Commit => self.map_step(Editor::commit),
            TileCmd::Cancel => self.map_step(Editor::cancel),
            TileCmd::Stroke { tool, path } => self.map_stroke(|e| e.stroke_tiles(tool, &path)),
            TileCmd::DeleteSelected => self.map_stroke(Editor::delete_tiles),
        }
    }

    fn new_tileset(
        &mut self,
        name: &str,
        mode: ColorMode,
        size: (usize, usize),
    ) -> Result<Outcome, LimitError> {
        let id = self.transact(|d| d.add_tileset(name, mode, size))?;
        Ok(created(id.get()))
    }

    fn copy_tileset_for_layer(&mut self, layer: LayerId) -> Result<Outcome, LimitError> {
        let index = self.index_of(layer)?;
        let copy = self.transact(|d| d.copy_tileset_for(index))?;
        Ok(created(copy.get()))
    }

    fn duplicate_tile_cmd(
        &mut self,
        tileset: TilesetId,
        tile: TileId,
    ) -> Result<Outcome, LimitError> {
        let copy = self.transact(|d| d.duplicate_tile(tileset, tile))?;
        Ok(created(copy.get()))
    }

    fn new_tile_layer(&mut self, at: usize, tileset: TilesetId) -> Result<Outcome, LimitError> {
        self.transact(|d| d.add_tilemap_layer(at, tileset))?;
        self.layer = at.min(self.doc.layers.len() - 1);
        Ok(Outcome::CHANGED)
    }

    fn new_map_layer(
        &mut self,
        at: usize,
        name: &str,
        mode: ColorMode,
        size: (usize, usize),
    ) -> Result<Outcome, LimitError> {
        let id = self.transact(|d| {
            let id = d.add_tileset(name, mode, size)?;
            d.add_tilemap_layer(at, id)?;
            Ok(id)
        })?;
        self.layer = at.min(self.doc.layers.len() - 1);
        Ok(created(id.get()))
    }

    fn add_tile_from(
        &mut self,
        tileset: TilesetId,
        from: TileSource,
    ) -> Result<Outcome, LimitError> {
        let image = match from {
            TileSource::Blank => Arc::new(Image::blank()),
            TileSource::Image(image) => image,
            TileSource::Selection => return self.tiles_from_selection(tileset),
        };
        let tile = self.transact(|d| d.add_tile(tileset, image))?;
        Ok(created(tile.get()))
    }

    /// Runs a map-placing command. Such commands are steps of their own only at the commit.
    fn map_stroke(
        &mut self,
        run: impl FnOnce(&mut Self) -> Result<(), LimitError>,
    ) -> Result<Outcome, LimitError> {
        let revision = self.revision;
        run(self)?;
        Ok(did(self.revision != revision))
    }

    /// The same for a map command that cannot fail.
    fn map_step(&mut self, run: impl FnOnce(&mut Self)) -> Result<Outcome, LimitError> {
        self.map_stroke(|e| {
            run(e);
            Ok(())
        })
    }

    fn stroke_tiles(&mut self, tool: TileTool, path: &[(i32, i32)]) -> Result<(), LimitError> {
        if let Some((&(x, y), rest)) = path.split_first() {
            self.begin_tiles(tool, x, y)?;
            for &(x, y) in rest {
                self.drag_tiles(x, y);
            }
            self.commit();
        }
        Ok(())
    }

    /// Typing a name joins one step, so a name typed in full is undone in one go.
    fn rename_tileset(&mut self, tileset: TilesetId, name: &str) -> Result<Outcome, LimitError> {
        let name = clean_name(name);
        let found = self.doc.tileset(tileset)?;
        if name.is_empty() || found.name() == name {
            return Ok(Outcome::UNCHANGED);
        }
        self.join_step(Dragged::TilesetName(tileset), (self.layer, self.frame));
        self.doc.rename_tileset(tileset, &name)?;
        self.amended();
        Ok(Outcome::CHANGED)
    }

    /// Swaps in the new pixels as a step that keeps the old image, so undoing it costs no copy.
    fn set_tile_image(
        &mut self,
        tileset: TilesetId,
        tile: TileId,
        image: Arc<Image>,
    ) -> Result<Outcome, LimitError> {
        let found = self.doc.tileset(tileset)?;
        let current = found.tile(tile).ok_or(LimitError::NoSuchItem)?;
        self.doc.check_tile_image(tileset, &image)?;
        if current.image == image {
            return Ok(Outcome::UNCHANGED);
        }
        let mut before = vec![(tile, image)];
        self.doc.check_tile_swap(tileset, &before)?;
        self.cancel();
        self.doc.swap_tile_images(tileset, &mut before);
        self.record(Edit::tile_images(tileset, before.into(), None));
        self.damage_tile_edit(tileset, &[tile]);
        Ok(Outcome::CHANGED)
    }

    /// The most a tile command may add to history, asked before it runs: a copy of the table
    /// of tilesets, and the pixels it brings.
    pub(super) fn tile_command_bytes(&self, command: &TileCmd) -> usize {
        let table = self.tilesets_table_bytes();
        let pixels = match command {
            TileCmd::AddTile {
                from: TileSource::Image(image),
                ..
            }
            | TileCmd::SetTileImage { image, .. } => image.heap_size(),
            // The selection copied, and the tiles cut from it.
            TileCmd::AddTile {
                from: TileSource::Selection,
                ..
            } => self.selected_bytes().saturating_mul(2),
            TileCmd::NewTileset { .. }
            | TileCmd::RenameTileset { .. }
            | TileCmd::RemoveTileset { .. }
            | TileCmd::AddTile { .. }
            | TileCmd::DuplicateTile { .. }
            | TileCmd::MoveTile { .. }
            | TileCmd::To { .. }
            | TileCmd::Commit
            | TileCmd::Cancel => 0,
            TileCmd::Begin { tool, .. } | TileCmd::Stroke { tool, .. } => {
                self.map_stroke_bytes(*tool)
            }
            TileCmd::DeleteSelected => self.map_stroke_bytes(TileTool::Erase),
            // The cels of the layers it adds or leaves, and the maps it rewrites, which the
            // step keeps as they were.
            TileCmd::RemoveTile { tileset, .. } => self.map_bytes(*tileset),
            TileCmd::NewLayer { .. }
            | TileCmd::NewMapLayer { .. }
            | TileCmd::CopyTilesetFor { .. } => self.structure_bytes(),
        };
        table
            .saturating_add(pixels)
            .saturating_add(self.floating_bytes())
    }

    /// What a copy of the table of tilesets may take, with their terrain sets.
    pub(super) fn tilesets_table_bytes(&self) -> usize {
        let tiles: usize = self.doc.tilesets.iter().map(|set| set.len()).sum();
        tiles
            .saturating_mul(BYTES_PER_TILE)
            .saturating_add(self.terrain_table_bytes())
    }

    /// What a map stroke may need: the overlay's slots for the cells the canvas shows, and the
    /// cells it fills, with a mark for each when it is a fill. Asked before anything changes, so
    /// a layer that is not a map is left to the command's own refusal.
    fn map_stroke_bytes(&self, tool: TileTool) -> usize {
        const MOST: usize = 64 << 20;
        let Ok(geometry) = self.map_geometry() else {
            return 0;
        };
        let cells = geometry.window_cells();
        let slots = cells.div_ceil(crate::tile::TILE_AREA).saturating_mul(8);
        let held = match tool {
            TileTool::Fill => cells.saturating_mul(6),
            // The points of a terrain fill, kept in a set, and the cells it changes with theirs.
            TileTool::Terrain(TerrainTool {
                shape: TerrainShape::Fill,
                ..
            }) => cells.saturating_mul(40),
            _ => cells.saturating_mul(4).min(MOST),
        };
        slots
            .saturating_add(held)
            .saturating_add(self.floating_bytes())
    }

    /// What the map drawings of the layers using a tileset hold, each drawing once.
    fn map_bytes(&self, tileset: TilesetId) -> usize {
        let mut seen = std::collections::HashSet::new();
        let maps = self
            .doc
            .layers()
            .iter()
            .filter(|l| l.tileset() == Some(tileset));
        let cels = maps.flat_map(|l| l.cels().iter());
        let distinct = cels.filter(|cel| seen.insert(cel.image_id()));
        distinct.fold(0usize, |sum, cel| sum.saturating_add(cel.heap_size()))
    }
}

fn created(id: u32) -> Outcome {
    Outcome {
        changed: true,
        created: Some(id),
    }
}
