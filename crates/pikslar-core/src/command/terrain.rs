//! Terrain commands (M11): terrain sets of a tileset and their slots. Each is one undo step, and
//! none when nothing changes. Terrain painting is a map stroke, in a later phase.

use super::{did, Outcome, TerrainCmd};
use crate::autotile::{clean_terrain_name, TerrainSet};
use crate::document::LimitError;
use crate::editor::Editor;
use crate::steps::Dragged;
use crate::tileset::{TileId, TilesetId};

/// The most one terrain set takes in a table a step copies: its place, a name of 64 characters
/// in UTF-8 and the slots of the largest kind.
const BYTES_PER_TERRAIN: usize = size_of::<TerrainSet>()
    + crate::autotile::MAX_TERRAIN_NAME * 4
    + crate::autotile::rules::BLOB_COUNT * size_of::<Option<TileId>>();

impl Editor {
    pub(super) fn apply_terrain(&mut self, command: TerrainCmd) -> Result<Outcome, LimitError> {
        match command {
            TerrainCmd::New {
                tileset,
                kind,
                name,
                template,
            } => {
                let id = self.transact(|d| d.add_terrain(tileset, kind, &name, template))?;
                Ok(Outcome {
                    changed: true,
                    created: Some(id.into()),
                })
            }
            TerrainCmd::Rename { tileset, set, name } => self.rename_terrain(tileset, set, &name),
            TerrainCmd::Remove { tileset, set } => {
                self.transact(|d| d.remove_terrain(tileset, set))?;
                Ok(Outcome::CHANGED)
            }
            TerrainCmd::Assign {
                tileset,
                set,
                slot,
                tile,
            } => self.put_in_slot(tileset, set, slot, Some(tile)),
            TerrainCmd::Clear { tileset, set, slot } => self.put_in_slot(tileset, set, slot, None),
        }
    }

    /// Fills or empties a slot as one step, or as none if it already held that.
    fn put_in_slot(
        &mut self,
        tileset: TilesetId,
        set: u16,
        slot: u8,
        tile: Option<TileId>,
    ) -> Result<Outcome, LimitError> {
        let changed = self.transact(|d| d.set_terrain_slot(tileset, set, slot.into(), tile))?;
        Ok(did(changed))
    }

    /// Typing a name joins one step, so a name typed in full is undone in one go.
    fn rename_terrain(
        &mut self,
        tileset: TilesetId,
        set: u16,
        name: &str,
    ) -> Result<Outcome, LimitError> {
        let name = clean_terrain_name(name);
        let found = self.doc.terrain(tileset, set)?;
        if name.is_empty() || found.name() == name {
            return Ok(Outcome::UNCHANGED);
        }
        let place = (self.layer, self.frame);
        self.join_step(Dragged::TerrainName(tileset, set), place);
        self.doc.rename_terrain(tileset, set, &name)?;
        self.amended();
        Ok(Outcome::CHANGED)
    }

    /// What the terrain sets of every tileset may take in a copy of the table.
    pub(super) fn terrain_table_bytes(&self) -> usize {
        let sets = self.doc.tilesets.iter();
        let terrains: usize = sets.map(|set| set.terrains().len()).sum();
        terrains.saturating_mul(BYTES_PER_TERRAIN)
    }

    /// The most a terrain command may add to history, asked before it runs: a copy of the table
    /// of tilesets with their terrain sets, and the tiles a template adds.
    pub(super) fn terrain_command_bytes(&self, command: &TerrainCmd) -> usize {
        // The table, and the set a new one adds.
        let table = self
            .tilesets_table_bytes()
            .saturating_add(BYTES_PER_TERRAIN);
        let added = match command {
            TerrainCmd::New {
                kind,
                template: true,
                ..
            } => kind.slot_count() * super::tile::BYTES_PER_TILE,
            TerrainCmd::New { .. }
            | TerrainCmd::Rename { .. }
            | TerrainCmd::Remove { .. }
            | TerrainCmd::Assign { .. }
            | TerrainCmd::Clear { .. } => 0,
        };
        table
            .saturating_add(added)
            .saturating_add(self.floating_bytes())
    }
}

#[cfg(test)]
#[path = "terrain_tests.rs"]
mod tests;
