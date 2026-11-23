//! What a document does with its tilesets. Commands run these inside a step, so a refusal
//! leaves the document as it was.

use super::budget::stored_tile_bytes;
use super::ids::{TileId, TilesetId};
use super::set::{clean_name, Tileset};
use crate::document::{ColorMode, Document, LimitError};
use crate::image::Image;
use std::sync::Arc;

impl Document {
    /// A tileset, for reading.
    pub fn tileset(&self, id: TilesetId) -> Result<&Tileset, LimitError> {
        self.tilesets.get(id).ok_or(LimitError::NoSuchItem)
    }

    pub(crate) fn tileset_mut(&mut self, id: TilesetId) -> Result<&mut Tileset, LimitError> {
        // Looked for first, so a tileset that is not there does not copy the table.
        self.tileset(id)?;
        Arc::make_mut(&mut self.tilesets)
            .get_mut(id)
            .ok_or(LimitError::NoSuchItem)
    }

    /// Adds an empty tileset. A name left empty becomes "Tileset" and its number.
    pub fn add_tileset(
        &mut self,
        name: &str,
        mode: ColorMode,
        tile_size: (usize, usize),
    ) -> Result<TilesetId, LimitError> {
        let mut name = clean_name(name);
        if name.is_empty() {
            name = format!("Tileset {}", self.tilesets.next_id());
        }
        Arc::make_mut(&mut self.tilesets).add(name, mode, tile_size)
    }

    /// How many layers place this tileset's tiles.
    pub fn tileset_users(&self, id: TilesetId) -> usize {
        self.layers
            .iter()
            .filter(|layer| layer.tileset() == Some(id))
            .count()
    }

    /// Removes a tileset that no layer uses, with its tiles. Its id is not given again.
    pub fn remove_tileset(&mut self, id: TilesetId) -> Result<(), LimitError> {
        self.tileset(id)?;
        if self.tileset_users(id) > 0 {
            return Err(LimitError::TilesetInUse);
        }
        Arc::make_mut(&mut self.tilesets).remove(id);
        Ok(())
    }

    /// Gives a tileset another name, cleaned as a new one's is. False if it is the same name, or
    /// nothing is left of the new one.
    pub fn rename_tileset(&mut self, id: TilesetId, name: &str) -> Result<bool, LimitError> {
        let name = clean_name(name);
        if name.is_empty() || self.tileset(id)?.name == name {
            return Ok(false);
        }
        self.tileset_mut(id)?.name = name;
        Ok(true)
    }

    /// Refuses an image that cannot be a tile of this tileset: one with drawn pixels outside the
    /// tile's size, or, in a palette tileset, naming a colour the palette does not have.
    pub fn check_tile_image(&self, id: TilesetId, image: &Image) -> Result<(), LimitError> {
        let set = self.tileset(id)?;
        if !set.holds(image) {
            return Err(LimitError::TileSize);
        }
        if set.mode() == ColorMode::Palette && !image.is_blank() {
            let (w, h) = set.tile_size();
            let known = self.colors.len() as u32;
            if image.to_cells(w, h).iter().any(|&cell| cell >= known) {
                return Err(LimitError::NoSuchItem);
            }
        }
        Ok(())
    }

    /// Adds a tile with the next id at the end of the tileset.
    pub fn add_tile(&mut self, set: TilesetId, image: Arc<Image>) -> Result<TileId, LimitError> {
        self.check_tile_image(set, &image)?;
        self.check_tile_room(0, stored_tile_bytes(&image))?;
        let tiles = self.tileset_mut(set)?;
        tiles.insert_tile(tiles.len(), image)
    }

    /// Adds a tile showing the same image as `tile`, right after it, with the next id.
    pub fn duplicate_tile(&mut self, set: TilesetId, tile: TileId) -> Result<TileId, LimitError> {
        let found = self.tileset(set)?;
        let at = found.position(tile).ok_or(LimitError::NoSuchItem)?;
        let image = found.tiles()[at].image.clone();
        // The file stores the copy on its own, though it shares the image here.
        self.check_tile_room(0, stored_tile_bytes(&image))?;
        self.tileset_mut(set)?.insert_tile(at + 1, image)
    }

    /// Takes a tile out of its tileset. Its id is not given again. Every cell that placed it,
    /// on every layer and frame, becomes empty. Returns how many cells that was.
    pub fn remove_tile(&mut self, set: TilesetId, tile: TileId) -> Result<usize, LimitError> {
        self.tileset(set)?
            .position(tile)
            .ok_or(LimitError::NoSuchItem)?;
        let emptied = self.empty_cells_of(set, tile);
        self.tileset_mut(set)?.remove_tile(tile);
        Ok(emptied)
    }

    /// Moves a tile to a place in panel order. False if it is already there.
    pub fn move_tile(
        &mut self,
        set: TilesetId,
        tile: TileId,
        to: usize,
    ) -> Result<bool, LimitError> {
        self.tileset(set)?
            .position(tile)
            .ok_or(LimitError::NoSuchItem)?;
        Ok(self.tileset_mut(set)?.move_tile(tile, to))
    }

    /// Swaps each tile's image with the one listed beside its id, leaving the images that were
    /// there. Ids that are gone are left alone, so playing a list twice is the identity.
    pub(crate) fn swap_tile_images(&mut self, set: TilesetId, list: &mut [(TileId, Arc<Image>)]) {
        let Ok(tiles) = self.tileset_mut(set) else {
            return;
        };
        for (id, image) in list.iter_mut() {
            if let Some(old) = tiles.set_image(*id, image.clone()) {
                *image = old;
            }
        }
    }
}

#[cfg(test)]
#[path = "doc_tests.rs"]
mod tests;
