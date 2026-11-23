//! What a terrain stroke painted, worked out by the model from the tool, the path the pointer
//! took and the map as it was: the painted points (cells, or for a Corner4 set painted by its
//! corners, vertices), and from them which cells the stroke may change. Written apart from
//! `pikslar-core`'s stroke; only the Bresenham line is shared, as plain tile strokes use it too.
//!
//! A selection is not modelled: it only takes points away, so the footprint is then a superset
//! and the checks built on it that need exactness are skipped by the caller.

use pikslar_core::command::{Corners, TerrainShape, TerrainTool};
use pikslar_core::editor::for_line;
use pikslar_core::tilemap::MapCell;
use pikslar_core::Image;
use std::collections::BTreeSet;

type At = (i32, i32);

/// Where a map's cells lie on the canvas.
#[derive(Clone, Copy, Debug)]
pub(super) struct Grid {
    pub tile: (i32, i32),
    pub offset: (i32, i32),
    /// First and last cell whose rectangle meets the canvas.
    low: At,
    high: At,
}

impl Grid {
    pub fn new(tile: (usize, usize), offset: (i16, i16), canvas: (usize, usize)) -> Grid {
        let tile = (tile.0.max(1) as i32, tile.1.max(1) as i32);
        let offset = (i32::from(offset.0), i32::from(offset.1));
        let low = (
            (-offset.0).div_euclid(tile.0),
            (-offset.1).div_euclid(tile.1),
        );
        let last = |off: i32, t: i32, size: usize| (size as i32 - 1 - off).div_euclid(t);
        let high = (
            last(offset.0, tile.0, canvas.0).max(low.0),
            last(offset.1, tile.1, canvas.1).max(low.1),
        );
        Grid {
            tile,
            offset,
            low,
            high,
        }
    }

    fn cell_of(&self, (x, y): At) -> At {
        (
            (x - self.offset.0).div_euclid(self.tile.0),
            (y - self.offset.1).div_euclid(self.tile.1),
        )
    }

    /// The cell corner nearest a canvas position; (`vx`, `vy`) is the top-left of that cell.
    fn vertex_of(&self, (x, y): At) -> At {
        (
            (x - self.offset.0 + self.tile.0 / 2).div_euclid(self.tile.0),
            (y - self.offset.1 + self.tile.1 / 2).div_euclid(self.tile.1),
        )
    }

    pub fn in_window(&self, c: At) -> bool {
        (self.low.0..=self.high.0).contains(&c.0) && (self.low.1..=self.high.1).contains(&c.1)
    }

    fn vertex_in_window(&self, v: At) -> bool {
        (self.low.0..=self.high.0 + 1).contains(&v.0)
            && (self.low.1..=self.high.1 + 1).contains(&v.1)
    }
}

/// The points a stroke painted.
#[derive(Clone, Debug)]
pub(super) struct Footprint {
    pub clear: bool,
    pub shape: TerrainShape,
    /// The points are vertices, not cells.
    pub vertices: bool,
    pub points: BTreeSet<At>,
    start: At,
    grid: Grid,
}

impl Footprint {
    /// The footprint of a stroke along `path`, on a map as it was before.
    pub fn new(
        tool: &TerrainTool,
        path: &[At],
        grid: &Grid,
        corner4: bool,
        before: &Image,
    ) -> Footprint {
        let by_corner = corner4
            && tool.corners == Corners::Vertex
            && matches!(tool.shape, TerrainShape::Brush | TerrainShape::Rect);
        let point = |p: At| {
            if by_corner {
                grid.vertex_of(p)
            } else {
                grid.cell_of(p)
            }
        };
        let points: Vec<At> = path.iter().map(|&p| point(p)).collect();
        let start = points.first().copied().unwrap_or((0, 0));
        let mut out = Footprint {
            clear: tool.clear,
            shape: tool.shape,
            vertices: by_corner,
            points: BTreeSet::new(),
            start,
            grid: *grid,
        };
        let reachable = |p: At| {
            if by_corner {
                grid.vertex_in_window(p)
            } else {
                grid.in_window(p)
            }
        };
        match tool.shape {
            TerrainShape::Brush => out.brush(&points, reachable),
            TerrainShape::Rect => out.rect(&points, reachable),
            TerrainShape::Fill => out.fill(grid, before),
            TerrainShape::Retile => out.retile(grid),
        }
        out
    }

    fn brush(&mut self, points: &[At], reachable: impl Fn(At) -> bool) {
        let mut last = self.start;
        let mut hit = Vec::new();
        hit.push(last);
        for &to in points {
            for_line(last.0, last.1, to.0, to.1, |x, y| hit.push((x, y)));
            last = to;
        }
        self.points
            .extend(hit.into_iter().filter(|&p| reachable(p)));
    }

    fn rect(&mut self, points: &[At], reachable: impl Fn(At) -> bool) {
        let end = points.last().copied().unwrap_or(self.start);
        let (x0, x1) = (self.start.0.min(end.0), self.start.0.max(end.0));
        let (y0, y1) = (self.start.1.min(end.1), self.start.1.max(end.1));
        // Only points of the window count, so only they are visited (a vertex set has one more
        // row and column, as in core).
        let extra = i32::from(self.vertices);
        let (low, high) = (self.grid.low, self.grid.high);
        for y in y0.max(low.1)..=y1.min(high.1 + extra) {
            for x in x0.max(low.0)..=x1.min(high.0 + extra) {
                if reachable((x, y)) {
                    self.points.insert((x, y));
                }
            }
        }
    }

    /// The 4-connected region of cells equal to the pressed one; a pinned cell is not part of it.
    fn fill(&mut self, grid: &Grid, before: &Image) {
        let target = before.get(self.start.0, self.start.1);
        if !grid.in_window(self.start) || MapCell::from_bits(target).is_pinned() {
            return;
        }
        let mut open = vec![self.start];
        while let Some(c) = open.pop() {
            if !grid.in_window(c) || before.get(c.0, c.1) != target || !self.points.insert(c) {
                continue;
            }
            open.extend([
                (c.0 + 1, c.1),
                (c.0 - 1, c.1),
                (c.0, c.1 + 1),
                (c.0, c.1 - 1),
            ]);
        }
    }

    fn retile(&mut self, grid: &Grid) {
        if grid.in_window(self.start) {
            self.points.insert(self.start);
        }
    }

    /// True when the stroke painted the cell itself (never so by corners).
    pub fn painted(&self, c: At) -> bool {
        !self.vertices && self.points.contains(&c)
    }

    /// True when the stroke set this vertex, directly or through a cell sharing it.
    pub fn vertex_painted(&self, (vx, vy): At) -> bool {
        if self.vertices {
            return self.points.contains(&(vx, vy));
        }
        [(vx - 1, vy - 1), (vx, vy - 1), (vx - 1, vy), (vx, vy)]
            .iter()
            .any(|c| self.points.contains(c))
    }

    /// True when the stroke is of a shape that updates the neighbours of what it painted.
    pub fn updates_neighbours(&self) -> bool {
        self.shape != TerrainShape::Retile
    }

    /// True when the cell is one the stroke may change: a painted cell, a neighbour of one, or
    /// a cell at a painted vertex. A retile changes only the pressed cell.
    pub fn reaches(&self, (x, y): At) -> bool {
        if !self.grid.in_window((x, y)) {
            return false;
        }
        if !self.updates_neighbours() {
            return self.points.contains(&(x, y));
        }
        let (low, high) = if self.vertices { (0, 1) } else { (-1, 1) };
        (low..=high).any(|dy| (low..=high).any(|dx| self.points.contains(&(x + dx, y + dy))))
    }

    /// Every cell of the window the stroke may change, in row order.
    pub fn reach(&self) -> Vec<At> {
        let (low, high) = match (self.updates_neighbours(), self.vertices) {
            (false, _) => (0, 0),
            (true, true) => (-1, 0),
            (true, false) => (-1, 1),
        };
        let mut cells = BTreeSet::new();
        for &(x, y) in &self.points {
            for dy in low..=high {
                for dx in low..=high {
                    cells.insert((y + dy, x + dx));
                }
            }
        }
        let inside = cells.into_iter().map(|(y, x)| (x, y));
        inside.filter(|&c| self.grid.in_window(c)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::tileset::TilesetId;

    fn tool(shape: TerrainShape, corners: Corners) -> TerrainTool {
        TerrainTool {
            tileset: TilesetId::new(1),
            set: 1,
            shape,
            clear: false,
            corners,
        }
    }

    fn grid() -> Grid {
        // 4 pixel tiles on a 16 pixel canvas: cells 0 to 3.
        Grid::new((4, 4), (0, 0), (16, 16))
    }

    #[test]
    fn a_rectangle_keeps_only_the_window_and_a_brush_follows_its_line() {
        let blank = Image::blank();
        let far = [(1, 1), (4_000, 4_000)];
        let rect = Footprint::new(
            &tool(TerrainShape::Rect, Corners::Cell),
            &far,
            &grid(),
            false,
            &blank,
        );
        assert_eq!(rect.points.len(), 16);
        let line = [(1, 1), (13, 1)];
        let brush = Footprint::new(
            &tool(TerrainShape::Brush, Corners::Cell),
            &line,
            &grid(),
            false,
            &blank,
        );
        assert_eq!(
            brush.points.iter().copied().collect::<Vec<_>>(),
            [(0, 0), (1, 0), (2, 0), (3, 0)]
        );
        assert!(brush.reaches((1, 1)) && !brush.reaches((1, 2)));
    }

    #[test]
    fn corners_reach_the_four_cells_at_a_vertex() {
        let blank = Image::blank();
        let by_corner = Footprint::new(
            &tool(TerrainShape::Brush, Corners::Vertex),
            &[(8, 8)],
            &grid(),
            true,
            &blank,
        );
        assert_eq!(
            by_corner.points.iter().copied().collect::<Vec<_>>(),
            [(2, 2)]
        );
        assert_eq!(by_corner.reach(), [(1, 1), (2, 1), (1, 2), (2, 2)]);
        assert!(by_corner.vertex_painted((2, 2)) && !by_corner.painted((2, 2)));
    }
}
