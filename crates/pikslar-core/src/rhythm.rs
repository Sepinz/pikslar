//! Line Rhythm: resolving a shaky freehand path into clean step patterns as it is drawn.
//!
//! The path is reduced to its essential corners, then each span is redrawn as a straight
//! line, which gives even runs. The tolerance is just over one pixel, so the result stays
//! where the artist's hand went: only wobble smaller than a pixel or so is removed.
//!
//! Older parts of the stroke are frozen, so what is already drawn never changes under
//! the pointer. Only the last two spans can still adjust.

use crate::editor::for_line;
use std::collections::HashSet;

/// How far, in pixels, the path may be simplified away from the hand.
const TOLERANCE: f32 = 1.3;
/// Spans stay revisable until this many corners exist in the unfrozen part.
const FREEZE_AT: usize = 4;

type Point = (i32, i32);

fn distance_to_chord(p: Point, a: Point, b: Point) -> f32 {
    let (dx, dy) = ((b.0 - a.0) as f32, (b.1 - a.1) as f32);
    let (px, py) = ((p.0 - a.0) as f32, (p.1 - a.1) as f32);
    let length = (dx * dx + dy * dy).sqrt();
    if length == 0.0 {
        return (px * px + py * py).sqrt();
    }
    (px * dy - py * dx).abs() / length
}

/// Returns the indices of the points that define the path's shape.
fn corners(path: &[Point]) -> Vec<usize> {
    if path.len() < 2 {
        return (0..path.len()).collect();
    }
    let mut keep = vec![false; path.len()];
    keep[0] = true;
    keep[path.len() - 1] = true;
    let mut spans = vec![(0, path.len() - 1)];
    while let Some((from, to)) = spans.pop() {
        let farthest = (from + 1..to)
            .map(|i| (i, distance_to_chord(path[i], path[from], path[to])))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, distance)) = farthest.filter(|&(_, d)| d > TOLERANCE) {
            debug_assert!(distance > 0.0);
            keep[i] = true;
            spans.push((from, i));
            spans.push((i, to));
        }
    }
    (0..path.len()).filter(|&i| keep[i]).collect()
}

/// Draws straight spans through chosen points, without L-shaped joints.
fn rasterize(points: impl Iterator<Item = Point>) -> Vec<Point> {
    let mut cells: Vec<Point> = Vec::new();
    let mut previous: Option<Point> = None;
    for point in points {
        let start = previous.unwrap_or(point);
        for_line(start.0, start.1, point.0, point.1, |x, y| {
            if cells.last() != Some(&(x, y)) {
                cells.push((x, y));
            }
            let n = cells.len();
            if n >= 3 && is_elbow(cells[n - 3], cells[n - 2], cells[n - 1]) {
                cells.remove(n - 2);
            }
        });
        previous = Some(point);
    }
    cells
}

fn is_elbow(a: Point, b: Point, c: Point) -> bool {
    let touching = |p: Point, q: Point| (p.0 - q.0).abs() + (p.1 - q.1).abs() == 1;
    (a.0 - c.0).abs() == 1 && (a.1 - c.1).abs() == 1 && touching(a, b) && touching(b, c)
}

/// Resolves a whole path at once.
pub fn resolve(path: &[Point]) -> Vec<Point> {
    rasterize(corners(path).into_iter().map(|i| path[i]))
}

/// The state of one stroke being resolved live.
#[derive(Debug)]
pub struct Trail {
    raw: Vec<Point>,
    /// Where the unfrozen part of `raw` begins.
    anchor: usize,
    frozen: HashSet<Point>,
    tail: Vec<Point>,
    settled: Vec<Point>,
}

impl Trail {
    pub fn new(start: Point) -> Self {
        Trail {
            raw: vec![start],
            anchor: 0,
            frozen: HashSet::new(),
            tail: vec![start],
            settled: Vec::new(),
        }
    }

    /// Records the hand's movement to a new point.
    pub fn extend_to(&mut self, x: i32, y: i32) {
        let last = self.raw.last().copied().unwrap_or((x, y));
        for_line(last.0, last.1, x, y, |px, py| {
            if self.raw.last() != Some(&(px, py)) {
                self.raw.push((px, py));
            }
        });
    }

    /// True if a cell belongs to a part of the stroke that can no longer change.
    pub fn is_frozen(&self, cell: Point) -> bool {
        self.frozen.contains(&cell)
    }

    /// Recomputes the live end of the stroke, returning the cells it now covers.
    pub fn update(&mut self) -> &[Point] {
        let open = &self.raw[self.anchor..];
        let mut kept = corners(open);
        self.settled.clear();
        if kept.len() >= FREEZE_AT {
            let settled = kept.len() - 3;
            let cells = rasterize(kept[..=settled].iter().map(|&i| open[i]));
            self.frozen.extend(cells.iter().copied());
            self.settled = cells;
            let offset = kept[settled];
            self.anchor += offset;
            kept.drain(..settled);
            kept.iter_mut().for_each(|i| *i -= offset);
        }
        let open = &self.raw[self.anchor..];
        self.tail = rasterize(kept.iter().map(|&i| open[i]));
        &self.tail
    }

    /// Cells that became permanent during the last update.
    pub fn settled(&self) -> &[Point] {
        &self.settled
    }

    pub fn tail(&self) -> &[Point] {
        &self.tail
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lint::{lint, LintKind};

    /// A deterministic stand-in for a hand: an ideal curve plus small wandering error.
    fn shaky(length: i32, curve: impl Fn(f32) -> f32, seed: u32) -> Vec<Point> {
        let mut state = seed;
        let mut random = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 16) as f32 / 65_536.0
        };
        let mut drift = 0.0f32;
        let mut path = Vec::new();
        let mut last: Option<Point> = None;
        for x in 0..length {
            drift = (drift * 0.7 + (random() - 0.5) * 1.4).clamp(-0.9, 0.9);
            let point = (x, (curve(x as f32) + drift).round() as i32);
            let start = last.unwrap_or(point);
            for_line(start.0, start.1, point.0, point.1, |px, py| {
                if path.last() != Some(&(px, py)) {
                    path.push((px, py));
                }
            });
            last = Some(point);
        }
        path
    }

    fn jag_count(cells: &[Point]) -> usize {
        const W: usize = 240;
        const H: usize = 240;
        let mut grid = vec![0u32; W * H];
        for &(x, y) in cells {
            grid[y as usize * W + x as usize] = 7;
        }
        lint(&grid, W, H, |c| c)
            .iter()
            .filter(|f| matches!(f.kind, LintKind::Jag | LintKind::Double))
            .count()
    }

    fn distance_to_path(p: Point, path: &[Point]) -> f32 {
        path.iter()
            .map(|&q| (((p.0 - q.0).pow(2) + (p.1 - q.1).pow(2)) as f32).sqrt())
            .fold(f32::MAX, f32::min)
    }

    fn curves() -> Vec<Box<dyn Fn(f32) -> f32>> {
        vec![
            Box::new(|x| 120.0 + 0.37 * x * 0.5),
            Box::new(|x| 120.0 + 50.0 * (x / 30.0).sin()),
            Box::new(|x| 200.0 - 0.003 * (x - 100.0) * (x - 100.0)),
        ]
    }

    #[test]
    fn a_shaky_hand_comes_out_with_far_fewer_breaks() {
        let (mut before, mut after) = (0, 0);
        for (index, curve) in curves().iter().enumerate() {
            for seed in 0..8 {
                let raw = shaky(220, curve, seed + index as u32 * 100);
                before += jag_count(&raw);
                after += jag_count(&resolve(&raw));
            }
        }
        assert!(
            after * 3 <= before,
            "expected at least 3x fewer breaks: {before} before, {after} after"
        );
    }

    #[test]
    fn the_result_stays_close_to_the_hand() {
        for (index, curve) in curves().iter().enumerate() {
            let raw = shaky(220, curve, 11 + index as u32);
            let worst = resolve(&raw)
                .iter()
                .map(|&p| distance_to_path(p, &raw))
                .fold(0.0, f32::max);
            assert!(worst <= 1.6, "drifted {worst} pixels from the hand");
        }
    }

    #[test]
    fn deliberate_corners_survive() {
        let mut path: Vec<Point> = (0..20).map(|x| (x, 0)).collect();
        path.extend((1..20).map(|y| (19, y)));
        let resolved = resolve(&path);
        assert!(resolved.contains(&(19, 19)) && resolved.contains(&(0, 0)));
        assert!(
            resolved.iter().any(|&(x, y)| x >= 18 && y <= 1),
            "the corner must not be cut off"
        );
    }

    #[test]
    fn already_clean_lines_are_left_alone() {
        let mut clean = Vec::new();
        for_line(0, 0, 40, 10, |x, y| clean.push((x, y)));
        assert_eq!(resolve(&clean), clean);
    }

    #[test]
    fn tiny_paths_do_not_panic() {
        assert_eq!(resolve(&[]), Vec::<Point>::new());
        assert_eq!(resolve(&[(3, 3)]), vec![(3, 3)]);
    }

    #[test]
    fn drawn_cells_settle_and_stop_changing() {
        let raw = shaky(200, |x: f32| 100.0 + 40.0 * (x / 25.0).sin(), 3);
        let mut trail = Trail::new(raw[0]);
        let mut settled: Vec<Point> = Vec::new();
        for &(x, y) in &raw[1..] {
            trail.extend_to(x, y);
            trail.update();
            for &cell in &settled {
                assert!(trail.is_frozen(cell), "a frozen cell was forgotten");
            }
            settled = trail.frozen.iter().copied().collect();
        }
        assert!(
            settled.len() > raw.len() / 2,
            "the stroke must freeze as it goes"
        );
    }

    #[test]
    fn a_live_stroke_ends_at_the_pointer() {
        let mut trail = Trail::new((0, 0));
        trail.extend_to(30, 7);
        assert_eq!(trail.update().last(), Some(&(30, 7)));
    }
}
