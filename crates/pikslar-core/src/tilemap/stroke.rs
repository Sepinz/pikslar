//! A map stroke in progress: cells placed on an overlay in cell space, until the stroke is
//! committed as one step.
//!
//! Cell (`cx`, `cy`) is the pixel (`cx`, `cy`) of the map cel's image, and its canvas rectangle
//! starts at the cel's offset plus (`cx * tile_w`, `cy * tile_h`). Only cells whose rectangle
//! meets the canvas can be placed, so a map is edited where it can be seen and a stroke can
//! never reach past what an image holds.
//!
//! The overlay is the one pixel strokes use, made for the window of cells the canvas shows and
//! shifted by a multiple of 16 so that each of its tiles is exactly one tile of the image.
//! Placing a cell therefore costs the overlay's pool and nothing else.

use super::stamp::Prepared;
use crate::command::TileTool;
use crate::editor::for_line;
use crate::grid::Cell;
use crate::image::{Image, TileKey};
use crate::mask::Mask;
use crate::overlay::Overlay;
use crate::span_fill::scan_fill;
use crate::tile::{RawTile, Tile, TILE, TILE_AREA};
use std::sync::Arc;

/// A canvas rectangle: left, top, width and height.
pub(crate) type Rect = (i32, i32, i32, i32);

/// Where the cells are on the canvas, and which of them a stroke may place.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Geometry {
    tile: (i32, i32),
    offset: (i32, i32),
    /// The first and last cell whose rectangle meets the canvas.
    low: (i32, i32),
    high: (i32, i32),
    /// The cell the overlay's corner stands for: `low` rounded down to a multiple of 16.
    base: (i32, i32),
}

impl Geometry {
    /// The geometry of a map cel at `offset` for tiles of `tile` pixels on a canvas.
    pub fn new(tile: (usize, usize), offset: (i16, i16), canvas: (usize, usize)) -> Geometry {
        let tile = (tile.0.max(1) as i32, tile.1.max(1) as i32);
        let offset = (i32::from(offset.0), i32::from(offset.1));
        let first = |off: i32, t: i32| (0 - off).div_euclid(t);
        let last = |off: i32, t: i32, size: usize| (size as i32 - 1 - off).div_euclid(t);
        let low = (first(offset.0, tile.0), first(offset.1, tile.1));
        let high = (
            last(offset.0, tile.0, canvas.0).max(low.0),
            last(offset.1, tile.1, canvas.1).max(low.1),
        );
        let round = |v: i32| v.div_euclid(TILE as i32) * TILE as i32;
        Geometry {
            tile,
            offset,
            low,
            high,
            base: (round(low.0), round(low.1)),
        }
    }

    /// The cell a canvas position is in.
    #[inline]
    pub fn cell_of(&self, x: i32, y: i32) -> (i32, i32) {
        (
            (x - self.offset.0).div_euclid(self.tile.0),
            (y - self.offset.1).div_euclid(self.tile.1),
        )
    }

    /// The cell corner nearest a canvas position. Vertex (`vx`, `vy`) is the top-left corner of
    /// cell (`vx`, `vy`).
    pub fn vertex_of(&self, x: i32, y: i32) -> (i32, i32) {
        (
            (x - self.offset.0 + self.tile.0 / 2).div_euclid(self.tile.0),
            (y - self.offset.1 + self.tile.1 / 2).div_euclid(self.tile.1),
        )
    }

    /// True for a corner of a cell of the window.
    pub fn vertex_in_window(&self, v: (i32, i32)) -> bool {
        (self.low.0..=self.high.0 + 1).contains(&v.0)
            && (self.low.1..=self.high.1 + 1).contains(&v.1)
    }

    #[inline]
    pub fn in_window(&self, c: (i32, i32)) -> bool {
        (self.low.0..=self.high.0).contains(&c.0) && (self.low.1..=self.high.1).contains(&c.1)
    }

    /// The canvas rectangle of a cell.
    #[inline]
    pub fn rect_of(&self, c: (i32, i32)) -> Rect {
        (
            self.offset.0 + c.0 * self.tile.0,
            self.offset.1 + c.1 * self.tile.1,
            self.tile.0,
            self.tile.1,
        )
    }

    /// The canvas pixel at a cell's centre, which decides whether a selection holds the cell.
    #[inline]
    pub fn centre(&self, c: (i32, i32)) -> (i32, i32) {
        let (x, y, w, h) = self.rect_of(c);
        (x + w / 2, y + h / 2)
    }

    /// The window of cells as left, top, right and bottom, all included.
    pub fn window(&self) -> (i32, i32, i32, i32) {
        (self.low.0, self.low.1, self.high.0, self.high.1)
    }

    /// The size of the overlay a stroke places on.
    pub fn overlay_size(&self) -> (usize, usize) {
        (
            (self.high.0 - self.base.0 + 1) as usize,
            (self.high.1 - self.base.1 + 1) as usize,
        )
    }

    /// How many cells the window holds.
    pub fn window_cells(&self) -> usize {
        let (w, h) = self.overlay_size();
        w.saturating_mul(h)
    }

    /// The canvas rectangle of an image tile of cells.
    pub fn rect_of_image_tile(&self, ix: i32, iy: i32) -> Rect {
        let side = TILE as i32;
        (
            self.offset.0 + ix * side * self.tile.0,
            self.offset.1 + iy * side * self.tile.1,
            side * self.tile.0,
            side * self.tile.1,
        )
    }
}

/// Whether a selection, if there is one, holds the cell's centre.
#[inline]
pub(crate) fn selected(geo: &Geometry, selection: Option<&Mask>, c: (i32, i32)) -> bool {
    selection.is_none_or(|s| {
        let (x, y) = geo.centre(c);
        s.contains(x, y)
    })
}

/// One map stroke: what is placed so far, and what places more.
pub(crate) struct MapStroke {
    pub layer: usize,
    pub frame: usize,
    tool: TileTool,
    geo: Geometry,
    cells: Overlay,
    stamp: Prepared,
    start: (i32, i32),
    last: (i32, i32),
}

impl std::fmt::Debug for MapStroke {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MapStroke")
            .field("tool", &self.tool)
            .field("layer", &self.layer)
            .field("frame", &self.frame)
            .finish_non_exhaustive()
    }
}

impl MapStroke {
    pub fn new(
        (layer, frame): (usize, usize),
        tool: TileTool,
        geo: Geometry,
        stamp: Prepared,
        (start, cells): ((i32, i32), Overlay),
    ) -> MapStroke {
        MapStroke {
            layer,
            frame,
            tool,
            geo,
            cells,
            stamp,
            start,
            last: start,
        }
    }

    /// The stroke's overlay, to be emptied and used again.
    pub fn into_overlay(self) -> Overlay {
        self.cells
    }

    pub fn geometry(&self) -> &Geometry {
        &self.geo
    }

    /// Places one value in a cell, if the stroke may reach it.
    #[inline]
    fn put(
        &mut self,
        c: (i32, i32),
        value: u32,
        selection: Option<&Mask>,
        mark: &mut impl FnMut(Rect),
    ) {
        if !self.geo.in_window(c) || !selected(&self.geo, selection, c) {
            return;
        }
        self.cells
            .put(c.0 - self.geo.base.0, c.1 - self.geo.base.1, value);
        mark(self.geo.rect_of(c));
    }

    /// Places the block, or clears a cell, as the tool does, centred on a cell by the rule the
    /// round brush uses.
    fn place_at(&mut self, c: (i32, i32), selection: Option<&Mask>, mark: &mut impl FnMut(Rect)) {
        if self.tool == TileTool::Erase {
            self.put(c, 0, selection, mark);
            return;
        }
        let (w, h) = (self.stamp.width, self.stamp.height);
        let (left, top) = (c.0 - (w - 1) / 2, c.1 - (h - 1) / 2);
        for dy in 0..h {
            for dx in 0..w {
                let value = self.stamp.at(dx, dy);
                if value != 0 {
                    self.put((left + dx, top + dy), value, selection, mark);
                }
            }
        }
    }

    /// The press: places at the first cell. A fill is made by [`MapStroke::fill`] instead.
    pub fn press(&mut self, selection: Option<&Mask>, mark: &mut impl FnMut(Rect)) {
        if matches!(self.tool, TileTool::Place | TileTool::Erase) {
            self.place_at(self.start, selection, mark);
        }
    }

    /// Takes the pointer to a canvas position. Nothing happens while it stays in one cell.
    pub fn drag(
        &mut self,
        (x, y): (i32, i32),
        selection: Option<&Mask>,
        mark: &mut impl FnMut(Rect),
    ) {
        let to = self.geo.cell_of(x, y);
        if to == self.last {
            return;
        }
        match self.tool {
            TileTool::Place | TileTool::Erase => {
                for_line(self.last.0, self.last.1, to.0, to.1, |px, py| {
                    self.place_at((px, py), selection, mark);
                });
            }
            TileTool::Line => {
                self.reset(mark);
                self.line(self.start, to, selection, mark);
            }
            TileTool::Rect => {
                self.reset(mark);
                self.rectangle(self.start, to, selection, mark);
            }
            // Terrain strokes are not map strokes of cells; they never reach here.
            TileTool::Fill | TileTool::Pick | TileTool::Terrain(_) => {}
        }
        self.last = to;
    }

    fn line(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        selection: Option<&Mask>,
        mark: &mut impl FnMut(Rect),
    ) {
        for_line(a.0, a.1, b.0, b.1, |x, y| {
            self.place_at((x, y), selection, mark);
        });
    }

    fn rectangle(
        &mut self,
        a: (i32, i32),
        b: (i32, i32),
        selection: Option<&Mask>,
        mark: &mut impl FnMut(Rect),
    ) {
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        self.line((x0, y0), (x1, y0), selection, mark);
        self.line((x0, y1), (x1, y1), selection, mark);
        self.line((x0, y0), (x0, y1), selection, mark);
        self.line((x1, y0), (x1, y1), selection, mark);
    }

    /// Takes back everything placed, for a shape that is drawn again from its start. The canvas
    /// under it is marked so it shows what is there again.
    fn reset(&mut self, mark: &mut impl FnMut(Rect)) {
        let side = TILE as i32;
        let wide = self.cells.tiles_wide();
        for &index in self.cells.touched() {
            let (tx, ty) = (
                (index as usize % wide) as i32,
                (index as usize / wide) as i32,
            );
            let (ix, iy) = (tx + self.geo.base.0 / side, ty + self.geo.base.1 / side);
            mark(self.geo.rect_of_image_tile(ix, iy));
        }
        self.cells.clear();
    }

    /// Discards what was placed, marking the canvas it covered.
    pub fn discard(&mut self, mark: &mut impl FnMut(Rect)) {
        self.reset(mark);
    }

    /// Fills the 4-connected region of cells equal to the one under the press with the block,
    /// repeated as a pattern from the press. Only cells the stroke may reach are filled.
    pub fn fill(&mut self, image: &Image, selection: Option<&Mask>, mark: &mut impl FnMut(Rect)) {
        let geo = self.geo;
        let start = self.start;
        if !geo.in_window(start) || !selected(&geo, selection, start) {
            return;
        }
        let (left, top, right, bottom) = geo.window();
        let size = (right - left + 1, bottom - top + 1);
        let target = image.get(start.0, start.1);
        let open = |px: i32, py: i32| {
            let c = (left + px, top + py);
            selected(&geo, selection, c) && image.get(c.0, c.1) == target
        };
        let (w, h) = (self.stamp.width, self.stamp.height);
        let (ox, oy) = ((w - 1) / 2, (h - 1) / 2);
        let visit = |px: i32, py: i32| {
            let c = (left + px, top + py);
            let at = (
                (c.0 - start.0 + ox).rem_euclid(w),
                (c.1 - start.1 + oy).rem_euclid(h),
            );
            match self.stamp.at(at.0, at.1) {
                0 => {}
                value => self.put(c, value, selection, mark),
            }
        };
        scan_fill(size, (start.0 - left, start.1 - top), open, visit);
    }

    /// Clears every cell that holds a tile and whose centre the selection holds.
    pub fn clear_selected(&mut self, image: &Image, selection: &Mask, mark: &mut impl FnMut(Rect)) {
        let (x, y, w, h) = selection.bounds();
        let a = self.geo.cell_of(x, y);
        let b = self
            .geo
            .cell_of(x.saturating_add(w) - 1, y.saturating_add(h) - 1);
        let (left, top, right, bottom) = self.geo.window();
        for cy in a.1.max(top)..=b.1.min(bottom) {
            for cx in a.0.max(left)..=b.0.min(right) {
                if image.get(cx, cy) != 0 {
                    self.put((cx, cy), 0, Some(selection), mark);
                }
            }
        }
    }

    /// Visits each cell the stroke has placed so far, with the value placed there: 0 where it
    /// clears the cell.
    pub fn for_each_placed(&self, mut visit: impl FnMut((i32, i32), u32)) {
        let side = TILE as i32;
        let wide = self.cells.tiles_wide();
        for &index in self.cells.touched() {
            let Some(over) = self.cells.tile(index as usize) else {
                continue;
            };
            let left = (index as usize % wide) as i32 * side + self.geo.base.0;
            let top = (index as usize / wide) as i32 * side + self.geo.base.1;
            for i in 0..TILE_AREA {
                if let Some(value) = over.placed(i) {
                    let (dx, dy) = ((i % TILE) as i32, (i / TILE) as i32);
                    visit((left + dx, top + dy), value);
                }
            }
        }
    }

    /// What the stroke changes in the image, as tiles of the image with what they will hold,
    /// leaving out every tile the stroke leaves as it is.
    pub fn changes(&self, image: &Image) -> Vec<(TileKey, Cell)> {
        let side = TILE as i32;
        let wide = self.cells.tiles_wide();
        let mut changes = Vec::new();
        let mut raw: RawTile = [0; TILE_AREA];
        for &index in self.cells.touched() {
            let Some(over) = self.cells.tile(index as usize) else {
                continue;
            };
            let tx = (index as usize % wide) as i32 + self.geo.base.0 / side;
            let ty = (index as usize / wide) as i32 + self.geo.base.1 / side;
            image.decode(tx, ty, &mut raw);
            let before = raw;
            over.apply(&mut raw);
            if raw != before {
                let key = TileKey {
                    x: tx as i16,
                    y: ty as i16,
                };
                changes.push((key, Tile::pack(&raw).map(Arc::new)));
            }
        }
        changes
    }
}
