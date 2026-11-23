//! The room tiles take in a project file, and the limit a reader holds them to.
//!
//! A project file stores every tile image on its own, even where tiles share one image in memory,
//! and a reader decodes each one again: a tile duplicated a thousand times costs a pointer here
//! but a thousand pictures in the file. So the limit is counted the way the file is read, and
//! every change that could add tile pixels is refused before it would pass it, so that whatever
//! is saved opens again.

use super::ids::TilesetId;
use crate::document::{Document, LimitError};
use crate::image::Image;
use std::sync::Arc;

/// Every tile image of a document together, counted by [`stored_tile_bytes`], is at most this
/// much. A file reader refuses a file past it, so the editor holds every change to it as well.
///
/// The charge is what a read tile really holds (see `docs/format-v3.md`). Builds before this one
/// charged [`OLD_RECORD_BYTES`] a record and nothing for each storage tile's shared allocation,
/// against 256 MiB; this is the smallest whole MiB that every document that rule let be saved
/// still fits, by [`OLD_RULE_WORST_CASE`], so every file they saved opens.
pub const MAX_TILE_BYTES: usize = 416 << 20;

/// What a read tile holds beyond its table and storage tiles: its entry in the tileset's list
/// (16 bytes), its own image (an `Arc<Image>`, 48) and its place in the id index, which is at
/// most 39 just after the index has grown. Measured by `pikslar-format/tests/tile_resident.rs`.
pub const TILE_RECORD_BYTES: usize = 104;

/// What a storage tile holds beyond what [`crate::tile::Tile::heap_size`] counts for it: its
/// shared allocation is the tile and two reference counts.
pub const STORAGE_TILE_EXTRA: usize = 8;

/// What builds before this one charged a record, against 256 MiB.
pub const OLD_RECORD_BYTES: usize = 64;

/// The most the present charge can come to for a document the old rule allowed: the old 256 MiB,
/// plus what each record costs more, for as many records as a document can have (every one a
/// blank tile, the cheapest under the old rule), plus the extra of the storage tiles the few
/// bytes left could pay for (one costs at least 56 bytes under the old rule).
pub const OLD_RULE_WORST_CASE: usize = {
    let records = super::MAX_TILESETS * super::MAX_TILES;
    let left = (256 << 20) - records * OLD_RECORD_BYTES;
    (256 << 20) + records * (TILE_RECORD_BYTES - OLD_RECORD_BYTES) + left / 56 * STORAGE_TILE_EXTRA
};

const _: () = assert!(OLD_RULE_WORST_CASE <= MAX_TILE_BYTES);

/// What one tile image comes to when a file holding it is read: its record, its table of storage
/// tiles and each storage tile as its own, however many images share it in memory.
pub fn stored_tile_bytes(image: &Image) -> usize {
    let extra = image
        .drawn_tiles()
        .count()
        .saturating_mul(STORAGE_TILE_EXTRA);
    TILE_RECORD_BYTES
        .saturating_add(image.heap_size())
        .saturating_add(extra)
}

impl Document {
    /// What every tile image of the document comes to in a file, by [`stored_tile_bytes`].
    pub fn stored_tile_bytes(&self) -> usize {
        self.tilesets.stored_bytes()
    }

    /// Refuses a change that takes out tile images storing `removed` bytes and brings in ones
    /// storing `added`, when it would take the document past [`MAX_TILE_BYTES`]. A change that
    /// adds nothing is never refused.
    pub(crate) fn check_tile_room(&self, removed: usize, added: usize) -> Result<(), LimitError> {
        if added <= removed {
            return Ok(());
        }
        let after = self
            .stored_tile_bytes()
            .saturating_sub(removed)
            .saturating_add(added);
        match after > MAX_TILE_BYTES {
            true => Err(LimitError::TileMemory),
            false => Ok(()),
        }
    }

    /// Refuses giving the listed tiles of a tileset these images, when the new images would take
    /// the document past [`MAX_TILE_BYTES`]. Ids the tileset lacks are left out, as a swap leaves
    /// them alone.
    pub(crate) fn check_tile_swap(
        &self,
        set: TilesetId,
        list: &[(super::TileId, Arc<Image>)],
    ) -> Result<(), LimitError> {
        let found = self.tileset(set)?;
        let (mut removed, mut added) = (0usize, 0usize);
        for (id, image) in list {
            if let Some(entry) = found.tile(*id) {
                removed = removed.saturating_add(stored_tile_bytes(&entry.image));
                added = added.saturating_add(stored_tile_bytes(image));
            }
        }
        self.check_tile_room(removed, added)
    }
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
