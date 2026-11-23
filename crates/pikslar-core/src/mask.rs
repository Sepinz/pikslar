//! A one-bit shape: which cells a selection covers.
//!
//! A mask carries its own origin, so a selection can be dragged partly off the canvas
//! without losing the part that is out of sight.

use crate::editor::for_line;

const WORD_BITS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    stride: usize,
    bits: Vec<u64>,
}

/// One straight piece of a shape's outline, in document cell boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub x: i32,
    pub y: i32,
    pub len: i32,
    pub horizontal: bool,
}

/// An outline as it will be once its shape is stretched from `from` to `to`.
///
/// Scaling samples nearest-neighbor, so a boundary between source cells lands on the first
/// target cell past it. Exact when enlarging. Shrinking can skip thin rows the outline still
/// shows, which the real outline corrects on release, without building the mask every move.
pub fn scale_edges(
    edges: &[Edge],
    from: (i32, i32, i32, i32),
    to: (i32, i32, i32, i32),
) -> Vec<Edge> {
    let along = |v: i32, origin: i32, old: i32, target: i32, new: i32| {
        target
            + ((v - origin) as i64 * new as i64 + old as i64 - 1).div_euclid(old.max(1) as i64)
                as i32
    };
    let map_x = |x: i32| along(x, from.0, from.2, to.0, to.2);
    let map_y = |y: i32| along(y, from.1, from.3, to.1, to.3);
    edges
        .iter()
        .filter_map(|e| {
            let (x, y) = (map_x(e.x), map_y(e.y));
            let len = if e.horizontal {
                map_x(e.x + e.len) - x
            } else {
                map_y(e.y + e.len) - y
            };
            (len > 0).then_some(Edge {
                x,
                y,
                len,
                horizontal: e.horizontal,
            })
        })
        .collect()
}

impl Mask {
    /// A mask with nothing set, covering the given area.
    pub fn blank(x: i32, y: i32, w: i32, h: i32) -> Mask {
        let (w, h) = (w.max(0), h.max(0));
        let stride = (w as usize).div_ceil(WORD_BITS);
        Mask {
            x,
            y,
            w,
            h,
            stride,
            bits: vec![0; stride * h as usize],
        }
    }

    pub fn rect(x: i32, y: i32, w: i32, h: i32) -> Mask {
        let mut mask = Mask::blank(x, y, w, h);
        for row in mask.bits.chunks_exact_mut(mask.stride.max(1)) {
            row.fill(u64::MAX);
            let spare = w as usize % WORD_BITS;
            if spare != 0 {
                if let Some(last) = row.last_mut() {
                    *last = (1u64 << spare) - 1;
                }
            }
        }
        mask
    }

    /// How many words a mask of this size holds. `None` for an impossible size.
    pub fn word_count(w: i32, h: i32) -> Option<usize> {
        let (w, h) = (usize::try_from(w).ok()?, usize::try_from(h).ok()?);
        w.div_ceil(WORD_BITS).checked_mul(h)
    }

    /// The raw bits, row by row.
    pub fn words(&self) -> &[u64] {
        &self.bits
    }

    /// Rebuilds a mask from `words`. `None` if they do not fit the size.
    pub fn from_words(x: i32, y: i32, w: i32, h: i32, mut bits: Vec<u64>) -> Option<Mask> {
        if Mask::word_count(w, h)? != bits.len() {
            return None;
        }
        let stride = (w as usize).div_ceil(WORD_BITS);
        let spare = w as usize % WORD_BITS;
        if spare != 0 {
            // Bits past the width would be visited as cells that do not exist.
            for row in bits.chunks_exact_mut(stride) {
                if let Some(last) = row.last_mut() {
                    *last &= (1u64 << spare) - 1;
                }
            }
        }
        Some(Mask {
            x,
            y,
            w,
            h,
            stride,
            bits,
        })
    }

    /// The area covered: x, y, width and height.
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        (self.x, self.y, self.w, self.h)
    }

    #[inline]
    fn locate(&self, x: i32, y: i32) -> Option<(usize, u64)> {
        let (cx, cy) = (x - self.x, y - self.y);
        if cx < 0 || cy < 0 || cx >= self.w || cy >= self.h {
            return None;
        }
        let cx = cx as usize;
        Some((
            cy as usize * self.stride + cx / WORD_BITS,
            1 << (cx % WORD_BITS),
        ))
    }

    #[inline]
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.locate(x, y)
            .is_some_and(|(word, bit)| self.bits[word] & bit != 0)
    }

    /// Sets one cell. Cells outside the mask's area are ignored.
    #[inline]
    pub fn set(&mut self, x: i32, y: i32) {
        if let Some((word, bit)) = self.locate(x, y) {
            self.bits[word] |= bit;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&word| word == 0)
    }

    pub fn count(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// Bytes held, for accounting.
    pub fn heap_size(&self) -> usize {
        self.bits.len() * 8
    }

    /// What this shape keeps alive while only history holds it: its bits, and the shared
    /// allocation it lives in with its two counts.
    pub(crate) fn kept_bytes(&self) -> usize {
        2 * size_of::<usize>() + size_of::<Mask>() + self.bits.capacity() * size_of::<u64>()
    }

    /// Moves the whole shape.
    pub fn translate(&mut self, dx: i32, dy: i32) {
        self.x += dx;
        self.y += dy;
    }

    /// Calls `visit` with the document position of every set cell.
    pub fn for_each(&self, mut visit: impl FnMut(i32, i32)) {
        if self.stride == 0 {
            return;
        }
        for (row, words) in self.bits.chunks_exact(self.stride).enumerate() {
            let y = self.y + row as i32;
            for (index, &word) in words.iter().enumerate() {
                let mut bits = word;
                while bits != 0 {
                    let cx = index * WORD_BITS + bits.trailing_zeros() as usize;
                    visit(self.x + cx as i32, y);
                    bits &= bits - 1;
                }
            }
        }
    }

    pub fn union(&self, other: &Mask) -> Mask {
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        let right = (self.x + self.w).max(other.x + other.w);
        let bottom = (self.y + self.h).max(other.y + other.h);
        let mut out = Mask::blank(x, y, right - x, bottom - y);
        self.for_each(|px, py| out.set(px, py));
        other.for_each(|px, py| out.set(px, py));
        out
    }

    pub fn subtract(&self, other: &Mask) -> Mask {
        let mut out = Mask::blank(self.x, self.y, self.w, self.h);
        self.for_each(|px, py| {
            if !other.contains(px, py) {
                out.set(px, py);
            }
        });
        out
    }

    /// The part of this shape inside a `w` by `h` canvas.
    pub fn within(&self, w: i32, h: i32) -> Mask {
        let (x0, y0) = (self.x.max(0), self.y.max(0));
        let (x1, y1) = ((self.x + self.w).min(w), (self.y + self.h).min(h));
        let mut out = Mask::blank(x0, y0, x1 - x0, y1 - y0);
        self.for_each(|px, py| out.set(px, py));
        out
    }

    /// Every cell of a `w` by `h` canvas this shape does not cover.
    pub fn inverted(&self, w: i32, h: i32) -> Mask {
        Mask::rect(0, 0, w, h).subtract(self)
    }

    /// The same shape in the smallest area that holds it.
    pub fn trimmed(&self) -> Mask {
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        self.for_each(|x, y| {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        });
        if x0 > x1 {
            return Mask::blank(self.x, self.y, 0, 0);
        }
        let mut out = Mask::blank(x0, y0, x1 - x0 + 1, y1 - y0 + 1);
        self.for_each(|x, y| out.set(x, y));
        out
    }

    /// Fills a closed path through cell centers, always including the path itself.
    pub fn polygon(points: &[(i32, i32)]) -> Mask {
        let Some(&first) = points.first() else {
            return Mask::blank(0, 0, 0, 0);
        };
        let (mut x0, mut y0, mut x1, mut y1) = (first.0, first.1, first.0, first.1);
        for &(x, y) in points {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        let mut out = Mask::blank(x0, y0, x1 - x0 + 1, y1 - y0 + 1);
        let mut crossings: Vec<Vec<f32>> = vec![Vec::new(); out.h as usize];
        let closing = points.iter().copied().skip(1).chain(Some(first));
        for (a, b) in points.iter().copied().zip(closing) {
            for_line(a.0, a.1, b.0, b.1, |x, y| out.set(x, y));
            add_crossings(a, b, y0, &mut crossings);
        }
        for (row, xs) in crossings.iter_mut().enumerate() {
            crate::sorting::sort_floats(xs);
            for pair in xs.chunks_exact(2) {
                let (from, to) = (pair[0].ceil() as i32, pair[1].floor() as i32);
                for x in from..=to {
                    out.set(x, y0 + row as i32);
                }
            }
        }
        out
    }

    /// The boundary between covered and uncovered cells, as merged straight runs.
    pub fn outline(&self) -> Vec<Edge> {
        let mut edges = Vec::new();
        for y in 0..=self.h {
            self.collect_runs(y, true, &mut edges);
        }
        for x in 0..=self.w {
            self.collect_runs(x, false, &mut edges);
        }
        edges
    }

    fn collect_runs(&self, line: i32, horizontal: bool, edges: &mut Vec<Edge>) {
        let (span, ox, oy) = (if horizontal { self.w } else { self.h }, self.x, self.y);
        let is_edge = |at: i32| {
            let (ax, ay, bx, by) = if horizontal {
                (ox + at, oy + line - 1, ox + at, oy + line)
            } else {
                (ox + line - 1, oy + at, ox + line, oy + at)
            };
            self.contains(ax, ay) != self.contains(bx, by)
        };
        let mut at = 0;
        while at < span {
            if !is_edge(at) {
                at += 1;
                continue;
            }
            let start = at;
            while at < span && is_edge(at) {
                at += 1;
            }
            let (x, y) = if horizontal {
                (ox + start, oy + line)
            } else {
                (ox + line, oy + start)
            };
            edges.push(Edge {
                x,
                y,
                len: at - start,
                horizontal,
            });
        }
    }
}

/// Records where a segment crosses each cell-center row, counting each row once.
fn add_crossings(a: (i32, i32), b: (i32, i32), top: i32, crossings: &mut [Vec<f32>]) {
    if a.1 == b.1 {
        return;
    }
    let (upper, lower) = if a.1 < b.1 { (a, b) } else { (b, a) };
    let slope = (lower.0 - upper.0) as f32 / (lower.1 - upper.1) as f32;
    for y in upper.1..lower.1 {
        let x = upper.0 as f32 + (y - upper.1) as f32 * slope;
        crossings[(y - top) as usize].push(x);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_shape_is_charged_for_the_allocation_it_lives_in() {
        let m = Mask::rect(0, 0, 100, 3);
        assert!(m.kept_bytes() >= m.heap_size() + size_of::<Mask>() + 16);
    }

    #[test]
    fn a_stretched_outline_is_the_outline_of_the_stretched_shape() {
        let mut m = Mask::blank(3, 5, 7, 6);
        for (x, y) in [
            (3, 5),
            (4, 5),
            (5, 5),
            (3, 6),
            (5, 6),
            (7, 7),
            (8, 8),
            (9, 10),
            (4, 9),
        ] {
            m.set(x, y);
        }
        let from = (3, 5, 7, 6);
        let key = |e: &Edge| (e.horizontal, e.y, e.x, e.len);
        for to in [
            (10, -4, 7, 6),
            (1, 2, 14, 12),
            (0, 0, 20, 17),
            (-3, 1, 33, 9),
        ] {
            let mut big = Mask::blank(to.0, to.1, to.2, to.3);
            for y in 0..to.3 {
                for x in 0..to.2 {
                    let (sx, sy) = (from.0 + x * from.2 / to.2, from.1 + y * from.3 / to.3);
                    if m.contains(sx, sy) {
                        big.set(to.0 + x, to.1 + y);
                    }
                }
            }
            let (mut got, mut want) = (scale_edges(&m.outline(), from, to), big.outline());
            got.sort_by_key(key);
            want.sort_by_key(key);
            assert_eq!(got, want, "to {to:?}");
        }
    }

    #[test]
    fn a_rect_covers_exactly_its_cells() {
        let m = Mask::rect(2, 3, 70, 2);
        assert!(m.contains(2, 3) && m.contains(71, 4));
        assert!(!m.contains(1, 3) && !m.contains(72, 3) && !m.contains(2, 5));
        assert_eq!(m.count(), 140);
    }

    #[test]
    fn set_ignores_cells_outside_its_area() {
        let mut m = Mask::blank(0, 0, 4, 4);
        m.set(-1, 0);
        m.set(4, 0);
        assert!(m.is_empty());
    }

    #[test]
    fn combining_shapes_gives_the_expected_cells() {
        let a = Mask::rect(0, 0, 4, 4);
        let b = Mask::rect(2, 2, 4, 4);
        assert_eq!(a.union(&b).count(), 28);
        let cut = a.subtract(&b);
        assert_eq!(cut.count(), 12);
        assert!(cut.contains(1, 1) && !cut.contains(2, 2));
    }

    #[test]
    fn inverting_twice_returns_the_shape() {
        let m = Mask::rect(1, 1, 3, 3);
        let twice = m.inverted(8, 8).inverted(8, 8);
        assert_eq!(twice.count(), m.count());
        assert!(twice.contains(1, 1) && !twice.contains(0, 0));
    }

    #[test]
    fn within_drops_only_what_is_off_canvas() {
        let m = Mask::rect(-2, -2, 6, 6);
        let clipped = m.within(3, 3);
        assert_eq!(clipped.bounds(), (0, 0, 3, 3));
        assert_eq!(clipped.count(), 9);
    }

    #[test]
    fn moving_keeps_the_shape() {
        let mut m = Mask::rect(0, 0, 2, 1);
        m.translate(-5, 7);
        assert!(m.contains(-4, 7) && !m.contains(0, 0));
    }

    #[test]
    fn trimming_finds_the_tight_bounds() {
        let mut m = Mask::blank(0, 0, 100, 100);
        m.set(40, 50);
        m.set(42, 51);
        assert_eq!(m.trimmed().bounds(), (40, 50, 3, 2));
        assert!(Mask::blank(0, 0, 9, 9).trimmed().is_empty());
    }

    #[test]
    fn a_polygon_fills_inside_its_path() {
        let m = Mask::polygon(&[(0, 0), (10, 0), (10, 10), (0, 10)]);
        assert_eq!(m.count(), 121);
    }

    #[test]
    fn a_concave_polygon_leaves_its_notch_empty() {
        let m = Mask::polygon(&[(0, 0), (12, 0), (12, 12), (6, 3), (0, 12)]);
        assert!(m.contains(2, 2));
        assert!(!m.contains(6, 10), "the notch is outside");
        assert!(m.contains(6, 3), "the path itself is included");
    }

    #[test]
    fn degenerate_polygons_do_not_panic() {
        assert!(Mask::polygon(&[]).is_empty());
        assert_eq!(Mask::polygon(&[(3, 3)]).count(), 1);
        assert_eq!(Mask::polygon(&[(3, 3), (6, 3)]).count(), 4);
    }

    #[test]
    fn a_rectangle_outline_is_four_edges() {
        let edges = Mask::rect(1, 2, 3, 4).outline();
        assert_eq!(edges.len(), 4);
        assert!(edges.contains(&Edge {
            x: 1,
            y: 2,
            len: 3,
            horizontal: true
        }));
        assert!(edges.contains(&Edge {
            x: 4,
            y: 2,
            len: 4,
            horizontal: false
        }));
    }

    #[test]
    fn a_hole_has_its_own_outline() {
        let ring = Mask::rect(0, 0, 5, 5).subtract(&Mask::rect(2, 2, 1, 1));
        assert_eq!(ring.outline().len(), 8);
    }
}
