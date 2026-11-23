//! A full-canvas picture made of shared, immutable tiles, as it is being built.
//!
//! Reading a file or taking in pixels goes through here. A cel keeps the [`Image`] this becomes.

use crate::image::Image;
use crate::tile::{RawTile, Tile, TILE, TILE_AREA, TILE_SHIFT};
use std::sync::Arc;

pub type Cell = Option<Arc<Tile>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    width: usize,
    height: usize,
    tiles_wide: usize,
    cells: Vec<Cell>,
}

impl Grid {
    pub fn new(width: usize, height: usize) -> Self {
        let tiles_wide = width.div_ceil(TILE);
        let count = tiles_wide * height.div_ceil(TILE);
        Grid {
            width,
            height,
            tiles_wide,
            cells: vec![None; count],
        }
    }

    /// Builds a grid from a full top-to-bottom cell buffer of `width * height` values.
    pub fn from_cells(width: usize, height: usize, px: &[u32]) -> Self {
        let mut grid = Grid::new(width, height);
        let mut raw = [0u32; TILE_AREA];
        for index in 0..grid.cells.len() {
            raw.fill(0);
            let (x0, y0) = grid.tile_origin(index);
            for y in 0..TILE.min(height - y0) {
                let start = (y0 + y) * width + x0;
                let count = TILE.min(width - x0);
                raw[y * TILE..y * TILE + count].copy_from_slice(&px[start..start + count]);
            }
            grid.cells[index] = Tile::pack(&raw).map(Arc::new);
        }
        grid
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn tiles_wide(&self) -> usize {
        self.tiles_wide
    }

    /// The picture as an image with a table only as large as what is drawn.
    pub fn into_image(self) -> Image {
        Image::from_table(self.tiles_wide, self.cells)
    }

    /// Replaces a cell and returns the previous one.
    pub fn replace(&mut self, index: usize, cell: Cell) -> Cell {
        std::mem::replace(&mut self.cells[index], cell)
    }

    fn tile_origin(&self, index: usize) -> (usize, usize) {
        (
            index % self.tiles_wide * TILE,
            index / self.tiles_wide * TILE,
        )
    }

    #[inline]
    fn tile_index(&self, x: usize, y: usize) -> usize {
        (y >> TILE_SHIFT) * self.tiles_wide + (x >> TILE_SHIFT)
    }

    /// Reads one cell, returning 0 outside the image.
    #[inline]
    pub fn get(&self, x: usize, y: usize) -> u32 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        match &self.cells[self.tile_index(x, y)] {
            Some(tile) => tile.get(((y & (TILE - 1)) << TILE_SHIFT) | (x & (TILE - 1))),
            None => 0,
        }
    }

    /// Decodes a tile into `out`, clearing it when the tile is empty.
    #[inline]
    fn decode(&self, index: usize, out: &mut RawTile) {
        match &self.cells[index] {
            Some(tile) => tile.decode(out),
            None => out.fill(0),
        }
    }

    /// Writes every cell to a top-to-bottom buffer of `width * height` values.
    pub fn to_cells(&self) -> Vec<u32> {
        let mut px = vec![0u32; self.width * self.height];
        let mut raw = [0u32; TILE_AREA];
        for index in 0..self.cells.len() {
            self.decode(index, &mut raw);
            let (x0, y0) = self.tile_origin(index);
            for y in 0..TILE.min(self.height - y0) {
                let start = (y0 + y) * self.width + x0;
                let count = TILE.min(self.width - x0);
                px[start..start + count].copy_from_slice(&raw[y * TILE..y * TILE + count]);
            }
        }
        px
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_round_trip_for_non_multiple_sizes() {
        let (w, h) = (37, 23);
        let px: Vec<u32> = (0..w * h).map(|i| (i as u32 % 5) * 3).collect();
        let grid = Grid::from_cells(w, h, &px);
        assert_eq!(grid.to_cells(), px);
        assert_eq!(grid.get(36, 22), px[w * h - 1]);
    }

    #[test]
    fn reading_outside_returns_transparent() {
        let grid = Grid::from_cells(4, 4, &[9; 16]);
        assert_eq!(grid.get(4, 0), 0);
        assert_eq!(grid.get(0, 4), 0);
    }

    #[test]
    fn blank_image_allocates_no_tiles() {
        let grid = Grid::from_cells(64, 64, &[0; 64 * 64]);
        assert!(grid.into_image().is_blank());
    }
}
