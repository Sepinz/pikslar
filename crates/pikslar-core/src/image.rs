//! A drawing held as shared tiles, in a table that covers only the tiles it uses.
//!
//! Coordinates are the drawing's own. The table is always tight: its outer rows and columns each
//! hold a tile, and a blank image has no table at all. That makes the form canonical, so two
//! images with the same content compare equal however they got there, and undo restores exactly
//! what was there.

use crate::grid::Cell;
use crate::tile::{RawTile, Tile, TILE, TILE_AREA, TILE_SHIFT};
use std::sync::Arc;

const IN_TILE: i32 = TILE as i32 - 1;

/// How far from its origin, in tiles either way, a table built from outside may reach: every
/// pixel position then fits an `i16`.
pub const MAX_TILE_REACH: i32 = 2048;

/// Where a tile is in a drawing's own coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileKey {
    pub x: i16,
    pub y: i16,
}

/// A rectangle of tiles, the end exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bounds {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl Bounds {
    fn around(key: TileKey) -> Self {
        let (x, y) = (key.x as i32, key.y as i32);
        Bounds {
            x0: x,
            y0: y,
            x1: x + 1,
            y1: y + 1,
        }
    }

    fn including(self, key: TileKey) -> Self {
        let (x, y) = (key.x as i32, key.y as i32);
        Bounds {
            x0: self.x0.min(x),
            y0: self.y0.min(y),
            x1: self.x1.max(x + 1),
            y1: self.y1.max(y + 1),
        }
    }
}

/// Tiles that show nothing are stored as none, so that equal pictures compare equal and the
/// table can be trimmed to what is drawn.
fn drop_blank_tiles(cells: &mut [Cell]) {
    for cell in cells.iter_mut() {
        if cell.as_deref().is_some_and(Tile::draws_nothing) {
            *cell = None;
        }
    }
}

/// Reads pixels through the tile it read last, so that neighbours cost one comparison each.
pub(crate) struct Reader<'a> {
    image: &'a Image,
    tx: i32,
    ty: i32,
    tile: Option<&'a Tile>,
}

impl Reader<'_> {
    /// The same answer as [`Image::get`].
    #[inline]
    pub(crate) fn get(&mut self, x: i32, y: i32) -> u32 {
        let (tx, ty) = (x >> TILE_SHIFT, y >> TILE_SHIFT);
        if (tx, ty) != (self.tx, self.ty) {
            (self.tx, self.ty) = (tx, ty);
            self.tile = self.image.tile(tx, ty);
        }
        match self.tile {
            Some(tile) => tile.get((((y & IN_TILE) << TILE_SHIFT) | (x & IN_TILE)) as usize),
            None => 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Image {
    x0: i16,
    y0: i16,
    tw: u16,
    th: u16,
    cells: Vec<Cell>,
}

impl Image {
    /// True if both images hold equal tiles in the same places: mostly the very same tiles,
    /// seen without a pixel being looked at. Tiles read back from disk are equal but not the
    /// same, and are compared.
    pub(crate) fn same_tiles(&self, other: &Image) -> bool {
        let same = |(a, b): (&Cell, &Cell)| match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
            _ => false,
        };
        (self.x0, self.y0, self.tw, self.th) == (other.x0, other.y0, other.tw, other.th)
            && self.cells.iter().zip(&other.cells).all(same)
    }

    /// The tiles this image holds, blank places left out.
    pub(crate) fn tiles(&self) -> impl Iterator<Item = &Arc<crate::tile::Tile>> {
        self.cells.iter().flatten()
    }

    /// An image with nothing drawn, which holds no table.
    pub fn blank() -> Self {
        Image::default()
    }

    /// The image held by a full table of rows `tiles_wide` long, starting at tile (0, 0).
    pub(crate) fn from_table(tiles_wide: usize, mut cells: Vec<Cell>) -> Self {
        drop_blank_tiles(&mut cells);
        let mut image = Image {
            x0: 0,
            y0: 0,
            tw: tiles_wide as u16,
            th: (cells.len() / tiles_wide.max(1)) as u16,
            cells,
        };
        image.trim();
        image
    }

    /// The image held by a table of rows `tiles_wide` long whose first tile is at (`x0`, `y0`),
    /// as a file stores it. `None` unless the rows are whole and every tile lies within
    /// [`MAX_TILE_REACH`] of the origin. Blank edges are trimmed and no tile is copied.
    pub fn from_tiles(x0: i16, y0: i16, tiles_wide: u16, mut cells: Vec<Cell>) -> Option<Image> {
        if cells.is_empty() {
            return Some(Image::blank());
        }
        let tw = tiles_wide as usize;
        if tw == 0 || cells.len() % tw != 0 {
            return None;
        }
        let th = cells.len() / tw;
        let inside = |start: i16, length: usize| {
            let start = start as i32;
            start >= -MAX_TILE_REACH && start + length as i32 <= MAX_TILE_REACH
        };
        if !inside(x0, tw) || th > u16::MAX as usize || !inside(y0, th) {
            return None;
        }
        drop_blank_tiles(&mut cells);
        let mut image = Image {
            x0,
            y0,
            tw: tiles_wide,
            th: th as u16,
            cells,
        };
        image.trim();
        Some(image)
    }

    /// True when nothing is drawn here. Costs the same at any size.
    #[inline]
    pub fn is_blank(&self) -> bool {
        self.cells.is_empty()
    }

    /// Where the table sits and how big it is: first tile x and y, then width and height in tiles.
    pub fn table(&self) -> (i32, i32, usize, usize) {
        (
            self.x0 as i32,
            self.y0 as i32,
            self.tw as usize,
            self.th as usize,
        )
    }

    /// The smallest rectangle holding every drawn pixel, in the image's own coordinates: x, y,
    /// width and height. The table is tight, so each side's pixel is in a tile of that side's
    /// row or column, and the first such tile that reaches the table's own edge settles it.
    pub fn drawn_bounds(&self) -> Option<(i32, i32, i32, i32)> {
        let (tx0, ty0, tw, th) = self.table();
        if tw == 0 {
            return None;
        }
        let (tx1, ty1) = (tx0 + tw as i32 - 1, ty0 + th as i32 - 1);
        let mut raw: RawTile = [0; TILE_AREA];
        let mut side = |tiles: &mut dyn Iterator<Item = (i32, i32)>, of: usize, low: bool| {
            let (mut best, edge) = if low { (IN_TILE, 0) } else { (0, IN_TILE) };
            for (tx, ty) in tiles {
                if let Some(tile) = self.tile(tx, ty) {
                    let found = tile_extent(tile, &mut raw)[of];
                    best = if low {
                        best.min(found)
                    } else {
                        best.max(found)
                    };
                    if best == edge {
                        break;
                    }
                }
            }
            best
        };
        let x0 = (tx0 << TILE_SHIFT) + side(&mut (ty0..=ty1).map(|ty| (tx0, ty)), 0, true);
        let y0 = (ty0 << TILE_SHIFT) + side(&mut (tx0..=tx1).map(|tx| (tx, ty0)), 1, true);
        let x1 = (tx1 << TILE_SHIFT) + side(&mut (ty0..=ty1).map(|ty| (tx1, ty)), 2, false);
        let y1 = (ty1 << TILE_SHIFT) + side(&mut (tx0..=tx1).map(|tx| (tx, ty1)), 3, false);
        Some((x0, y0, x1 - x0 + 1, y1 - y0 + 1))
    }

    #[inline]
    fn slot(&self, tx: i32, ty: i32) -> Option<usize> {
        let (column, row) = (tx - self.x0 as i32, ty - self.y0 as i32);
        // A position before the table wraps to a large number, so one comparison covers both.
        ((column as u32) < self.tw as u32 && (row as u32) < self.th as u32)
            .then(|| row as usize * self.tw as usize + column as usize)
    }

    /// The tile at a position, if anything is drawn there.
    #[inline]
    pub fn tile(&self, tx: i32, ty: i32) -> Option<&Tile> {
        self.slot(tx, ty).and_then(|i| self.cells[i].as_deref())
    }

    /// Decodes a tile into `out`, clearing it when nothing is drawn there.
    #[inline]
    pub fn decode(&self, tx: i32, ty: i32, out: &mut RawTile) {
        match self.tile(tx, ty) {
            Some(tile) => tile.decode(out),
            None => out.fill(0),
        }
    }

    /// Reads one cell, returning 0 where nothing is drawn.
    #[inline]
    pub fn get(&self, x: i32, y: i32) -> u32 {
        match self.tile(x >> TILE_SHIFT, y >> TILE_SHIFT) {
            Some(tile) => tile.get((((y & IN_TILE) << TILE_SHIFT) | (x & IN_TILE)) as usize),
            None => 0,
        }
    }

    /// A way to read many nearby pixels quickly.
    pub(crate) fn reader(&self) -> Reader<'_> {
        Reader {
            image: self,
            tx: i32::MIN,
            ty: i32::MIN,
            tile: None,
        }
    }

    /// What this image holds in its table of tile references.
    pub fn table_bytes(&self) -> usize {
        self.cells.len() * std::mem::size_of::<Cell>()
    }

    /// What this image holds in memory, counting every tile as its own.
    pub fn heap_size(&self) -> usize {
        self.table_bytes()
            + self
                .cells
                .iter()
                .flatten()
                .map(|tile| tile.heap_size())
                .sum::<usize>()
    }

    /// True when every tile lies inside a canvas of this size.
    pub fn fits(&self, width: usize, height: usize) -> bool {
        let (x, y, w, h) = self.table();
        self.is_blank()
            || (x >= 0
                && y >= 0
                && x as usize + w <= width.div_ceil(TILE)
                && y as usize + h <= height.div_ceil(TILE))
    }

    /// Writes this image to a top-to-bottom buffer of `width * height` values starting at (0, 0).
    pub fn to_cells(&self, width: usize, height: usize) -> Vec<u32> {
        let mut px = vec![0u32; width * height];
        let mut raw = [0u32; TILE_AREA];
        let (table_x, table_y, tw, th) = self.table();
        for row in 0..th as i32 {
            for column in 0..tw as i32 {
                let (tx, ty) = (table_x + column, table_y + row);
                let Some(tile) = self.tile(tx, ty) else {
                    continue;
                };
                let (x0, y0) = (tx * TILE as i32, ty * TILE as i32);
                if x0 < 0 || y0 < 0 || x0 as usize >= width || y0 as usize >= height {
                    continue;
                }
                let (x0, y0) = (x0 as usize, y0 as usize);
                tile.decode(&mut raw);
                let count = TILE.min(width - x0);
                for y in 0..TILE.min(height - y0) {
                    let start = (y0 + y) * width + x0;
                    px[start..start + count].copy_from_slice(&raw[y * TILE..y * TILE + count]);
                }
            }
        }
        px
    }

    /// The tiles that hold something, as they are shared: a caller that remembers which it has
    /// seen reads a tile used by many drawings once.
    pub(crate) fn drawn_tiles(&self) -> impl Iterator<Item = &Arc<Tile>> {
        self.cells.iter().flatten()
    }

    /// A copy in which each drawn tile is what `f` makes of it. `f` may hand back the very tile
    /// it was given, so tiles it leaves alone stay shared with this image, and may remember its
    /// answers so that a tile used by several drawings is rebuilt once. Tiles that come back
    /// blank are dropped and the table is trimmed.
    pub(crate) fn map_tiles(&self, mut f: impl FnMut(&Arc<Tile>) -> Cell) -> Image {
        let mut out = self.clone();
        for cell in out.cells.iter_mut() {
            let Some(tile) = cell else {
                continue;
            };
            let mapped = f(tile);
            *cell = mapped;
        }
        drop_blank_tiles(&mut out.cells);
        out.trim();
        out
    }

    /// Applies `f` to every stored value that is not 0, rebuilding tiles as needed.
    pub fn map_cells(&self, mut f: impl FnMut(u32) -> u32) -> Image {
        let mut out = self.clone();
        let mut raw = [0u32; TILE_AREA];
        for cell in out.cells.iter_mut() {
            let Some(tile) = cell else {
                continue;
            };
            tile.decode(&mut raw);
            for p in raw.iter_mut().filter(|p| **p != 0) {
                *p = f(*p);
            }
            *cell = Tile::pack(&raw).map(Arc::new);
        }
        out.trim();
        out
    }

    /// Puts each new cell in place and leaves what it replaced in `changes`, so that applying the
    /// result restores this image exactly. The table grows once, for all of them.
    pub fn swap_cells(&mut self, changes: &mut [(TileKey, Cell)]) {
        self.grow_for(changes);
        let mut emptied = false;
        for (key, cell) in changes.iter_mut() {
            // Clearing a tile the table does not reach has nothing to do.
            if let Some(index) = self.slot(key.x as i32, key.y as i32) {
                std::mem::swap(&mut self.cells[index], cell);
                emptied |= self.cells[index].is_none();
            }
        }
        if emptied {
            self.trim();
        }
    }

    fn bounds(&self) -> Option<Bounds> {
        (!self.is_blank()).then(|| {
            let (x, y, w, h) = self.table();
            Bounds {
                x0: x,
                y0: y,
                x1: x + w as i32,
                y1: y + h as i32,
            }
        })
    }

    fn grow_for(&mut self, changes: &[(TileKey, Cell)]) {
        let now = self.bounds();
        let needed = changes
            .iter()
            .filter(|(_, cell)| cell.is_some())
            .fold(now, |bounds, (key, _)| {
                Some(bounds.map_or(Bounds::around(*key), |b| b.including(*key)))
            });
        if let Some(needed) = needed.filter(|b| Some(*b) != now) {
            self.resize(needed);
        }
    }

    /// The rectangle of tiles actually in use.
    fn used(&self) -> Option<Bounds> {
        let (tw, th) = (self.tw as usize, self.th as usize);
        let filled_row = |r: &usize| self.cells[r * tw..(r + 1) * tw].iter().any(Option::is_some);
        let top = (0..th).find(filled_row)?;
        let bottom = (0..th).rfind(filled_row)?;
        let filled_column = |c: &usize| (top..=bottom).any(|r| self.cells[r * tw + c].is_some());
        let left = (0..tw).find(filled_column)?;
        let right = (0..tw).rfind(filled_column)?;
        let (x, y) = (self.x0 as i32, self.y0 as i32);
        Some(Bounds {
            x0: x + left as i32,
            y0: y + top as i32,
            x1: x + right as i32 + 1,
            y1: y + bottom as i32 + 1,
        })
    }

    /// Restores the tight form after tiles were emptied.
    fn trim(&mut self) {
        match self.used() {
            None => *self = Image::blank(),
            Some(used) if Some(used) != self.bounds() => self.resize(used),
            Some(_) => {}
        }
    }

    /// Moves the table to new edges. Tiles are not touched, and any left outside must be empty.
    fn resize(&mut self, to: Bounds) {
        let width = (to.x1 - to.x0) as usize;
        let height = (to.y1 - to.y0) as usize;
        debug_assert!(width <= u16::MAX as usize && height <= u16::MAX as usize);
        let mut cells: Vec<Cell> = vec![None; width * height];
        let (x0, y0, tw, th) = self.table();
        let left = x0.max(to.x0);
        let right = (x0 + tw as i32).min(to.x1);
        for y in y0.max(to.y0)..(y0 + th as i32).min(to.y1) {
            if left >= right {
                break;
            }
            let count = (right - left) as usize;
            let from = (y - y0) as usize * tw + (left - x0) as usize;
            let into = (y - to.y0) as usize * width + (left - to.x0) as usize;
            for (new, old) in cells[into..into + count]
                .iter_mut()
                .zip(&mut self.cells[from..from + count])
            {
                *new = old.take();
            }
        }
        debug_assert!(self.cells.iter().all(Option::is_none));
        *self = Image {
            x0: to.x0 as i16,
            y0: to.y0 as i16,
            tw: width as u16,
            th: height as u16,
            cells,
        };
    }
}

/// Where a tile's drawn cells reach within it: least x, least y, greatest x, greatest y.
fn tile_extent(tile: &Tile, raw: &mut RawTile) -> [i32; 4] {
    if let Tile::Solid(_) = tile {
        return [0, 0, IN_TILE, IN_TILE];
    }
    tile.decode(raw);
    let mut found = [IN_TILE, IN_TILE, 0, 0];
    for (i, _) in raw.iter().enumerate().filter(|(_, &v)| v != 0) {
        let (x, y) = ((i & IN_TILE as usize) as i32, (i >> TILE_SHIFT) as i32);
        found = [
            found[0].min(x),
            found[1].min(y),
            found[2].max(x),
            found[3].max(y),
        ];
    }
    found
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;
