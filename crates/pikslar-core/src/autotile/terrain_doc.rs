//! What a document does with the terrain sets of its tilesets. Commands run these inside a step,
//! so a refusal leaves the document as it was.

use super::terrain::{clean_terrain_name, next_terrain_id, TerrainKind, TerrainSet, MAX_TERRAINS};
use crate::document::{Document, LimitError};
use crate::image::Image;
use crate::tileset::{TileId, TilesetId, MAX_TILES, MAX_TILE_ID};
use std::sync::Arc;

impl Document {
    /// A terrain set of a tileset, for reading.
    pub fn terrain(&self, tileset: TilesetId, set: u16) -> Result<&TerrainSet, LimitError> {
        self.tileset(tileset)?
            .terrain(set)
            .ok_or(LimitError::NoSuchItem)
    }

    fn terrain_mut(&mut self, tileset: TilesetId, set: u16) -> Result<&mut TerrainSet, LimitError> {
        // Looked for first, so a set that is not there copies nothing.
        self.terrain(tileset, set)?;
        let sets = self.tileset_mut(tileset)?.terrains_mut();
        sets.iter_mut()
            .find(|s| s.id() == set)
            .ok_or(LimitError::NoSuchItem)
    }

    /// Adds a terrain set with every slot empty, and returns its id. A name left empty becomes
    /// "Terrain" and its number. With `template`, a blank tile is added at the end of the tileset
    /// for every slot, in slot order, and each slot takes its own.
    pub fn add_terrain(
        &mut self,
        tileset: TilesetId,
        kind: TerrainKind,
        name: &str,
        template: bool,
    ) -> Result<u16, LimitError> {
        let found = self.tileset(tileset)?;
        let id = next_terrain_id(found.terrains()).ok_or(LimitError::TooManyTerrains)?;
        let count = kind.slot_count();
        let last_id = found.next_tile_id() as usize + count - 1;
        if template && (found.len() + count > MAX_TILES || last_id > MAX_TILE_ID as usize) {
            return Err(LimitError::TooManyTiles);
        }
        if template {
            let blank = crate::tileset::stored_tile_bytes(&Image::blank());
            self.check_tile_room(0, blank.saturating_mul(count))?;
        }
        let mut name = clean_terrain_name(name);
        if name.is_empty() {
            name = format!("Terrain {id}");
        }
        let mut set = TerrainSet::new(id, name, kind);
        if template {
            let blank = Arc::new(Image::blank());
            for slot in 0..count {
                let tile = self.add_tile(tileset, blank.clone())?;
                set.set_slot(slot, Some(tile))?;
            }
        }
        self.tileset_mut(tileset)?.terrains_mut().push(set);
        Ok(id)
    }

    /// Gives a terrain set another name, cleaned as a new one's is. False if it is the same
    /// name, or nothing is left of the new one.
    pub fn rename_terrain(
        &mut self,
        tileset: TilesetId,
        set: u16,
        name: &str,
    ) -> Result<bool, LimitError> {
        let name = clean_terrain_name(name);
        if name.is_empty() || self.terrain(tileset, set)?.name() == name {
            return Ok(false);
        }
        self.terrain_mut(tileset, set)?.name = name;
        Ok(true)
    }

    /// Removes a terrain set. Its tiles stay in the tileset and on the maps.
    pub fn remove_terrain(&mut self, tileset: TilesetId, set: u16) -> Result<(), LimitError> {
        self.terrain(tileset, set)?;
        let sets = self.tileset_mut(tileset)?.terrains_mut();
        sets.retain(|s| s.id() != set);
        Ok(())
    }

    /// Puts a tile of the set's tileset in a slot, or empties the slot. False if it held that.
    pub fn set_terrain_slot(
        &mut self,
        tileset: TilesetId,
        set: u16,
        slot: usize,
        tile: Option<TileId>,
    ) -> Result<bool, LimitError> {
        let found = self.terrain(tileset, set)?;
        if slot >= found.slots().len() {
            return Err(LimitError::TerrainSlot);
        }
        if let Some(tile) = tile {
            self.tileset(tileset)?
                .position(tile)
                .ok_or(LimitError::NoSuchItem)?;
        }
        if found.slot(slot) == tile {
            return Ok(false);
        }
        self.terrain_mut(tileset, set)?.set_slot(slot, tile)
    }

    /// Whether [`Document::load_terrains`] would take these sets for the tileset, changing
    /// nothing: the tileset must exist, with at most [`MAX_TERRAINS`] sets of distinct ids.
    pub fn check_terrains(
        &self,
        tileset: TilesetId,
        sets: &[TerrainSet],
    ) -> Result<(), LimitError> {
        self.tileset(tileset)?;
        let mut ids: Vec<u32> = sets.iter().map(|s| u32::from(s.id())).collect();
        ids.sort_unstable();
        ids.dedup();
        match sets.len() > MAX_TERRAINS || ids.len() != sets.len() {
            true => Err(LimitError::TooManyTerrains),
            false => Ok(()),
        }
    }

    /// Gives a tileset the terrain sets a file kept, in place of any it had. Names are cleaned
    /// and a slot naming a tile the tileset lacks is emptied; true if either happened. Refused
    /// for more than [`MAX_TERRAINS`] sets or two of one id, changing nothing.
    pub fn load_terrains(
        &mut self,
        tileset: TilesetId,
        mut sets: Vec<TerrainSet>,
    ) -> Result<bool, LimitError> {
        self.check_terrains(tileset, &sets)?;
        let found = self.tileset(tileset)?;
        let mut trimmed = false;
        for set in &mut sets {
            trimmed |= set.keep_only(|tile| found.position(tile).is_some());
            let name = clean_terrain_name(&set.name);
            trimmed |= name != set.name;
            set.name = name;
        }
        *self.tileset_mut(tileset)?.terrains_mut() = sets;
        Ok(trimmed)
    }
}

#[cfg(test)]
#[path = "terrain_doc_tests.rs"]
mod tests;
