//! Flipping and rotating a whole drawing about the canvas centre, every pixel kept.
//!
//! Pixels past the canvas edge turn with the rest, so the image is rebuilt and its new offset is
//! chosen to keep every pixel within the image's reach.

use super::cel::{Cel, OFFSET_LIMIT};
use crate::document::LimitError;
use crate::grid::Cell;
use crate::image::{Image, MAX_TILE_REACH};
use crate::tile::{RawTile, Tile, TILE, TILE_AREA, TILE_SHIFT};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A quarter turn or a mirror of the whole canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    FlipHorizontal,
    FlipVertical,
    Clockwise,
    CounterClockwise,
}

const SIDE: i32 = TILE as i32;
/// The local pixel positions an image can hold, on each axis.
const LOWEST: i32 = -MAX_TILE_REACH * SIDE;
const HIGHEST: i32 = MAX_TILE_REACH * SIDE - 1;

impl Turn {
    /// Where a canvas position goes. A rotation of a canvas that is not square pivots on its
    /// centre, rounding toward zero, as rotating a selection does.
    fn map(self, (w, h): (i32, i32), (x, y): (i32, i32)) -> (i32, i32) {
        let (ox, oy) = ((w - h) / 2, (h - w) / 2);
        match self {
            Turn::FlipHorizontal => (w - 1 - x, y),
            Turn::FlipVertical => (x, h - 1 - y),
            Turn::Clockwise => (ox + h - 1 - y, oy + x),
            Turn::CounterClockwise => (ox + y, oy + w - 1 - x),
        }
    }

    /// Where a rectangle of canvas positions goes, as its least and most corners.
    fn map_box(self, size: (i32, i32), lo: (i32, i32), hi: (i32, i32)) -> ((i32, i32), (i32, i32)) {
        let (a, b) = (self.map(size, lo), self.map(size, hi));
        ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)))
    }
}

/// The offset on one axis nearest `wanted`, on a multiple of 16 where one fits, that keeps the
/// pixels from `lo` to `hi` within reach and the offset within range.
fn choose_offset(wanted: i32, lo: i32, hi: i32) -> Option<i32> {
    let least = (hi - HIGHEST).max(-OFFSET_LIMIT);
    let most = (lo - LOWEST).min(OFFSET_LIMIT);
    if least > most {
        return None;
    }
    let aligned = (wanted + SIDE / 2).div_euclid(SIDE) * SIDE;
    let target = aligned.clamp(least, most);
    if target % SIDE == 0 {
        return Some(target);
    }
    let down = most.div_euclid(SIDE) * SIDE;
    let up = (least + SIDE - 1).div_euclid(SIDE) * SIDE;
    let near = [down, up]
        .into_iter()
        .filter(|v| (least..=most).contains(v))
        .min_by_key(|v| (v - wanted).abs());
    Some(near.unwrap_or(target))
}

/// Gathers turned pixels into tiles of the new image.
struct Builder {
    origin: (i32, i32),
    tiles: BTreeMap<(i32, i32), Box<RawTile>>,
}

impl Builder {
    fn put(&mut self, (x, y): (i32, i32), value: u32) {
        let (lx, ly) = (x - self.origin.0, y - self.origin.1);
        let key = (ly >> TILE_SHIFT, lx >> TILE_SHIFT);
        let raw = self
            .tiles
            .entry(key)
            .or_insert_with(|| Box::new([0; TILE_AREA]));
        raw[((ly & (SIDE - 1)) * SIDE + (lx & (SIDE - 1))) as usize] = value;
    }

    fn finish(self) -> Option<Image> {
        let Some((&(top, _), _)) = self.tiles.first_key_value() else {
            return Some(Image::blank());
        };
        let bottom = self.tiles.keys().map(|k| k.0).max().unwrap_or(top);
        let left = self.tiles.keys().map(|k| k.1).min().unwrap_or(0);
        let right = self.tiles.keys().map(|k| k.1).max().unwrap_or(0);
        let mut cells: Vec<Cell> = Vec::new();
        for ty in top..=bottom {
            for tx in left..=right {
                cells.push(
                    self.tiles
                        .get(&(ty, tx))
                        .and_then(|r| Tile::pack(r))
                        .map(Arc::new),
                );
            }
        }
        let wide = u16::try_from(right - left + 1).ok()?;
        Image::from_tiles(
            i16::try_from(left).ok()?,
            i16::try_from(top).ok()?,
            wide,
            cells,
        )
    }
}

impl Cel {
    /// This cel's whole drawing turned about the centre of a `w` by `h` canvas, at the offset
    /// nearest its own that can hold it. `None` if the cel is blank and nothing would change.
    pub(crate) fn turned(&self, turn: Turn, w: usize, h: usize) -> Result<Option<Cel>, LimitError> {
        if self.is_blank() {
            return Ok(None);
        }
        let size = (w as i32, h as i32);
        let (cx, cy) = (self.x as i32, self.y as i32);
        let (x0, y0, tw, th) = self.image.table();
        let lo = (x0 * SIDE + cx, y0 * SIDE + cy);
        let hi = (
            (x0 + tw as i32) * SIDE - 1 + cx,
            (y0 + th as i32) * SIDE - 1 + cy,
        );
        let (new_lo, new_hi) = turn.map_box(size, lo, hi);
        let (Some(nx), Some(ny)) = (
            choose_offset(cx, new_lo.0, new_hi.0),
            choose_offset(cy, new_lo.1, new_hi.1),
        ) else {
            return Err(LimitError::OutOfReach);
        };
        let mut builder = Builder {
            origin: (nx, ny),
            tiles: BTreeMap::new(),
        };
        let mut raw = [0u32; TILE_AREA];
        for ty in y0..y0 + th as i32 {
            for tx in x0..x0 + tw as i32 {
                let Some(tile) = self.image.tile(tx, ty) else {
                    continue;
                };
                tile.decode(&mut raw);
                for (i, &value) in raw.iter().enumerate().filter(|(_, v)| **v != 0) {
                    let (lx, ly) = (
                        tx * SIDE + (i as i32 & (SIDE - 1)),
                        ty * SIDE + (i as i32 >> TILE_SHIFT),
                    );
                    builder.put(turn.map(size, (lx + cx, ly + cy)), value);
                }
            }
        }
        let image = builder.finish().ok_or(LimitError::OutOfReach)?;
        Ok(Cel::at(image, nx, ny).map(|mut cel| {
            cel.link = self.link;
            cel
        }))
    }
}
