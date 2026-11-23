//! Writing canvas tiles into a cel's own tiles, at any offset.
//!
//! A stroke is painted in canvas tiles. At an offset that is a multiple of 16 each canvas tile is
//! one image tile. Otherwise each covers parts of up to four, and each image tile is put together
//! from the parts of the canvas tiles over it, decoded once and packed once per commit.

use super::cel::Cel;
use crate::grid::Cell;
use crate::image::TileKey;
use crate::tile::{RawTile, Tile, TILE, TILE_SHIFT};
use std::collections::BTreeMap;
use std::sync::Arc;

const IN_TILE: i32 = TILE as i32 - 1;

/// An image tile being put together: what it held, and what it will hold.
struct Work {
    before: Box<RawTile>,
    after: Box<RawTile>,
}

/// Collects canvas tiles for one commit and turns them into changes keyed by image tile.
pub struct CanvasWriter<'a> {
    cel: &'a Cel,
    /// The canvas position of the image's local origin, in pixels.
    origin: (i32, i32),
    aligned: bool,
    changes: Vec<(TileKey, Cell)>,
    /// Image tiles still waiting for parts, by row then column.
    work: BTreeMap<(i32, i32), Work>,
    last_row: Option<i32>,
}

fn key(x: i32, y: i32) -> TileKey {
    // A canvas tile always maps within the image's reach, which fits an i16 with room to spare.
    TileKey {
        x: x as i16,
        y: y as i16,
    }
}

impl<'a> CanvasWriter<'a> {
    pub(crate) fn new(cel: &'a Cel) -> Self {
        let origin = (cel.x as i32, cel.y as i32);
        CanvasWriter {
            cel,
            origin,
            aligned: origin.0 & IN_TILE == 0 && origin.1 & IN_TILE == 0,
            changes: Vec::new(),
            work: BTreeMap::new(),
            last_row: None,
        }
    }

    /// The image tile a canvas tile's corner falls in, and how far into it.
    fn local(&self, tx: usize, ty: usize) -> ((i32, i32), (usize, usize)) {
        let lx = tx as i32 * TILE as i32 - self.origin.0;
        let ly = ty as i32 * TILE as i32 - self.origin.1;
        (
            (lx >> TILE_SHIFT, ly >> TILE_SHIFT),
            ((lx & IN_TILE) as usize, (ly & IN_TILE) as usize),
        )
    }

    /// Takes the new contents of a changed canvas tile. At an offset that is not a multiple of
    /// 16, canvas tiles must come row by row, top first, so finished image tiles can be packed
    /// as soon as nothing more can reach them.
    /// True when canvas tiles must be given row by row, top first.
    pub(crate) fn needs_rows(&self) -> bool {
        !self.aligned
    }

    pub(crate) fn put(&mut self, tx: usize, ty: usize, raw: &RawTile) {
        let ((ix, iy), (rx, ry)) = self.local(tx, ty);
        if self.aligned {
            self.changes
                .push((key(ix, iy), Tile::pack(raw).map(Arc::new)));
            return;
        }
        if self.last_row != Some(ty as i32) {
            debug_assert!(self.last_row.is_none_or(|row| row < ty as i32));
            // Canvas row `ty` reaches image rows `iy` and `iy + 1`, so rows above `iy` are done.
            self.flush_rows_before(iy);
            self.last_row = Some(ty as i32);
        }
        // Each part: which image tile, where the piece starts in it, its size, and where it
        // starts in the canvas tile. The inverse of `Cel::decode`.
        let parts = [
            ((0, 0), (rx, ry), (TILE - rx, TILE - ry), (0, 0)),
            ((1, 0), (0, ry), (rx, TILE - ry), (TILE - rx, 0)),
            ((0, 1), (rx, 0), (TILE - rx, ry), (0, TILE - ry)),
            ((1, 1), (0, 0), (rx, ry), (TILE - rx, TILE - ry)),
        ];
        for ((dx, dy), into, (w, h), from) in parts {
            if w == 0 || h == 0 {
                continue;
            }
            let at = (ix + dx, iy + dy);
            let image = self.cel.image();
            let work = self.work.entry((at.1, at.0)).or_insert_with(|| {
                let mut before = Box::new([0u32; crate::tile::TILE_AREA]);
                image.decode(at.0, at.1, &mut before);
                Work {
                    after: before.clone(),
                    before,
                }
            });
            for row in 0..h {
                let source = (from.1 + row) * TILE + from.0;
                let target = (into.1 + row) * TILE + into.0;
                work.after[target..target + w].copy_from_slice(&raw[source..source + w]);
            }
        }
    }

    fn flush_rows_before(&mut self, row: i32) {
        while let Some(entry) = self.work.first_entry() {
            if entry.key().0 >= row {
                break;
            }
            let ((y, x), work) = entry.remove_entry();
            if work.after != work.before {
                self.changes
                    .push((key(x, y), Tile::pack(&work.after).map(Arc::new)));
            }
        }
    }

    /// The changes to swap into the image, each keyed by image tile.
    pub(crate) fn finish(mut self) -> Vec<(TileKey, Cell)> {
        self.flush_rows_before(i32::MAX);
        self.changes
    }
}
