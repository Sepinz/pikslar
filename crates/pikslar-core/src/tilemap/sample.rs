//! Reading the pixels of any layer: the one way the compositor, the previews, the onion skin and
//! the exports see a layer, whatever its kind.
//!
//! A paint layer hands out its stored tiles. A tilemap layer builds the canvas tile from the
//! cells that cover it, one run of pixels at a time: each cell is looked up once, and the pixels
//! of its tile are copied through the cell's orientation by stepping along the source, with no
//! allocation and no lookup per pixel.

use super::cache::TileCache;
use super::cell::MapCell;
use crate::document::Document;
use crate::image::{Image, Reader};
use crate::layer::{Cel, Layer, LayerKind};
use crate::orient::Orientation;
use crate::tile::{RawTile, Tile, TILE};
use crate::tileset::{TileId, Tileset, TilesetId};
use std::sync::Arc;

const SIDE: i32 = TILE as i32;

/// The inverse of each orientation by its number, worked out when the program is built, since
/// finding one at run time takes dozens of steps and every cell needs it.
pub(super) const INVERSES: [Orientation; 8] = {
    let mut table = [Orientation::IDENTITY; 8];
    let mut i = 0;
    while i < 8 {
        table[i] = Orientation::ALL[i].inverse();
        i += 1;
    }
    table
};

impl Document {
    /// Decodes canvas tile (`tx`, `ty`) of `layer` in `frame` into `out`, in the layer's own
    /// colour mode, and says whether anything may be drawn there. When it says no, `out` is not
    /// written, so the caller keeps its own zeros.
    ///
    /// Paint and reference layers read their stored tiles, and tilemap layers the tiles their
    /// cells place, turned as the cells say.
    #[inline]
    pub fn layer_tile(
        &self,
        layer: &Layer,
        frame: usize,
        tx: usize,
        ty: usize,
        out: &mut RawTile,
    ) -> bool {
        if let LayerKind::Tilemap(set) = layer.kind {
            return self.map_tile(set, layer.cel(frame), (tx, ty), out);
        }
        let cel = layer.cel(frame);
        if !cel.has_tile(tx, ty) {
            return false;
        }
        cel.decode(tx, ty, out);
        true
    }

    /// [`Document::layer_tile`], with a tilemap layer's tiles read through `cache`, which
    /// whoever draws the same maps again and again keeps. The pixels are the same.
    #[inline]
    pub fn layer_tile_cached(
        &self,
        (layer, frame): (&Layer, usize),
        (tx, ty): (usize, usize),
        out: &mut RawTile,
        cache: &mut TileCache,
    ) -> bool {
        let LayerKind::Tilemap(set) = layer.kind else {
            return self.layer_tile(layer, frame, tx, ty, out);
        };
        let cel = layer.cel(frame);
        match self.tilesets.get(set) {
            Some(tiles) if !cel.is_blank() => {
                let corner = (tx as i32 * SIDE, ty as i32 * SIDE);
                sample_through(cel, tiles, corner, out, Some(cache))
            }
            _ => false,
        }
    }

    /// The pixel of `layer` in `frame` at canvas position (`x`, `y`), 0 where nothing is drawn.
    #[inline]
    pub fn layer_pixel(&self, layer: &Layer, frame: usize, x: i32, y: i32) -> u32 {
        match layer.kind {
            LayerKind::Tilemap(set) => self.map_pixel(set, layer.cel(frame), x, y),
            _ => layer.cel(frame).get(x, y),
        }
    }

    #[inline(never)]
    fn map_tile(
        &self,
        set: TilesetId,
        cel: &Cel,
        (tx, ty): (usize, usize),
        out: &mut RawTile,
    ) -> bool {
        if cel.is_blank() {
            return false;
        }
        let Some(tiles) = self.tilesets.get(set) else {
            return false;
        };
        sample_window(cel, tiles, (tx as i32 * SIDE, ty as i32 * SIDE), out)
    }

    /// The pixel (`x`, `y`) of a cell of `set` as the cell places it, turned as it says, in the
    /// tileset's colour mode. 0 for an empty cell, a tile the tileset lacks, or a place outside
    /// the tile. For showing a cell that is not in a map yet, such as a stroke in progress.
    pub fn cell_pixel(&self, set: TilesetId, cell: MapCell, x: usize, y: usize) -> u32 {
        let Some(tiles) = self.tilesets.get(set) else {
            return 0;
        };
        let (w, h) = tiles.tile_size();
        let Some(placed) = Placed::of(tiles, cell).filter(|_| x < w && y < h) else {
            return 0;
        };
        let inverse = INVERSES[usize::from(placed.orientation.index())];
        let (sx, sy) = inverse.apply(x, y, w, h);
        placed.image.get(sx as i32, sy as i32)
    }

    #[inline(never)]
    fn map_pixel(&self, set: TilesetId, cel: &Cel, x: i32, y: i32) -> u32 {
        let Some(tiles) = self.tilesets.get(set) else {
            return 0;
        };
        let (w, h) = tiles.tile_size();
        let (ox, oy) = cel.position();
        let (lx, ly) = (x - i32::from(ox), y - i32::from(oy));
        let (tw, th) = (w as i32, h as i32);
        let cell = MapCell::from_bits(cel.image().get(lx.div_euclid(tw), ly.div_euclid(th)));
        let Some(placed) = Placed::of(tiles, cell) else {
            return 0;
        };
        let (dx, dy) = (lx.rem_euclid(tw) as usize, ly.rem_euclid(th) as usize);
        let inverse = INVERSES[usize::from(placed.orientation.index())];
        let (sx, sy) = inverse.apply(dx, dy, w, h);
        placed.image.get(sx as i32, sy as i32)
    }
}

/// A cell that shows something: its tile, the tile's image and how it is turned.
pub(super) struct Placed<'a> {
    pub(super) id: TileId,
    pub(super) arc: &'a Arc<Image>,
    pub(super) image: &'a Image,
    orientation: Orientation,
    size: (usize, usize),
}

impl<'a> Placed<'a> {
    /// What the cell places, or `None` for an empty cell, a tile the tileset lacks, a tile
    /// with nothing drawn, or a diagonal flip of a tile that is not square (which no sound
    /// document holds).
    #[inline]
    fn of(tiles: &'a Tileset, cell: MapCell) -> Option<Self> {
        let id = cell.tile()?;
        let entry = tiles.tile(id)?;
        let size = tiles.tile_size();
        let orientation = cell.orientation();
        if entry.image.is_blank() || (orientation.swaps_axes() && size.0 != size.1) {
            return None;
        }
        Some(Placed {
            id,
            arc: &entry.image,
            image: &entry.image,
            orientation,
            size,
        })
    }
}

/// The smallest canvas rectangle holding every cell of the map `cel` that is not empty: x, y,
/// width and height, past the canvas too. The cells it holds may still draw nothing.
pub(crate) fn map_bounds(cel: &Cel, tiles: &Tileset) -> Option<(i32, i32, i32, i32)> {
    let (cx, cy, cw, ch) = cel.image().drawn_bounds()?;
    let (w, h) = tiles.tile_size();
    let (tw, th) = (w as i32, h as i32);
    let (ox, oy) = cel.position();
    Some((
        i32::from(ox) + cx * tw,
        i32::from(oy) + cy * th,
        cw * tw,
        ch * th,
    ))
}

/// Writes the 16 by 16 canvas window whose corner is canvas position `corner` into `out`, as
/// the map cel `cel` places the tiles of `tiles` there. False, with `out` left alone, when no
/// cell of the window places anything.
pub(crate) fn sample_window(
    cel: &Cel,
    tiles: &Tileset,
    corner: (i32, i32),
    out: &mut RawTile,
) -> bool {
    sample_through(cel, tiles, corner, out, None)
}

/// [`sample_window`], reading the tiles' pixels through `cache` where it holds them.
pub(super) fn sample_through(
    cel: &Cel,
    tiles: &Tileset,
    corner: (i32, i32),
    out: &mut RawTile,
    mut cache: Option<&mut TileCache>,
) -> bool {
    let (w, h) = tiles.tile_size();
    let (tw, th) = (w as i32, h as i32);
    let (ox, oy) = cel.position();
    // The window in the cel's own coordinates, where one cell is one image pixel.
    let (lx, ly) = (corner.0 - i32::from(ox), corner.1 - i32::from(oy));
    let (cx0, cx1) = (lx.div_euclid(tw), (lx + SIDE - 1).div_euclid(tw));
    let (cy0, cy1) = (ly.div_euclid(th), (ly + SIDE - 1).div_euclid(th));
    let mut cells = cel.image().reader();
    let mut drawn = false;
    for cy in cy0..=cy1 {
        let rows = ((cy * th - ly).max(0), ((cy + 1) * th - ly).min(SIDE));
        for cx in cx0..=cx1 {
            let cell = MapCell::from_bits(cells.get(cx, cy));
            let Some(placed) = Placed::of(tiles, cell) else {
                continue;
            };
            if !drawn {
                out.fill(0);
                drawn = true;
            }
            let columns = ((cx * tw - lx).max(0), ((cx + 1) * tw - lx).min(SIDE));
            let from = (cx * tw - lx, cy * th - ly);
            let kept = cache.as_deref_mut().and_then(|c| c.pixels(tiles, &placed));
            match kept {
                Some(px) => placed.copy_kept(out, from, (columns, rows), px),
                None => placed.copy_into(out, from, (columns, rows)),
            }
        }
    }
    drawn
}

impl Placed<'_> {
    /// [`Placed::copy_into`] from the tile's pixels decoded row by row in `px`. Unturned
    /// cells copy whole rows.
    fn copy_kept(
        &self,
        out: &mut RawTile,
        from: (i32, i32),
        (columns, rows): ((i32, i32), (i32, i32)),
        px: &[u32],
    ) {
        let w = self.size.0;
        if self.orientation != Orientation::IDENTITY {
            self.walk_kept(out, from, (columns, rows), px);
            return;
        }
        let len = (columns.1 - columns.0) as usize;
        let sx = (columns.0 - from.0) as usize;
        for row in rows.0..rows.1 {
            let source = (row - from.1) as usize * w + sx;
            let at = (row * SIDE + columns.0) as usize;
            out[at..at + len].copy_from_slice(&px[source..source + len]);
        }
    }

    /// [`Placed::walk`] over decoded pixels: a turned tile is read by stepping one index
    /// through `px`, by a fixed amount along a row and another down a column.
    fn walk_kept(
        &self,
        out: &mut RawTile,
        from: (i32, i32),
        (columns, rows): ((i32, i32), (i32, i32)),
        px: &[u32],
    ) {
        let (w, h) = self.size;
        let inverse = INVERSES[usize::from(self.orientation.index())];
        let index = |x: usize, y: usize| {
            let (sx, sy) = inverse.apply(x, y, w, h);
            (sy * w + sx) as isize
        };
        let start = index(0, 0);
        let across = if w > 1 { index(1, 0) - start } else { 0 };
        let down = if h > 1 { index(0, 1) - start } else { 0 };
        let first = (columns.0 - from.0) as isize;
        for row in rows.0..rows.1 {
            let dy = (row - from.1) as isize;
            let mut at = start + across * first + down * dy;
            let line =
                &mut out[(row * SIDE + columns.0) as usize..(row * SIDE + columns.1) as usize];
            for pixel in line {
                *pixel = px.get(at as usize).copied().unwrap_or(0);
                at += across;
            }
        }
    }

    /// Copies the part of the cell inside `columns` and `rows` of the window into `out`. `from`
    /// is where the cell's corner lies in the window.
    fn copy_into(
        &self,
        out: &mut RawTile,
        from: (i32, i32),
        (columns, rows): ((i32, i32), (i32, i32)),
    ) {
        let (w, h) = self.size;
        // A tile of at most 16 by 16 is one stored tile, read without the table.
        if w <= TILE && h <= TILE {
            match self.image.tile(0, 0) {
                Some(Tile::Solid(v)) => fill_rect(out, columns, rows, *v),
                Some(tile) => {
                    let read = |sx: i32, sy: i32| tile.get((sy * SIDE + sx) as usize);
                    self.walk(out, from, (columns, rows), read);
                }
                None => {}
            }
        } else {
            let mut reader: Reader<'_> = self.image.reader();
            self.walk(out, from, (columns, rows), |sx, sy| reader.get(sx, sy));
        }
    }

    /// For each window pixel of the part, reads the source pixel the orientation names. The
    /// source position is stepped along rather than worked out again for each pixel.
    #[inline(always)]
    fn walk(
        &self,
        out: &mut RawTile,
        from: (i32, i32),
        (columns, rows): ((i32, i32), (i32, i32)),
        mut read: impl FnMut(i32, i32) -> u32,
    ) {
        let (w, h) = self.size;
        let inverse = INVERSES[usize::from(self.orientation.index())];
        let at = |x: usize, y: usize| {
            let (sx, sy) = inverse.apply(x, y, w, h);
            (sx as i32, sy as i32)
        };
        let (x0, y0) = at(0, 0);
        let step = |(sx, sy): (i32, i32)| (sx - x0, sy - y0);
        let (ax, ay) = if w > 1 { step(at(1, 0)) } else { (0, 0) };
        let (bx, by) = if h > 1 { step(at(0, 1)) } else { (0, 0) };
        let first = columns.0 - from.0;
        for row in rows.0..rows.1 {
            let dy = row - from.1;
            let (mut sx, mut sy) = (x0 + ax * first + bx * dy, y0 + ay * first + by * dy);
            let line =
                &mut out[(row * SIDE + columns.0) as usize..(row * SIDE + columns.1) as usize];
            for pixel in line {
                *pixel = read(sx, sy);
                sx += ax;
                sy += ay;
            }
        }
    }
}

fn fill_rect(out: &mut RawTile, (c0, c1): (i32, i32), (r0, r1): (i32, i32), value: u32) {
    for row in r0..r1 {
        out[(row * SIDE + c0) as usize..(row * SIDE + c1) as usize].fill(value);
    }
}

#[cfg(test)]
#[path = "sample_tests.rs"]
mod tests;
