//! What a tile edit changes on the canvas: only where a shown map places an edited tile.
//!
//! Editing a tile's pixels, or undoing or redoing that, changes every placement of the tile and
//! nothing else. The editor reports it as the tiles that were edited, and whoever draws the
//! canvas finds the canvas tiles they reach with [`Document::placed_canvas_tiles`], in the frames
//! it shows, which is more than the current frame while the onion skin shows its neighbours.

use crate::document::Document;
use crate::image::Image;
use crate::tile::{RawTile, Tile, TILE, TILE_AREA};
use crate::tileset::{TileId, TilesetId, MAX_TILE_ID};

/// The tiles of one tileset whose pixels an edit changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileEdit {
    tileset: TilesetId,
    /// Sorted, each once.
    tiles: Vec<TileId>,
    /// Canvas tiles drawn again besides the placements, such as where a stroke on a map was
    /// shown before it was written into its tiles. Sorted, each once.
    canvas: Vec<u32>,
}

impl TileEdit {
    /// An edit of these tiles of `tileset`.
    pub fn new(tileset: TilesetId, tiles: &[TileId]) -> Self {
        let mut tiles = tiles.to_vec();
        TileId::sort(&mut tiles);
        tiles.dedup();
        TileEdit {
            tileset,
            tiles,
            canvas: Vec::new(),
        }
    }

    /// The same edit, drawing these canvas tiles again too, numbered along rows from the top
    /// left as [`Document::placed_canvas_tiles`] names them.
    pub fn with_canvas_tiles(mut self, tiles: &[u32]) -> Self {
        self.canvas.extend_from_slice(tiles);
        self.canvas.sort_unstable();
        self.canvas.dedup();
        self
    }

    /// The canvas tiles drawn again besides those placing an edited tile.
    pub fn canvas_tiles(&self) -> &[u32] {
        &self.canvas
    }

    pub fn tileset(&self) -> TilesetId {
        self.tileset
    }

    pub fn tiles(&self) -> &[TileId] {
        &self.tiles
    }

    /// Adds `other`'s tiles to this edit, if they are of one tileset.
    pub fn join(&mut self, other: &TileEdit) -> bool {
        if self.tileset != other.tileset {
            return false;
        }
        self.tiles.extend_from_slice(&other.tiles);
        TileId::sort(&mut self.tiles);
        self.tiles.dedup();
        if !other.canvas.is_empty() {
            self.canvas.extend_from_slice(&other.canvas);
            self.canvas.sort_unstable();
            self.canvas.dedup();
        }
        true
    }

    /// True if a cell value places one of the edited tiles, turned any way.
    #[inline]
    fn places(&self, bits: u32) -> bool {
        let id = bits & MAX_TILE_ID;
        match self.tiles.as_slice() {
            [] => false,
            [only] => only.get() == id,
            many => many.binary_search_by_key(&id, |t| t.get()).is_ok(),
        }
    }

    /// True if a stored tile of cell values may hold a cell that places an edited tile.
    fn may_hold(&self, tile: &Tile) -> bool {
        match tile {
            Tile::Solid(v) => self.places(*v),
            Tile::Packed { palette, .. } => palette.iter().any(|&v| self.places(v)),
            Tile::Raw(_) => true,
        }
    }

    /// Calls `found` with the position of every cell of the map `image` that places an edited
    /// tile, one stored tile at a time and skipping those that cannot hold one.
    fn for_cells_in(&self, image: &Image, mut found: impl FnMut(i64, i64)) {
        let (x0, y0, wide, high) = image.table();
        let places =
            (y0..y0 + high as i32).flat_map(|ty| (x0..x0 + wide as i32).map(move |tx| (tx, ty)));
        let mut raw: RawTile = [0; TILE_AREA];
        for (tx, ty) in places {
            let Some(stored) = image.tile(tx, ty).filter(|t| self.may_hold(t)) else {
                continue;
            };
            stored.decode(&mut raw);
            let hits = raw
                .iter()
                .enumerate()
                .filter(|(_, &bits)| self.places(bits));
            for (i, _) in hits {
                let cx = i64::from(tx) * TILE as i64 + (i % TILE) as i64;
                let cy = i64::from(ty) * TILE as i64 + (i / TILE) as i64;
                found(cx, cy);
            }
        }
    }
}

impl Document {
    /// Calls `mark` with the number of every canvas tile, counted along rows from the top left,
    /// that a cell placing an edited tile covers in `frame`, on each layer `drawn` takes by
    /// position from the bottom, after the edit's own [`TileEdit::canvas_tiles`]. A canvas tile
    /// may be named more than once.
    pub fn placed_canvas_tiles(
        &self,
        edit: &TileEdit,
        frame: usize,
        drawn: &[bool],
        mut mark: impl FnMut(usize),
    ) {
        let across = self.width.div_ceil(TILE);
        let count = across * self.height.div_ceil(TILE);
        for &tile in edit.canvas.iter().filter(|&&t| (t as usize) < count) {
            mark(tile as usize);
        }
        let Ok(tiles) = self.tileset(edit.tileset) else {
            return;
        };
        let (w, h) = tiles.tile_size();
        let size = (w as i64, h as i64);
        let canvas = (self.width as i64, self.height as i64);
        let layers = self.layers().iter().enumerate();
        let shown = |i: usize| drawn.get(i) == Some(&true);
        for (_, layer) in layers.filter(|(i, l)| l.tileset() == Some(edit.tileset) && shown(*i)) {
            let cel = layer.cel(frame);
            let (ox, oy) = cel.position();
            let corner = (i64::from(ox), i64::from(oy));
            edit.for_cells_in(cel.image(), |cx, cy| {
                let at = (corner.0 + cx * size.0, corner.1 + cy * size.1);
                canvas_tiles(at, size, canvas, across, &mut mark);
            });
        }
    }
}

/// Names the canvas tiles the rectangle of `size` at `at` touches, within `canvas`.
fn canvas_tiles(
    at: (i64, i64),
    size: (i64, i64),
    canvas: (i64, i64),
    across: usize,
    mark: &mut impl FnMut(usize),
) {
    let (left, top) = (at.0.max(0), at.1.max(0));
    let (right, bottom) = ((at.0 + size.0).min(canvas.0), (at.1 + size.1).min(canvas.1));
    if left >= right || top >= bottom {
        return;
    }
    let side = TILE as i64;
    for ty in top / side..=(bottom - 1) / side {
        for tx in left / side..=(right - 1) / side {
            mark(ty as usize * across + tx as usize);
        }
    }
}

#[cfg(test)]
#[path = "damage_tests.rs"]
mod tests;
