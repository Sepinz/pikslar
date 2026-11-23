//! One tileset: its tiles in panel order, each an image that is shared until edited.

use super::budget::stored_tile_bytes;
use super::ids::{TileId, TilesetId, MAX_TILE_ID};
use crate::autotile::terrain::TerrainSet;
use crate::document::{ColorMode, LimitError};
use crate::image::Image;
use std::collections::HashMap;
use std::sync::Arc;

/// A document holds at most this many tilesets.
pub const MAX_TILESETS: usize = 64;
/// A tileset holds at most this many live tiles.
pub const MAX_TILES: usize = 65_535;
/// A tile is 1 to this many pixels each way.
pub const MAX_TILE_SIDE: usize = 256;
/// Names are kept to this many characters, as layer names are.
pub const MAX_TILESET_NAME: usize = 64;

/// A name as a tileset keeps it: trimmed, without control, direction or zero-width characters
/// (see [`crate::names`]), cut short.
pub fn clean_name(name: &str) -> String {
    // Control characters go first and the cut comes before the last trim, so a cleaned name
    // is clean already and a file's name reads back as it was written.
    let kept = crate::names::without_hidden(name);
    let cut: String = kept.trim().chars().take(MAX_TILESET_NAME).collect();
    cut.trim_end().to_owned()
}

/// One tile of a tileset. Its pixels are in the tile's own coordinates, from (0, 0), and a tile
/// that is blank holds no pixels at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileEntry {
    pub id: TileId,
    pub image: Arc<Image>,
}

/// The tiles one or more tilemap layers place. All of its tiles share one size and colour mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tileset {
    id: TilesetId,
    pub(crate) name: String,
    mode: ColorMode,
    tile_w: u16,
    tile_h: u16,
    /// The id the next tile takes. Never goes back, so a deleted tile's id is not given again.
    next_tile: u32,
    tiles: Vec<TileEntry>,
    /// Where each id is in `tiles`, rebuilt whenever tiles are added, removed or moved.
    index: HashMap<TileId, usize>,
    /// The terrain sets of these tiles (M11). Shared with the steps of history until changed.
    terrains: Arc<Vec<TerrainSet>>,
    /// What every tile image comes to in a file, by [`stored_tile_bytes`], kept as tiles change.
    stored: usize,
}

impl Tileset {
    /// A tileset with no tiles, refused unless a tile is 1 to [`MAX_TILE_SIDE`] pixels each way.
    pub(crate) fn new(
        id: TilesetId,
        name: String,
        mode: ColorMode,
        (tile_w, tile_h): (usize, usize),
    ) -> Result<Self, LimitError> {
        let side = 1..=MAX_TILE_SIDE;
        if !side.contains(&tile_w) || !side.contains(&tile_h) {
            return Err(LimitError::TileSize);
        }
        Ok(Tileset {
            id,
            name,
            mode,
            tile_w: tile_w as u16,
            tile_h: tile_h as u16,
            next_tile: 1,
            tiles: Vec::new(),
            index: HashMap::new(),
            terrains: Arc::default(),
            stored: 0,
        })
    }

    /// Gives a copy of a tileset its own id and name.
    /// A tileset as a file kept it: tiles in panel order with their ids and the saved counter for
    /// the next one. `None` unless the size is 1 to 256 each way, there are at most
    /// [`MAX_TILES`] tiles, the ids are distinct, from 1 and below the counter, the counter is
    /// at most one past [`MAX_TILE_ID`], and every image lies within the tile size.
    pub fn from_parts(
        id: TilesetId,
        name: String,
        mode: ColorMode,
        tile_size: (usize, usize),
        next_tile: u32,
        tiles: Vec<TileEntry>,
    ) -> Option<Self> {
        let mut set = Tileset::new(id, name, mode, tile_size).ok()?;
        if tiles.len() > MAX_TILES || next_tile == 0 || next_tile > MAX_TILE_ID + 1 {
            return None;
        }
        if tiles
            .iter()
            .any(|entry| entry.id.get() >= next_tile || !set.holds(&entry.image))
        {
            return None;
        }
        set.next_tile = next_tile;
        set.stored = tiles.iter().fold(0usize, |sum, entry| {
            sum.saturating_add(stored_tile_bytes(&entry.image))
        });
        set.tiles = tiles;
        set.reindex();
        (set.index.len() == set.tiles.len()).then_some(set)
    }

    pub(crate) fn relabel(&mut self, id: TilesetId, name: String) {
        self.id = id;
        self.name = name;
    }

    pub fn id(&self) -> TilesetId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn mode(&self) -> ColorMode {
        self.mode
    }

    /// Width and height of every tile, in pixels.
    pub fn tile_size(&self) -> (usize, usize) {
        (self.tile_w.into(), self.tile_h.into())
    }

    /// The tiles, in the order the panel shows them.
    pub fn tiles(&self) -> &[TileEntry] {
        &self.tiles
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// The id the next tile takes.
    pub fn next_tile_id(&self) -> u32 {
        self.next_tile
    }

    /// Where a tile is in panel order.
    pub fn position(&self, id: TileId) -> Option<usize> {
        self.index.get(&id).copied()
    }

    /// A tile by id. Ids are given out in order, so while nothing has been removed or moved a
    /// tile sits at its id less one, which is tried first and saves the hash table's lookup.
    #[inline]
    pub fn tile(&self, id: TileId) -> Option<&TileEntry> {
        let guess = (id.get() as usize).wrapping_sub(1);
        match self.tiles.get(guess) {
            Some(entry) if entry.id == id => Some(entry),
            _ => self.position(id).map(|at| &self.tiles[at]),
        }
    }

    /// True if every drawn pixel of `image` lies inside a tile of this tileset.
    pub fn holds(&self, image: &Image) -> bool {
        let (w, h) = self.tile_size();
        image.drawn_bounds().is_none_or(|(x, y, bw, bh)| {
            x >= 0 && y >= 0 && x as i64 + bw as i64 <= w as i64 && y as i64 + bh as i64 <= h as i64
        })
    }

    /// Adds a tile at `at`, or at the end if that is past it, with the next id.
    pub(crate) fn insert_tile(
        &mut self,
        at: usize,
        image: Arc<Image>,
    ) -> Result<TileId, LimitError> {
        if self.tiles.len() >= MAX_TILES {
            return Err(LimitError::TooManyTiles);
        }
        if self.next_tile > MAX_TILE_ID {
            return Err(LimitError::TooManyTiles);
        }
        let id = TileId::new(self.next_tile).ok_or(LimitError::TooManyTiles)?;
        self.next_tile += 1;
        self.stored = self.stored.saturating_add(stored_tile_bytes(&image));
        let at = at.min(self.tiles.len());
        self.tiles.insert(at, TileEntry { id, image });
        if at + 1 == self.tiles.len() {
            // Added at the end: no other tile moved, so a file of 65,535 tiles is not
            // indexed 65,535 times over.
            self.index.insert(id, at);
        } else {
            self.reindex();
        }
        Ok(id)
    }

    /// Takes a tile out, emptying the terrain slots it filled. Its id is not given again.
    pub(crate) fn remove_tile(&mut self, id: TileId) -> Option<TileEntry> {
        let at = self.position(id)?;
        let entry = self.tiles.remove(at);
        self.stored = self.stored.saturating_sub(stored_tile_bytes(&entry.image));
        self.reindex();
        if self.terrains.iter().any(|set| set.holds(id)) {
            self.terrains_mut().iter_mut().for_each(|set| {
                set.forget(id);
            });
        }
        Some(entry)
    }

    /// Moves a tile to `to` in panel order, or to the end if that is past it.
    pub(crate) fn move_tile(&mut self, id: TileId, to: usize) -> bool {
        let Some(from) = self.position(id) else {
            return false;
        };
        let to = to.min(self.tiles.len() - 1);
        if from == to {
            return false;
        }
        let entry = self.tiles.remove(from);
        self.tiles.insert(to, entry);
        self.reindex();
        true
    }

    /// Gives a tile another image and returns the one it had. The index is untouched, as no id
    /// moves.
    pub(crate) fn set_image(&mut self, id: TileId, image: Arc<Image>) -> Option<Arc<Image>> {
        let at = self.position(id)?;
        let added = stored_tile_bytes(&image);
        let old = std::mem::replace(&mut self.tiles[at].image, image);
        self.stored = self
            .stored
            .saturating_sub(stored_tile_bytes(&old))
            .saturating_add(added);
        Some(old)
    }

    /// What every tile image of this tileset comes to in a file, by [`stored_tile_bytes`].
    pub fn stored_bytes(&self) -> usize {
        self.stored
    }

    /// The terrain sets, in the order they were made.
    pub fn terrains(&self) -> &[TerrainSet] {
        &self.terrains
    }

    /// A terrain set by id.
    pub fn terrain(&self, id: u16) -> Option<&TerrainSet> {
        self.terrains.iter().find(|set| set.id() == id)
    }

    /// The terrain sets, to change. Copied first if history shares them.
    pub(crate) fn terrains_mut(&mut self) -> &mut Vec<TerrainSet> {
        Arc::make_mut(&mut self.terrains)
    }

    fn reindex(&mut self) {
        self.index = self
            .tiles
            .iter()
            .enumerate()
            .map(|(at, entry)| (entry.id, at))
            .collect();
    }

    /// What this tileset takes besides its images and its own place in a list: its name, its
    /// list of tiles and its index.
    pub(crate) fn table_bytes(&self) -> usize {
        let index =
            crate::history::hash_table_bytes(self.index.capacity(), size_of::<(TileId, usize)>());
        let terrains = self.terrains.capacity() * size_of::<TerrainSet>()
            + self
                .terrains
                .iter()
                .map(TerrainSet::heap_bytes)
                .sum::<usize>();
        self.name.capacity() + self.tiles.capacity() * size_of::<TileEntry>() + index + terrains
    }
}

#[cfg(test)]
#[path = "set_tests.rs"]
mod tests;
