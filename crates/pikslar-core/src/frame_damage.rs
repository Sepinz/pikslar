//! Which canvas tiles look different in one frame than in another: what a frame step needs to
//! draw again, instead of the whole canvas.
//!
//! The answer may name a tile that looks the same (it errs towards drawing), never the other
//! way. A layer whose two cels show one image at one place adds nothing, which covers holds and
//! anything else shared. Otherwise a paint layer at one place in both frames adds the stored
//! tiles that differ, and a tilemap layer the cells that differ, each found by comparing the
//! shared stored tiles of the two images first and looking inside only those that are not one
//! and the same. A layer that moved between the frames adds everything either cel draws.
//!
//! Only cels differ from frame to frame. Tilesets, the palette, visibility, blending and
//! opacity are the same for every frame, so they never make a frame step draw anything.

use crate::document::Document;
use crate::layer::{Cel, LayerKind};
use crate::tile::{Tile, TILE, TILE_SHIFT};
use crate::tilemap::map_bounds;
use std::ops::ControlFlow;

/// What `mark` says after each tile: go on, or stop looking.
type Flow = ControlFlow<()>;

/// The canvas, in pixels and in 16 by 16 canvas tiles.
struct Canvas {
    size: (i32, i32),
    tiles_wide: usize,
}

impl Canvas {
    /// Calls `mark` with each canvas tile the rectangle touches inside the canvas.
    fn mark(
        &self,
        (x, y, w, h): (i32, i32, i32, i32),
        mark: &mut impl FnMut(usize) -> Flow,
    ) -> Flow {
        let (x0, y0) = (x.max(0), y.max(0));
        let (x1, y1) = ((x + w).min(self.size.0), (y + h).min(self.size.1));
        if x0 >= x1 || y0 >= y1 {
            return Flow::Continue(());
        }
        let side = TILE as i32;
        for ty in y0 / side..=(y1 - 1) / side {
            for tx in x0 / side..=(x1 - 1) / side {
                mark(ty as usize * self.tiles_wide + tx as usize)?;
            }
        }
        Flow::Continue(())
    }
}

/// True when two stored tiles, either of which may be missing, hold different cells.
fn differ(a: Option<&Tile>, b: Option<&Tile>) -> bool {
    match (a, b) {
        (None, None) => false,
        (Some(a), Some(b)) => !std::ptr::eq(a, b) && a != b,
        _ => true,
    }
}

/// Every stored tile position either cel's table covers, with both tiles there.
fn each_stored_pair<'a>(
    a: &'a Cel,
    b: &'a Cel,
    mut f: impl FnMut(i32, i32, Option<&'a Tile>, Option<&'a Tile>) -> Flow,
) -> Flow {
    let (ax, ay, aw, ah) = a.image().table();
    let (bx, by, bw, bh) = b.image().table();
    let (x0, y0) = (ax.min(bx), ay.min(by));
    let x1 = (ax + aw as i32).max(bx + bw as i32);
    let y1 = (ay + ah as i32).max(by + bh as i32);
    for ty in y0..y1 {
        for tx in x0..x1 {
            f(tx, ty, a.image().tile(tx, ty), b.image().tile(tx, ty))?;
        }
    }
    Flow::Continue(())
}

/// A paint or reference layer.
fn paint_change(a: &Cel, b: &Cel, canvas: &Canvas, mark: &mut impl FnMut(usize) -> Flow) -> Flow {
    if a.position() != b.position() {
        for cel in [a, b] {
            if let Some(bounds) = cel.drawn_bounds() {
                canvas.mark(bounds, mark)?;
            }
        }
        return Flow::Continue(());
    }
    let (px, py) = (i32::from(a.position().0), i32::from(a.position().1));
    let side = TILE as i32;
    each_stored_pair(a, b, |tx, ty, ta, tb| {
        if differ(ta, tb) {
            canvas.mark((px + tx * side, py + ty * side, side, side), mark)?;
        }
        Flow::Continue(())
    })
}

/// A tilemap layer whose tiles are `size` pixels.
fn map_change(
    (a, b): (&Cel, &Cel),
    tiles: &crate::tileset::Tileset,
    canvas: &Canvas,
    mark: &mut impl FnMut(usize) -> Flow,
) -> Flow {
    if a.position() != b.position() {
        for cel in [a, b] {
            if let Some(bounds) = map_bounds(cel, tiles) {
                canvas.mark(bounds, mark)?;
            }
        }
        return Flow::Continue(());
    }
    let (px, py) = (i32::from(a.position().0), i32::from(a.position().1));
    let (w, h) = tiles.tile_size();
    let (w, h) = (w as i32, h as i32);
    each_stored_pair(a, b, |tx, ty, ta, tb| {
        if !differ(ta, tb) {
            return Flow::Continue(());
        }
        for i in 0..TILE * TILE {
            let read = |t: Option<&Tile>| t.map_or(0, |t| t.get(i));
            if read(ta) != read(tb) {
                let cx = (tx << TILE_SHIFT) + (i % TILE) as i32;
                let cy = (ty << TILE_SHIFT) + (i / TILE) as i32;
                canvas.mark((px + cx * w, py + cy * h, w, h), mark)?;
            }
        }
        Flow::Continue(())
    })
}

impl Document {
    /// The canvas pixels layer `index`'s cel in `frame` may draw, as x, y, width and height,
    /// or `None` when it draws nothing: a blank cel, a map whose tileset is missing, or a
    /// layer that does not exist.
    pub fn cel_area(&self, index: usize, frame: usize) -> Option<(i32, i32, i32, i32)> {
        let layer = self.layers().get(index)?;
        let cel = layer.cel(frame);
        match layer.kind {
            LayerKind::Tilemap(set) => map_bounds(cel, self.tilesets.get(set)?),
            _ => cel.drawn_bounds(),
        }
    }

    /// Calls `mark` with the number of each canvas tile (16 by 16, row by row) where a layer
    /// that `drawn` takes, by position from the bottom, may look different in frame `to` than
    /// in frame `from`. A tile may be named more than once. Looking stops early when `mark`
    /// says to break. See the module notes.
    pub fn frame_change_tiles(
        &self,
        (from, to): (usize, usize),
        drawn: &[bool],
        mut mark: impl FnMut(usize) -> ControlFlow<()>,
    ) {
        let canvas = Canvas {
            size: (self.width as i32, self.height as i32),
            tiles_wide: self.width.div_ceil(TILE),
        };
        for (index, layer) in self.layers().iter().enumerate() {
            if !drawn.get(index).copied().unwrap_or(false) {
                continue;
            }
            let (a, b) = (layer.cel(from), layer.cel(to));
            if a.looks_the_same(b) {
                continue;
            }
            let flow = match layer.kind {
                // A map whose tileset is missing draws nothing in either frame.
                LayerKind::Tilemap(set) => match self.tilesets.get(set) {
                    Some(tiles) => map_change((a, b), tiles, &canvas, &mut mark),
                    None => Flow::Continue(()),
                },
                _ => paint_change(a, b, &canvas, &mut mark),
            };
            if flow.is_break() {
                return;
            }
        }
    }
}

#[cfg(test)]
#[path = "frame_damage_tests.rs"]
mod tests;
