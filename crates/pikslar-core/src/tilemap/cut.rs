//! Tiles from a selection: the selected pixels of a paint layer, cut into tiles of a tileset's
//! size, each added unless the tileset (or an earlier cut of the same selection) already has
//! exactly that picture.
//!
//! Duplicates are found by a hash of the pixels and then confirmed pixel by pixel, so two
//! different pictures are never taken for one however the hash falls.

use crate::command::Outcome;
use crate::document::{ColorMode, LimitError};
use crate::editor::Editor;
use crate::grid::Grid;
use crate::image::Image;
use crate::tileset::{TilesetId, MAX_TILES, MAX_TILE_BYTES};
use std::collections::HashMap;
use std::sync::Arc;

/// The default hash of a tile's pixels: FNV-1a over the values.
pub(crate) fn fnv(pixels: &[u32]) -> u64 {
    pixels.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &p| {
        (hash ^ u64::from(p)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Where a tile with a known hash is: in the tileset already, or among those cut so far.
#[derive(Clone, Copy)]
enum Home {
    Existing(usize),
    Cut(usize),
}

/// A tile cut from a selection: its image, and its pixels to compare others with.
pub(crate) type CutTile = (Arc<Image>, Vec<u32>);

/// What a cut may add to a tileset: how many tiles, and how many bytes of tiles as a file stores
/// them ([`crate::tileset::stored_tile_bytes`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Room {
    pub(crate) tiles: usize,
    pub(crate) bytes: usize,
}

impl Room {
    /// Takes one tile out of the room, refused once there is none left.
    fn take(&mut self, image: &Image) -> Result<(), LimitError> {
        let bytes = crate::tileset::stored_tile_bytes(image);
        self.tiles = self.tiles.checked_sub(1).ok_or(LimitError::TooManyTiles)?;
        self.bytes = self
            .bytes
            .checked_sub(bytes)
            .ok_or(LimitError::TileMemory)?;
        Ok(())
    }
}

/// The tiles of `size` that a `width` by `height` block of values holds, left to right and top
/// to bottom from its corner, at the edge padded with clear values. A tile with nothing in it,
/// and a tile the same as one of `existing` or as one cut before it, is left out. Refused as soon
/// as the tiles cut would pass `room`, so a selection far too big is not cut whole first.
pub(crate) fn cut_tiles(
    block: (&[u32], usize, usize),
    size: (usize, usize),
    existing: &[Arc<Image>],
    hash: impl Fn(&[u32]) -> u64,
    mut room: Room,
) -> Result<Vec<CutTile>, LimitError> {
    let (values, width, height) = block;
    let (tw, th) = size;
    let mut homes: HashMap<u64, Vec<Home>> = HashMap::new();
    for (i, image) in existing.iter().enumerate() {
        let pixels = image.to_cells(tw, th);
        homes
            .entry(hash(&pixels))
            .or_default()
            .push(Home::Existing(i));
    }
    let mut cut: Vec<CutTile> = Vec::new();
    for top in (0..height).step_by(th) {
        for left in (0..width).step_by(tw) {
            let pixels = one_tile(values, (width, height), (left, top), size);
            if pixels.iter().all(|&p| p == 0) {
                continue;
            }
            let found = homes.get(&hash(&pixels)).is_some_and(|list| {
                list.iter().any(|home| match *home {
                    Home::Existing(i) => existing[i].to_cells(tw, th) == pixels,
                    Home::Cut(i) => cut[i].1 == pixels,
                })
            });
            if found {
                continue;
            }
            homes
                .entry(hash(&pixels))
                .or_default()
                .push(Home::Cut(cut.len()));
            let image = Arc::new(Grid::from_cells(tw, th, &pixels).into_image());
            room.take(&image)?;
            cut.push((image, pixels));
        }
    }
    Ok(cut)
}

/// The `size` tile whose corner is at `corner` of a block, clear past the block's edge.
fn one_tile(
    values: &[u32],
    (width, height): (usize, usize),
    (left, top): (usize, usize),
    (tw, th): (usize, usize),
) -> Vec<u32> {
    let mut pixels = vec![0u32; tw * th];
    for y in 0..th.min(height - top) {
        let from = (top + y) * width + left;
        let count = tw.min(width - left);
        pixels[y * tw..y * tw + count].copy_from_slice(&values[from..from + count]);
    }
    pixels
}

impl Editor {
    /// Adds the selected pixels of the active layer to a tileset as tiles, as one step. The
    /// first new tile's id is the outcome's `created`. Nothing changes, and there is no step,
    /// if every tile is clear or already in the tileset.
    pub(crate) fn tiles_from_selection(
        &mut self,
        tileset: TilesetId,
    ) -> Result<Outcome, LimitError> {
        let set = self.doc.tileset(tileset)?;
        if self.doc.layers[self.layer].mode != set.mode() {
            return Err(LimitError::TilemapMode);
        }
        let clip = self.copy_selected().ok_or(LimitError::NothingToCapture)?;
        let values = match set.mode() {
            ColorMode::Palette => clip.ids.as_deref().ok_or(LimitError::NothingToCapture)?,
            ColorMode::Rgba => &clip.colors[..],
        };
        let existing: Vec<Arc<Image>> = set.tiles().iter().map(|t| t.image.clone()).collect();
        let block = (values, clip.width, clip.height);
        let room = Room {
            tiles: MAX_TILES.saturating_sub(existing.len()),
            bytes: MAX_TILE_BYTES.saturating_sub(self.doc.stored_tile_bytes()),
        };
        let fresh = cut_tiles(block, set.tile_size(), &existing, fnv, room)?;
        if fresh.is_empty() {
            return Ok(Outcome::UNCHANGED);
        }
        let first = self.transact(|d| {
            let mut first = None;
            for (image, _) in &fresh {
                let id = d.add_tile(tileset, image.clone())?;
                first.get_or_insert(id);
            }
            Ok(first)
        })?;
        Ok(Outcome {
            changed: true,
            created: first.map(|id| id.get()),
        })
    }
}
