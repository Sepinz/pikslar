//! The pixels of tileset tiles, kept decoded for whoever draws maps over and over.
//!
//! A tile image is stored packed, so reading one pixel of it costs a match and a few shifts.
//! Drawing a map reads every pixel of every placed tile, on every redraw, so the screen keeps
//! each small tile's pixels as a plain row-by-row array instead, made the first time the tile
//! is drawn and made again whenever its image is another one.
//!
//! An entry remembers the image it was made from by a weak reference. Comparing that with the
//! tile's image is one pointer comparison, and while the weak reference lives the image's
//! allocation is not given back, so another image can never take its address and pass for it.
//! The weak reference keeps none of the image's pixels alive.
//!
//! Only tiles of at most 16 by 16 pixels are kept, and the cache never holds more than
//! [`MAX_CACHED_PIXELS`]; whatever does not fit is drawn from the tileset as before. The pixels
//! drawn are the same either way.

use super::sample::Placed;
use crate::image::Image;
use crate::tile::TILE;
use crate::tileset::{Tileset, TilesetId, MAX_TILESETS};
use std::sync::{Arc, Weak};

/// The most pixels the cache holds across all tilesets: 4 MiB. The stress map's 2,000 tiles of
/// 8 by 8 take 128,000 of them.
pub const MAX_CACHED_PIXELS: usize = 1 << 20;

/// The decoded tiles of one tileset, by tile id: id `n`'s pixels start at `n * w * h`.
#[derive(Debug)]
struct SetPixels {
    set: TilesetId,
    size: (usize, usize),
    made_from: Vec<Weak<Image>>,
    px: Vec<u32>,
}

/// Decoded tileset tiles for drawing maps. See the module notes.
#[derive(Debug, Default)]
pub struct TileCache {
    sets: Vec<SetPixels>,
    held: usize,
}

impl TileCache {
    pub const fn new() -> Self {
        TileCache {
            sets: Vec::new(),
            held: 0,
        }
    }

    /// The bytes the decoded pixels take.
    pub fn held_bytes(&self) -> usize {
        self.held * 4
    }

    /// Lets go of everything.
    pub fn clear(&mut self) {
        *self = TileCache::new();
    }

    /// Where the entries of `tiles` are, made empty if the tileset's tile size is not theirs.
    /// `None` for tiles too large to keep.
    fn set_slot(&mut self, tiles: &Tileset) -> Option<usize> {
        let size = tiles.tile_size();
        if size.0 > TILE || size.1 > TILE {
            return None;
        }
        let id = tiles.id();
        match self.sets.iter().position(|s| s.set == id) {
            Some(at) if self.sets[at].size == size => Some(at),
            Some(at) => {
                self.held -= self.sets[at].px.len();
                self.sets[at] = SetPixels::new(id, size);
                Some(at)
            }
            None => {
                // Tilesets come and go; the entries of ones long gone are dropped wholesale.
                if self.sets.len() >= 2 * MAX_TILESETS {
                    self.clear();
                }
                self.sets.push(SetPixels::new(id, size));
                Some(self.sets.len() - 1)
            }
        }
    }

    /// The pixels of what `placed` places, row by row in the tile's own size, decoded now if
    /// they were not kept already. `None` when they do not fit in the cache.
    pub(super) fn pixels(&mut self, tiles: &Tileset, placed: &Placed<'_>) -> Option<&[u32]> {
        let slot = self.set_slot(tiles)?;
        let set = &mut self.sets[slot];
        let id = placed.id.get() as usize;
        let area = set.size.0 * set.size.1;
        let end = (id + 1).checked_mul(area)?;
        if end > set.px.len() {
            let more = end - set.px.len();
            if self.held + more > MAX_CACHED_PIXELS {
                return None;
            }
            self.held += more;
            set.px.resize(end, 0);
            set.made_from.resize(id + 1, Weak::new());
        }
        let current = Arc::as_ptr(placed.arc);
        let pixels = &mut set.px[id * area..end];
        if set.made_from[id].as_ptr() != current {
            let w = set.size.0;
            for (i, pixel) in pixels.iter_mut().enumerate() {
                *pixel = placed.image.get((i % w) as i32, (i / w) as i32);
            }
            set.made_from[id] = Arc::downgrade(placed.arc);
        }
        Some(pixels)
    }
}

impl SetPixels {
    fn new(set: TilesetId, size: (usize, usize)) -> Self {
        SetPixels {
            set,
            size,
            made_from: Vec::new(),
            px: Vec::new(),
        }
    }
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
