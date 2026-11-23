//! A brute-force reading of a tilemap, written straight from the definition of a cell, for the
//! tests to hold the real sampler against. It shares no code with the sampler: it keeps the
//! cells in its own table, turns tiles with Tiled's three flags worked out in the open, and
//! reads one canvas pixel at a time.

use crate::document::{ColorMode, Document};
use crate::grid::Grid;
use crate::image::Image;
use crate::layer::Cel;
use crate::tile::{Tile, TILE, TILE_AREA};
use crate::tileset::TilesetId;
use std::collections::HashMap;
use std::sync::Arc;

pub(super) struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next() >> 11) % u64::from(n.max(1))) as u32
    }
}

/// Tile sizes tried: squares and oblongs, below, at and past one storage tile.
pub(super) const SIZES: [(usize, usize); 11] = [
    (8, 8),
    (16, 16),
    (4, 4),
    (1, 1),
    (5, 3),
    (3, 7),
    (17, 17),
    (32, 16),
    (12, 20),
    (2, 2),
    (16, 8),
];

/// A random document with one tilemap layer, and the cells and tiles it was made from.
pub(super) struct Case {
    pub doc: Document,
    pub set: TilesetId,
    /// Pixels of tile `n` (id `n + 1`), row by row.
    pub tiles: Vec<Vec<u32>>,
    pub size: (usize, usize),
    pub cells: HashMap<(i32, i32), u32>,
    pub offset: (i32, i32),
}

/// The three Tiled flags of a cell value, as (diagonal, horizontal, vertical).
fn flags(bits: u32) -> (bool, bool, bool) {
    (
        bits & 0x2000_0000 != 0,
        bits & 0x8000_0000 != 0,
        bits & 0x4000_0000 != 0,
    )
}

/// The tile's pixels placed as Tiled says: the diagonal flip first, then horizontal, then
/// vertical. Placed from each source pixel to where it lands.
pub(super) fn place(source: &[u32], (w, h): (usize, usize), bits: u32) -> Vec<u32> {
    let (diagonal, horizontal, vertical) = flags(bits);
    let mut placed = vec![0u32; w * h];
    for sy in 0..h {
        for sx in 0..w {
            let (mut x, mut y, mut width, mut height) = (sx, sy, w, h);
            if diagonal {
                (x, y, width, height) = (y, x, height, width);
            }
            if horizontal {
                x = width - 1 - x;
            }
            if vertical {
                y = height - 1 - y;
            }
            placed[y * width + x] = source[sy * w + sx];
        }
    }
    placed
}

impl Case {
    /// The canvas pixel (`x`, `y`) of the map, as the definition of a cell gives it.
    pub fn pixel(&self, x: i32, y: i32) -> u32 {
        let (w, h) = (self.size.0 as i32, self.size.1 as i32);
        let (lx, ly) = (x - self.offset.0, y - self.offset.1);
        let (cx, cy) = (lx.div_euclid(w), ly.div_euclid(h));
        let bits = self.cells.get(&(cx, cy)).copied().unwrap_or(0);
        let id = (bits & 0x0fff_ffff) as usize;
        if id == 0 || id > self.tiles.len() {
            return 0;
        }
        let placed = place(&self.tiles[id - 1], self.size, bits);
        placed[ly.rem_euclid(h) as usize * self.size.0 + lx.rem_euclid(w) as usize]
    }

    /// True if the map places one of `ids` over canvas pixel (`x`, `y`).
    pub fn places_at(&self, x: i32, y: i32, ids: &[u32]) -> bool {
        let (w, h) = (self.size.0 as i32, self.size.1 as i32);
        let (cx, cy) = (
            (x - self.offset.0).div_euclid(w),
            (y - self.offset.1).div_euclid(h),
        );
        let id = self.cells.get(&(cx, cy)).copied().unwrap_or(0) & 0x0fff_ffff;
        id != 0 && ids.contains(&id)
    }
}

/// A map cel from a table of cells, as a stored image whose tiles reach below zero.
fn map_image(cells: &HashMap<(i32, i32), u32>) -> Image {
    let (first, wide) = (-1, 4);
    let mut stored = Vec::new();
    for ty in first..first + wide {
        for tx in first..first + wide {
            let mut raw = [0u32; TILE_AREA];
            for (i, v) in raw.iter_mut().enumerate() {
                let at = (
                    tx * TILE as i32 + (i % TILE) as i32,
                    ty * TILE as i32 + (i / TILE) as i32,
                );
                *v = cells.get(&at).copied().unwrap_or(0);
            }
            stored.push(Tile::pack(&raw).map(Arc::new));
        }
    }
    Image::from_tiles(first as i16, first as i16, wide as u16, stored).expect("a table in reach")
}

/// A random tilemap layer over a `width` by `height` canvas, from `seed`.
pub(super) fn random_case(seed: u64, (width, height): (usize, usize)) -> Case {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let size = SIZES[rng.below(SIZES.len() as u32) as usize];
    let mut doc = Document::new(width, height, ColorMode::Rgba).expect("a canvas");
    let set = doc
        .add_tileset("T", ColorMode::Rgba, size)
        .expect("a tileset");
    let mut tiles = Vec::new();
    for _ in 0..1 + rng.below(6) {
        let blank = rng.below(6) == 0;
        let solid = rng.below(4) == 0;
        let base = 0xff00_0000 | rng.below(0x00ff_ffff);
        let pixels: Vec<u32> = (0..size.0 * size.1)
            .map(|_| match (blank, solid) {
                (true, _) => 0,
                (_, true) => base,
                _ => rng.next() as u32 | 1,
            })
            .collect();
        let image = Grid::from_cells(size.0, size.1, &pixels).into_image();
        doc.add_tile(set, Arc::new(image)).expect("a tile");
        tiles.push(pixels);
    }
    let mut cells = HashMap::new();
    for cy in -16..48 {
        for cx in -16..48 {
            if rng.below(3) == 0 {
                cells.insert((cx, cy), random_cell(&mut rng, tiles.len(), size));
            }
        }
    }
    let offset = (rng.below(60) as i32 - 30, rng.below(60) as i32 - 30);
    doc.add_tilemap_layer(0, set).expect("a layer");
    let cel = Cel::at(map_image(&cells), offset.0, offset.1).expect("in reach");
    doc.put_cel(0, 0, cel).expect("a cel");
    Case {
        doc,
        set,
        tiles,
        size,
        cells,
        offset,
    }
}

/// A cell: usually a tile of the set turned any way the tile's shape allows, sometimes a tile
/// the set lacks or a flagged empty cell, and the reserved pinned bit now and then.
fn random_cell(rng: &mut Rng, tiles: usize, size: (usize, usize)) -> u32 {
    let id = match rng.below(10) {
        0 => 0,
        1 => tiles as u32 + 1 + rng.below(5),
        _ => 1 + rng.below(tiles as u32),
    };
    let mut turn = rng.below(8);
    if size.0 != size.1 {
        turn &= 0b110;
    }
    let pinned = if rng.below(5) == 0 { 1 << 28 } else { 0 };
    id | turn << 29 | pinned
}
