//! Editing tiles through the canvas (Pixels mode): the pixels a stroke paints on a tilemap
//! layer go into the tiles its cells place.
//!
//! Each painted canvas pixel is carried through its cell's inverse orientation to a pixel of
//! the cell's tile, the way [`crate::Document::layer_pixel`] reads it back, so the pixel shows
//! where it was painted. The pixels are taken in canvas row order, top to bottom and left to
//! right, and the last one written to a tile pixel wins. Pixels over an empty cell are dropped
//! and counted. Only the storage tiles of a tile image that a stroke changed are rebuilt, so the
//! new image shares the rest with the old one that history keeps.

use super::cell::MapCell;
use super::sample::INVERSES;
use crate::document::Document;
use crate::grid::Cell;
use crate::image::{Image, TileKey};
use crate::layer::Cel;
use crate::orient::Orientation;
use crate::overlay::{Overlay, OverlayTile};
use crate::tile::{RawTile, Tile, TILE, TILE_AREA, TILE_SHIFT};
use crate::tileset::{TileId, Tileset};
use std::collections::HashMap;
use std::sync::Arc;

/// What a stroke's pixels do to the tiles under them.
#[derive(Debug, Default)]
pub(crate) struct Through {
    /// The new image of each tile whose pixels changed, each tile once.
    pub(crate) images: Vec<(TileId, Arc<Image>)>,
    /// How many painted pixels fell where no tile is placed.
    pub(crate) dropped: usize,
    /// How many fell on a placed tile, whether or not they changed it.
    pub(crate) landed: usize,
}

/// Works out the tiles that the pixels of `overlay`, painted on the map of `layer` in `frame`,
/// change. Nothing is written: the caller swaps the images in as one step.
pub(crate) fn write_through(
    doc: &Document,
    layer: usize,
    frame: usize,
    overlay: &Overlay,
) -> Through {
    let found = doc.layers.get(layer).and_then(|l| {
        let tiles = doc.tileset(l.tileset()?).ok()?;
        Some((l.cels().get(frame)?, tiles))
    });
    let Some((cel, tiles)) = found else {
        return Through::default();
    };
    let mut target = Target::new(cel, tiles);
    let mut edits = Edits::default();
    let (mut dropped, mut landed) = (0, 0);
    painted_in_row_order(overlay, |x, y, value| match target.locate(x, y) {
        Some((tile, at)) => {
            landed += 1;
            edits.put(tiles, tile, at, value);
        }
        None => dropped += 1,
    });
    Through {
        images: edits.finish(),
        dropped,
        landed,
    }
}

/// Calls `paint` with every pixel the overlay holds, row by row from the top, each row from the
/// left, whatever order its storage tiles were touched in.
fn painted_in_row_order(overlay: &Overlay, mut paint: impl FnMut(i32, i32, u32)) {
    let wide = overlay.tiles_wide().max(1);
    let height = overlay.size().1;
    let mut order = overlay.touched().to_vec();
    order.sort_unstable();
    order.dedup();
    for band in order.chunk_by(|a, b| *a as usize / wide == *b as usize / wide) {
        let top = band[0] as usize / wide * TILE;
        for row in (0..TILE).take_while(|row| top + row < height) {
            for &index in band {
                let Some(tile) = overlay.tile(index as usize) else {
                    continue;
                };
                let left = index as usize % wide * TILE;
                row_of(tile, row, (left, top + row), &mut paint);
            }
        }
    }
}

/// The placed pixels of one row of an overlay tile, from the left.
#[inline]
fn row_of(
    tile: &OverlayTile,
    row: usize,
    (x, y): (usize, usize),
    paint: &mut impl FnMut(i32, i32, u32),
) {
    for column in 0..TILE {
        if let Some(value) = tile.placed(row * TILE + column) {
            paint((x + column) as i32, y as i32, value);
        }
    }
}

/// A tile a cell places, and the inverse of its turn.
type Placing = (TileId, Orientation);

/// Finds the tile pixel a canvas pixel shows, remembering the last cell looked up, since a
/// stroke's pixels come in runs along a cell.
struct Target<'a> {
    cel: &'a Cel,
    tiles: &'a Tileset,
    corner: (i32, i32),
    size: (usize, usize),
    last: Option<((i32, i32), Option<Placing>)>,
}

impl<'a> Target<'a> {
    fn new(cel: &'a Cel, tiles: &'a Tileset) -> Self {
        let (ox, oy) = cel.position();
        Target {
            cel,
            tiles,
            corner: (i32::from(ox), i32::from(oy)),
            size: tiles.tile_size(),
            last: None,
        }
    }

    /// The tile and its pixel that canvas position (`x`, `y`) shows, or `None` over an empty
    /// cell.
    fn locate(&mut self, x: i32, y: i32) -> Option<(TileId, (usize, usize))> {
        let (w, h) = self.size;
        let (tw, th) = (w as i32, h as i32);
        let (lx, ly) = (x - self.corner.0, y - self.corner.1);
        let at = (lx.div_euclid(tw), ly.div_euclid(th));
        let (tile, inverse) = match self.last {
            Some((seen, placed)) if seen == at => placed,
            _ => {
                let placed = self.placed(at);
                self.last = Some((at, placed));
                placed
            }
        }?;
        let (dx, dy) = (lx.rem_euclid(tw) as usize, ly.rem_euclid(th) as usize);
        Some((tile, inverse.apply(dx, dy, w, h)))
    }

    /// The tile cell `at` places and the inverse of its turn, if it places one the tileset has
    /// in a turn its tiles allow.
    fn placed(&self, (cx, cy): (i32, i32)) -> Option<Placing> {
        let cell = MapCell::from_bits(self.cel.image().get(cx, cy));
        let tile = cell.tile()?;
        self.tiles.tile(tile)?;
        let orientation = cell.orientation();
        let (w, h) = self.size;
        if orientation.swaps_axes() && w != h {
            return None;
        }
        Some((tile, INVERSES[usize::from(orientation.index())]))
    }
}

/// The tiles a stroke writes to, each with the storage tiles of its image decoded as written.
#[derive(Default)]
struct Edits {
    tiles: Vec<Rewrite>,
    /// Where each tile is in `tiles`, which keeps the order they were first written to.
    at: HashMap<TileId, usize>,
}

struct Rewrite {
    tile: TileId,
    image: Arc<Image>,
    blocks: Vec<Block>,
    /// Where each block is in `blocks`, by its storage tile.
    at: HashMap<(i32, i32), usize>,
}

/// One storage tile of a tile image: as it was, and as written.
struct Block {
    key: (i32, i32),
    before: Box<RawTile>,
    now: Box<RawTile>,
}

impl Edits {
    /// Writes `value` to pixel `at` of `tile`.
    fn put(&mut self, tiles: &Tileset, tile: TileId, (x, y): (usize, usize), value: u32) {
        let rewrite = match self.at.get(&tile) {
            Some(&i) => &mut self.tiles[i],
            None => {
                let Some(entry) = tiles.tile(tile) else {
                    return;
                };
                self.at.insert(tile, self.tiles.len());
                self.tiles.push(Rewrite {
                    tile,
                    image: entry.image.clone(),
                    blocks: Vec::new(),
                    at: HashMap::new(),
                });
                let last = self.tiles.len() - 1;
                &mut self.tiles[last]
            }
        };
        let key = ((x >> TILE_SHIFT) as i32, (y >> TILE_SHIFT) as i32);
        let block = rewrite.block(key);
        block.now[((y & (TILE - 1)) << TILE_SHIFT) | (x & (TILE - 1))] = value;
    }

    /// The new image of every tile whose pixels changed.
    fn finish(self) -> Vec<(TileId, Arc<Image>)> {
        self.tiles
            .into_iter()
            .filter_map(|rewrite| Some((rewrite.tile, rewrite.rebuilt()?)))
            .collect()
    }
}

impl Rewrite {
    /// The block of the image at `key`, decoded the first time it is written to.
    fn block(&mut self, key: (i32, i32)) -> &mut Block {
        let index = match self.at.get(&key) {
            Some(&i) => i,
            None => {
                let mut before = Box::new([0; TILE_AREA]);
                self.image.decode(key.0, key.1, &mut before);
                let now = before.clone();
                self.at.insert(key, self.blocks.len());
                self.blocks.push(Block { key, before, now });
                self.blocks.len() - 1
            }
        };
        &mut self.blocks[index]
    }

    /// The image with the changed blocks put in, or `None` if no pixel ends up different.
    fn rebuilt(self) -> Option<Arc<Image>> {
        let mut changes: Vec<(TileKey, Cell)> = self
            .blocks
            .iter()
            .filter(|b| b.now != b.before)
            .map(|b| {
                let key = TileKey {
                    x: b.key.0 as i16,
                    y: b.key.1 as i16,
                };
                (key, Tile::pack(&b.now).map(Arc::new))
            })
            .collect();
        if changes.is_empty() {
            return None;
        }
        let mut image = Image::clone(&self.image);
        image.swap_cells(&mut changes);
        Some(Arc::new(image))
    }
}

#[cfg(test)]
#[path = "write_through_tests.rs"]
mod tests;
