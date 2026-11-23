//! The mutable working area for the stroke in progress.
//!
//! Nothing here reaches the document until the stroke is committed, so tools can freely
//! revise pixels they already placed.

use crate::tile::{TILE, TILE_AREA, TILE_SHIFT};

pub struct OverlayTile {
    px: [u32; TILE_AREA],
    mask: [u64; 4],
}

impl OverlayTile {
    fn blank() -> Box<Self> {
        Box::new(OverlayTile {
            px: [0; TILE_AREA],
            mask: [0; 4],
        })
    }

    /// Overwrites cells of `out` where this overlay has placed a value.
    #[inline]
    pub fn apply(&self, out: &mut [u32; TILE_AREA]) {
        for (word_index, &word) in self.mask.iter().enumerate() {
            let mut bits = word;
            while bits != 0 {
                let i = word_index * 64 + bits.trailing_zeros() as usize;
                out[i] = self.px[i];
                bits &= bits - 1;
            }
        }
    }

    /// The value placed in cell `i` of the tile (row by row), if one was placed.
    #[inline]
    pub(crate) fn placed(&self, i: usize) -> Option<u32> {
        let set = self
            .mask
            .get(i >> 6)
            .is_some_and(|w| w >> (i & 63) & 1 != 0);
        set.then(|| self.px[i])
    }

    fn is_empty(&self) -> bool {
        self.mask == [0; 4]
    }
}

pub struct Overlay {
    width: usize,
    height: usize,
    tiles_wide: usize,
    tiles: Vec<Option<Box<OverlayTile>>>,
    touched: Vec<u32>,
    // Boxes move between `tiles` and `pool` without copying a tile's kilobyte of pixels.
    #[allow(clippy::vec_box)]
    pool: Vec<Box<OverlayTile>>,
}

impl Overlay {
    pub fn new(width: usize, height: usize) -> Self {
        let tiles_wide = width.div_ceil(TILE);
        let count = tiles_wide * height.div_ceil(TILE);
        let tiles = std::iter::repeat_with(|| None).take(count).collect();
        Overlay {
            width,
            height,
            tiles_wide,
            tiles,
            touched: Vec::new(),
            pool: Vec::new(),
        }
    }

    /// The canvas size the overlay was made for.
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn tiles_wide(&self) -> usize {
        self.tiles_wide
    }

    /// Which tile of the canvas holds a position inside it.
    #[inline]
    pub fn tile_index(&self, x: usize, y: usize) -> usize {
        (y >> TILE_SHIFT) * self.tiles_wide + (x >> TILE_SHIFT)
    }

    #[inline]
    fn locate(&self, x: usize, y: usize) -> (usize, usize) {
        let tile = (y >> TILE_SHIFT) * self.tiles_wide + (x >> TILE_SHIFT);
        (tile, ((y & (TILE - 1)) << TILE_SHIFT) | (x & (TILE - 1)))
    }

    /// Places a value. Coordinates outside the image are ignored.
    #[inline]
    pub fn put(&mut self, x: i32, y: i32, value: u32) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let (tile, cell) = self.locate(x as usize, y as usize);
        if self.tiles[tile].is_none() {
            let fresh = self.pool.pop().unwrap_or_else(OverlayTile::blank);
            self.tiles[tile] = Some(fresh);
            self.touched.push(tile as u32);
        }
        if let Some(t) = &mut self.tiles[tile] {
            t.px[cell] = value;
            t.mask[cell >> 6] |= 1 << (cell & 63);
        }
    }

    /// Takes a previously placed value back out.
    #[inline]
    pub fn remove(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let (tile, cell) = self.locate(x as usize, y as usize);
        if let Some(t) = &mut self.tiles[tile] {
            t.mask[cell >> 6] &= !(1 << (cell & 63));
        }
    }

    pub fn tile(&self, index: usize) -> Option<&OverlayTile> {
        self.tiles.get(index).and_then(|t| t.as_deref())
    }

    /// Tiles holding at least one value, plus any emptied since they were created.
    pub fn touched(&self) -> &[u32] {
        &self.touched
    }

    pub fn is_empty(&self) -> bool {
        self.touched
            .iter()
            .all(|&t| self.tiles[t as usize].as_ref().is_none_or(|o| o.is_empty()))
    }

    /// Discards everything, keeping buffers for the next stroke.
    pub fn clear(&mut self) {
        for t in self.touched.drain(..) {
            if let Some(mut tile) = self.tiles[t as usize].take() {
                tile.mask = [0; 4];
                self.pool.push(tile);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_only_touches_placed_cells() {
        let mut o = Overlay::new(32, 32);
        o.put(1, 2, 42);
        let mut out = [7u32; TILE_AREA];
        o.tile(0).expect("tile").apply(&mut out);
        assert_eq!(out[2 * 16 + 1], 42);
        assert_eq!(out[0], 7);
    }

    #[test]
    fn placing_transparent_is_distinct_from_untouched() {
        let mut o = Overlay::new(16, 16);
        o.put(0, 0, 0);
        let mut out = [7u32; TILE_AREA];
        o.tile(0).expect("tile").apply(&mut out);
        assert_eq!(out[0], 0);
        assert_eq!(out[1], 7);
    }

    #[test]
    fn remove_undoes_a_put() {
        let mut o = Overlay::new(16, 16);
        o.put(3, 3, 9);
        o.remove(3, 3);
        assert!(o.is_empty());
    }

    #[test]
    fn out_of_bounds_is_ignored() {
        let mut o = Overlay::new(16, 16);
        o.put(-1, 0, 1);
        o.put(16, 0, 1);
        o.put(0, 16, 1);
        assert!(o.touched().is_empty());
    }

    #[test]
    fn clear_recycles_tiles_without_stale_values() {
        let mut o = Overlay::new(16, 16);
        o.put(0, 0, 5);
        o.clear();
        assert!(o.tile(0).is_none());
        o.put(1, 0, 6);
        let mut out = [7u32; TILE_AREA];
        o.tile(0).expect("tile").apply(&mut out);
        assert_eq!(out[0], 7, "old mask must not leak");
    }
}
