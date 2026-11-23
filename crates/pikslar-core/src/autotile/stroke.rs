//! A terrain stroke in progress: the points painted so far, until it is committed as one step.
//!
//! Pointer positions become cells, or for a Corner4 set painted by its corners the nearest
//! vertices, and are kept as a set of points. Nothing is worked out per event beyond adding
//! points: what the stroke changes is found from the points and the map as it was, by
//! [`super::paint::changes`], when a preview is asked for and at the commit. Only cells whose
//! rectangle meets the canvas, and with a selection whose centre it holds, are changed.

use super::paint::{self, At, Paint, Rule};
use super::rules::RuleKind;
use crate::command::{Corners, TerrainShape, TerrainTool};
use crate::editor::for_line;
use crate::image::Image;
use crate::mask::Mask;
use crate::span_fill::scan_fill;
use crate::tilemap::stroke::{selected, Geometry, Rect};
use crate::tilemap::PINNED;
use std::sync::Arc;

#[cfg(test)]
thread_local! {
    /// How many points `add` was asked to paint on this thread, so a test can show that a drag
    /// far outside the map visits only the window and not every point on the way.
    pub(crate) static ADDS_TRIED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One terrain stroke on a map cel.
pub(crate) struct TerrainStroke {
    pub layer: usize,
    pub frame: usize,
    pub tool: TerrainTool,
    geo: Geometry,
    rule: Rule,
    paint: Paint,
    selection: Option<Arc<Mask>>,
    start: At,
    last: At,
}

impl std::fmt::Debug for TerrainStroke {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerrainStroke")
            .field("tool", &self.tool)
            .field("layer", &self.layer)
            .field("frame", &self.frame)
            .field("points", &self.paint.points.len())
            .finish_non_exhaustive()
    }
}

impl TerrainStroke {
    /// A stroke pressed at a canvas position. A Corner4 set painted by corners takes vertices,
    /// except for a fill, which paints cells.
    pub fn new(
        (layer, frame): (usize, usize),
        tool: TerrainTool,
        (geo, rule): (Geometry, Rule),
        selection: Option<Arc<Mask>>,
        (x, y): (i32, i32),
    ) -> TerrainStroke {
        let vertices = rule.kind() == RuleKind::Corner4
            && tool.corners == Corners::Vertex
            && matches!(tool.shape, TerrainShape::Brush | TerrainShape::Rect);
        let start = if vertices {
            geo.vertex_of(x, y)
        } else {
            geo.cell_of(x, y)
        };
        let paint = Paint {
            clear: tool.clear,
            vertices,
            points: Default::default(),
        };
        TerrainStroke {
            layer,
            frame,
            tool,
            geo,
            rule,
            paint,
            selection,
            start,
            last: start,
        }
    }

    pub fn geometry(&self) -> &Geometry {
        &self.geo
    }

    fn point_of(&self, x: i32, y: i32) -> At {
        if self.paint.vertices {
            self.geo.vertex_of(x, y)
        } else {
            self.geo.cell_of(x, y)
        }
    }

    /// The canvas the cells a point can change cover: its 3 by 3 cells, or the 4 at a vertex.
    fn reach(&self, (x, y): At) -> Rect {
        let (lx, ly, w, h) = self.geo.rect_of((x - 1, y - 1));
        let side = if self.paint.vertices { 2 } else { 3 };
        (lx, ly, w * side, h * side)
    }

    /// Paints a point, if the stroke may reach it, and marks the canvas it can change.
    fn add(&mut self, p: At, mark: &mut impl FnMut(Rect)) {
        #[cfg(test)]
        ADDS_TRIED.with(|n| n.set(n.get() + 1));
        let reachable = if self.paint.vertices {
            self.geo.vertex_in_window(p)
        } else {
            self.geo.in_window(p) && selected(&self.geo, self.selection.as_deref(), p)
        };
        if reachable && self.paint.points.insert(p) {
            mark(self.reach(p));
        }
    }

    /// The press: paints the first point. A fill and a retile are made by their own calls.
    pub fn press(&mut self, mark: &mut impl FnMut(Rect)) {
        if matches!(self.tool.shape, TerrainShape::Brush | TerrainShape::Rect) {
            self.add(self.start, mark);
        }
    }

    /// Takes the pointer to a canvas position. Nothing happens while it stays at one point.
    pub fn drag(&mut self, (x, y): (i32, i32), mark: &mut impl FnMut(Rect)) {
        let to = self.point_of(x, y);
        if to == self.last {
            return;
        }
        match self.tool.shape {
            TerrainShape::Brush => {
                let from = self.last;
                for_line(from.0, from.1, to.0, to.1, |px, py| {
                    self.add((px, py), mark)
                });
            }
            TerrainShape::Rect => {
                self.discard(mark);
                // Only the window can be painted: a pointer far outside costs no more.
                let (left, top, right, bottom) = self.geo.window();
                let extra = i32::from(self.paint.vertices);
                let (x0, x1) = (self.start.0.min(to.0), self.start.0.max(to.0));
                let (y0, y1) = (self.start.1.min(to.1), self.start.1.max(to.1));
                for py in y0.max(top)..=y1.min(bottom + extra) {
                    for px in x0.max(left)..=x1.min(right + extra) {
                        self.add((px, py), mark);
                    }
                }
            }
            TerrainShape::Fill | TerrainShape::Retile => {}
        }
        self.last = to;
    }

    /// Takes back every point, marking the canvas they could change.
    pub fn discard(&mut self, mark: &mut impl FnMut(Rect)) {
        for &p in &self.paint.points {
            mark(self.reach(p));
        }
        self.paint.points.clear();
    }

    /// Paints the 4-connected region of cells holding what the pressed cell holds, pins aside.
    pub fn fill(&mut self, image: &Image, mark: &mut impl FnMut(Rect)) {
        let (geo, start) = (self.geo, self.start);
        let selection = self.selection.clone();
        let selection = selection.as_deref();
        if !geo.in_window(start) || !selected(&geo, selection, start) {
            return;
        }
        let (left, top, right, bottom) = geo.window();
        // A pinned cell is no part of the region: it stays, and stops the flood like a wall.
        let target = image.get(start.0, start.1);
        if target & PINNED != 0 {
            return;
        }
        let open = |px: i32, py: i32| {
            let c = (left + px, top + py);
            selected(&geo, selection, c) && image.get(c.0, c.1) == target
        };
        let size = (right - left + 1, bottom - top + 1);
        let from = (start.0 - left, start.1 - top);
        scan_fill(size, from, open, |px, py| {
            self.add((left + px, top + py), mark)
        });
    }

    /// True when the stroke may change a cell.
    fn may(&self, c: At) -> bool {
        self.geo.in_window(c) && selected(&self.geo, self.selection.as_deref(), c)
    }

    /// Every cell the stroke changes, in row order, with the value it will hold.
    pub fn changes(&self, image: &Image) -> Vec<(At, u32)> {
        if self.tool.shape == TerrainShape::Retile {
            return self.retile(image);
        }
        let map = |(x, y): At| image.get(x, y);
        paint::changes(&self.rule, &self.paint, &map, |c| self.may(c))
    }

    /// The pressed cell re-autotiled, if that changes it.
    fn retile(&self, image: &Image) -> Vec<(At, u32)> {
        let c = self.start;
        let map = |(x, y): At| image.get(x, y);
        let before = image.get(c.0, c.1);
        match paint::retile(&self.rule, &map, c) {
            Some(value) if value != before && self.may(c) => vec![(c, value)],
            _ => Vec::new(),
        }
    }

    /// The canvas the cell a retile presses covers.
    pub fn start_rect(&self) -> Rect {
        self.geo.rect_of(self.start)
    }
}
