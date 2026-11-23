//! Pixel Lint: finding the small mistakes that are easy to miss at normal zoom.
//!
//! Every rule is deliberately conservative. A linter that nags about intentional choices
//! gets switched off, so a rule only fires on patterns that are rarely on purpose.

use crate::layer::{Cel, Layer, LayerKind};
use std::collections::{HashMap, HashSet};

/// How far past each canvas edge [`lint_cel`] looks, so art that continues there is judged as
/// it would be on a larger canvas.
pub const LINT_MARGIN: usize = 8;

/// Stop after this many, since a screen full of warnings helps nobody.
const MAX_FINDINGS: usize = 2_000;
/// Palette analysis beyond this many colors means the art is shaded by blending anyway.
const MAX_COLORS_COMPARED: usize = 32;
/// Colors this close in every channel are hard to tell apart.
const NEAR_COLOR_DISTANCE: i32 = 3;
/// A shade used this many times less than its near twin looks like a slip of the picker.
const ACCIDENTAL_COLOR_RATIO: usize = 10;
/// Specks closer together than this are a spray or a sparkle, not a mistake.
const SPECK_CROWD_RADIUS: i32 = 8;
/// A line has few cells of its own color nearby. More than this is a patch of texture.
const MOST_CELLS_NEAR_A_LINE: usize = 9;
/// A line divides at most two flat areas. More colors around it means shading or texture.
const MOST_COLORS_BESIDE_A_LINE: usize = 2;
/// A slip of the picker touches only a handful of cells.
const MOST_CELLS_IN_A_SLIP: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LintKind {
    /// A lone pixel with nothing touching it.
    Stray,
    /// A pixel that makes a line two pixels thick at a step.
    Double,
    /// A run of a stepped line that breaks the rhythm of its neighbors.
    Jag,
    /// Two colors in use that are almost the same.
    NearColors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Finding {
    pub kind: LintKind,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Where the finding begins before it is cut to the canvas, so it can be known again after
    /// its cel moves. The same as `x` and `y` unless it began past the edge.
    pub from: (i32, i32),
}

/// A read-only view of a cel, optionally with its axes swapped.
#[derive(Clone, Copy)]
struct Cells<'a> {
    cells: &'a [u32],
    w: usize,
    h: usize,
    swapped: bool,
}

impl Cells<'_> {
    fn across(&self) -> usize {
        if self.swapped {
            self.h
        } else {
            self.w
        }
    }

    fn down(&self) -> usize {
        if self.swapped {
            self.w
        } else {
            self.h
        }
    }

    #[inline]
    fn get(&self, a: i32, b: i32) -> u32 {
        let (x, y) = if self.swapped { (b, a) } else { (a, b) };
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return 0;
        }
        self.cells[y as usize * self.w + x as usize]
    }
}

#[derive(Clone, Copy)]
struct Run {
    start: i32,
    end: i32,
    value: u32,
}

impl Run {
    fn len(&self) -> i32 {
        self.end - self.start + 1
    }
}

/// The single-pixel-thick runs in one line of the image.
fn thin_runs(cells: Cells<'_>, line: i32, out: &mut Vec<Run>) {
    out.clear();
    let across = cells.across() as i32;
    let mut a = 0;
    while a < across {
        let value = cells.get(a, line);
        if value == 0 {
            a += 1;
            continue;
        }
        let start = a;
        let mut thin = true;
        while a < across && cells.get(a, line) == value {
            thin &= cells.get(a, line - 1) != value && cells.get(a, line + 1) != value;
            a += 1;
        }
        if thin {
            out.push(Run {
                start,
                end: a - 1,
                value,
            });
        }
    }
}

/// True if a run breaks from two neighbors that agree with each other, by enough to show.
/// Neighbors that disagree are a curve changing direction, which is what curves do.
fn breaks_rhythm(previous: i32, length: i32, next: i32) -> bool {
    let (low, high) = (previous.min(next), previous.max(next));
    if high - low > (low / 2).max(1) {
        return false;
    }
    let shorter = low - length >= (length / 2).max(2);
    let longer = length - high >= (high / 2).max(2);
    shorter || longer
}

/// A stepped line still being followed, ending at `reach` on the next row.
#[derive(Clone, Copy)]
struct Chain {
    reach: i32,
    value: u32,
    runs: [Run; 3],
    len: usize,
}

impl Chain {
    fn started_by(run: Run) -> Chain {
        Chain {
            reach: 0,
            value: run.value,
            runs: [run; 3],
            len: 1,
        }
    }

    fn then(mut self, run: Run) -> Chain {
        if self.len == self.runs.len() {
            self.runs.rotate_left(1);
            self.runs[self.len - 1] = run;
        } else {
            self.runs[self.len] = run;
            self.len += 1;
        }
        self
    }
}

/// The chain that the run at `key` continues. Both are sorted by position.
fn find_chain(open: &[Chain], key: i32, value: u32) -> Option<Chain> {
    let first = open.partition_point(|c| c.reach < key);
    open[first..]
        .iter()
        .take_while(|c| c.reach == key)
        .find(|c| c.value == value)
        .copied()
}

fn continues(following: &[Run], run: &Run, rightward: bool) -> bool {
    let next = if rightward {
        let first = following.partition_point(|n| n.start < run.end + 1);
        following.get(first).filter(|n| n.start == run.end + 1)
    } else {
        let first = following.partition_point(|n| n.end < run.start - 1);
        following.get(first).filter(|n| n.end == run.start - 1)
    };
    next.is_some_and(|n| n.value == run.value)
}

/// Follows stair-stepped lines down the image in one direction.
fn scan_stairs(cells: Cells<'_>, rightward: bool, findings: &mut Vec<Finding>) {
    let down = cells.down() as i32;
    let mut current: Vec<Run> = Vec::new();
    let mut following: Vec<Run> = Vec::new();
    let mut open: Vec<Chain> = Vec::new();
    let mut next_open: Vec<Chain> = Vec::new();
    thin_runs(cells, 0, &mut current);
    for line in 0..down {
        thin_runs(cells, line + 1, &mut following);
        next_open.clear();
        for run in &current {
            let key = if rightward { run.start } else { run.end };
            let chain = match find_chain(&open, key, run.value) {
                Some(chain) => chain.then(*run),
                None => Chain::started_by(*run),
            };
            check_tail(&chain.runs[..chain.len], line, cells.swapped, findings);
            if continues(&following, run, rightward) {
                let reach = if rightward {
                    run.end + 1
                } else {
                    run.start - 1
                };
                next_open.push(Chain { reach, ..chain });
            }
        }
        std::mem::swap(&mut open, &mut next_open);
        std::mem::swap(&mut current, &mut following);
    }
}

/// Checks the middle of the last three runs in a chain, once the third has arrived.
fn check_tail(chain: &[Run], line: i32, swapped: bool, findings: &mut Vec<Finding>) {
    let [.., before, middle, after] = chain else {
        return;
    };
    if !breaks_rhythm(before.len(), middle.len(), after.len()) {
        return;
    }
    let (start, end) = (
        before.start.min(middle.start).min(after.start),
        before.end.max(middle.end).max(after.end),
    );
    let (a, b, across, down) = (start, line - 2, end - start + 1, 3);
    let (x, y, w, h) = if swapped {
        (b, a, down, across)
    } else {
        (a, b, across, down)
    };
    findings.push(Finding {
        kind: LintKind::Jag,
        x,
        y,
        w,
        h,
        from: (x, y),
    });
}

/// The neighbors present around a cell, as same-color flags in reading order.
fn is_stray(cells: Cells<'_>, x: i32, y: i32) -> bool {
    (-1..=1).all(|dy| (-1..=1).all(|dx| (dx == 0 && dy == 0) || cells.get(x + dx, y + dy) == 0))
}

/// A corner pixel of a staircase: removing it leaves a clean diagonal.
fn is_double(cells: Cells<'_>, x: i32, y: i32, value: u32) -> bool {
    let same = |dx: i32, dy: i32| cells.get(x + dx, y + dy) == value;
    let across = [(1, 0), (-1, 0)];
    let upright = [(0, 1), (0, -1)];
    if across.iter().filter(|d| same(d.0, d.1)).count() != 1
        || upright.iter().filter(|d| same(d.0, d.1)).count() != 1
    {
        return false;
    }
    let horizontal = across.iter().copied().find(|d| same(d.0, d.1));
    let vertical = upright.iter().copied().find(|d| same(d.0, d.1));
    let (Some((hx, _)), Some((_, vy))) = (horizontal, vertical) else {
        return false;
    };
    let filled_corner = same(hx, vy);
    let straight_arm = same(hx * 2, 0) || same(0, vy * 2);
    // Each arm must carry on diagonally, or this is just a small L-shaped mark.
    let continues_h = same(hx * 2, -vy) || same(hx, -vy);
    let continues_v = same(-hx, vy * 2) || same(-hx, vy);
    !filled_corner
        && !straight_arm
        && continues_h
        && continues_v
        && is_on_a_line(cells, x, y, value)
}

/// Shading and noise make the same corner shapes, but with far more of the color around.
fn is_on_a_line(cells: Cells<'_>, x: i32, y: i32, value: u32) -> bool {
    let mut nearby = 0;
    let mut others = [value; MOST_COLORS_BESIDE_A_LINE];
    let mut other_count = 0;
    for dy in -2..=2 {
        for dx in -2..=2 {
            let cell = cells.get(x + dx, y + dy);
            if cell == value {
                nearby += 1;
            } else if !others[..other_count].contains(&cell) {
                if other_count == others.len() {
                    return false;
                }
                others[other_count] = cell;
                other_count += 1;
            }
        }
    }
    nearby <= MOST_CELLS_NEAR_A_LINE
}

/// Keeps only specks with no other speck nearby. `specks` is in reading order.
fn lone_specks(specks: &[Finding]) -> Vec<Finding> {
    let crowded = |index: usize| {
        let me = specks[index];
        let near = |other: &Finding| {
            (other.x - me.x).abs() <= SPECK_CROWD_RADIUS
                && (other.y - me.y).abs() <= SPECK_CROWD_RADIUS
        };
        let in_reach = |other: &&Finding| (other.y - me.y).abs() <= SPECK_CROWD_RADIUS;
        specks[index + 1..].iter().take_while(in_reach).any(near)
            || specks[..index].iter().rev().take_while(in_reach).any(near)
    };
    (0..specks.len())
        .filter(|&index| !crowded(index))
        .map(|index| specks[index])
        .collect()
}

fn scan_cells(cells: Cells<'_>, findings: &mut Vec<Finding>) {
    let mut specks = Vec::new();
    let at = |kind, x, y| Finding {
        kind,
        x,
        y,
        w: 1,
        h: 1,
        from: (x, y),
    };
    for y in 0..cells.h as i32 {
        for x in 0..cells.w as i32 {
            let value = cells.get(x, y);
            if value == 0 {
                continue;
            }
            if is_stray(cells, x, y) {
                specks.push(at(LintKind::Stray, x, y));
            } else if is_double(cells, x, y, value) {
                findings.push(at(LintKind::Double, x, y));
            }
        }
    }
    findings.extend(lone_specks(&specks));
}

fn is_near(a: u32, b: u32) -> bool {
    let (a, b) = (a.to_le_bytes(), b.to_le_bytes());
    a.iter()
        .zip(&b)
        .all(|(&p, &q)| (p as i32 - q as i32).abs() <= NEAR_COLOR_DISTANCE)
}

/// Counts how many cells show each color, and where each first appears.
fn color_usage(cells: &[u32], color: &impl Fn(u32) -> u32) -> HashMap<u32, (usize, usize)> {
    let mut used: HashMap<u32, (usize, usize)> = HashMap::new();
    let mut start = 0;
    while start < cells.len() {
        let value = cells[start];
        let length = cells[start..].iter().take_while(|&&c| c == value).count();
        if value != 0 {
            used.entry(color(value)).or_insert((start, 0)).1 += length;
        }
        start += length;
    }
    used
}

/// Reports a color used rarely when a near twin is used far more, at its first use.
/// Two shades that are both well used are a choice, and shading is full of them.
fn scan_colors(cells: &[u32], w: usize, color: &impl Fn(u32) -> u32, findings: &mut Vec<Finding>) {
    let used = color_usage(cells, color);
    if used.len() > MAX_COLORS_COMPARED {
        return;
    }
    // In color order: the colors are distinct, so sorting them alone, with the shared sort of
    // `u32`, orders the entries as sorting the entries would.
    let mut keys: Vec<u32> = used.keys().copied().collect();
    keys.sort_unstable();
    let entry = |c: &u32| used.get(c).map(|&(i, n)| (*c, i, n));
    let colors: Vec<(u32, usize, usize)> = keys.iter().filter_map(entry).collect();
    let mut reported = HashSet::new();
    for (position, a) in colors.iter().enumerate() {
        for b in &colors[position + 1..] {
            let (rare, common) = if a.2 <= b.2 { (a, b) } else { (b, a) };
            if is_near(a.0, b.0)
                && rare.2 <= MOST_CELLS_IN_A_SLIP
                && rare.2 * ACCIDENTAL_COLOR_RATIO <= common.2
                && reported.insert(rare.0)
            {
                let (x, y) = ((rare.1 % w) as i32, (rare.1 / w) as i32);
                findings.push(Finding {
                    kind: LintKind::NearColors,
                    x,
                    y,
                    w: 1,
                    h: 1,
                    from: (x, y),
                });
            }
        }
    }
}

/// Checks one cel. `color` turns a cell value into the color it shows.
pub fn lint(cells: &[u32], w: usize, h: usize, color: impl Fn(u32) -> u32) -> Vec<Finding> {
    let mut findings = lint_all(cells, w, h, color);
    findings.truncate(MAX_FINDINGS);
    findings
}

/// Checks the part of a cel on a `w` by `h` canvas, looking [`LINT_MARGIN`] pixels past each
/// edge through the cel's offset. Only findings that reach the canvas are kept, cut to it.
pub fn lint_cel(cel: &Cel, w: usize, h: usize, color: impl Fn(u32) -> u32) -> Vec<Finding> {
    let margin = LINT_MARGIN as i32;
    let (wide, tall) = (w + 2 * LINT_MARGIN, h + 2 * LINT_MARGIN);
    let cells = cel.window(-margin, -margin, wide, tall);
    let mut found: Vec<Finding> = lint_all(&cells, wide, tall, color)
        .into_iter()
        .filter_map(|f| cut_to_canvas(f, margin, w as i32, h as i32))
        .collect();
    found.truncate(MAX_FINDINGS);
    found
}

/// Checks the layer's cel of `frame` as [`lint_cel`] does, for the layers that hold pixels to
/// correct. A reference is a picture to draw from and a group has no pixels. A tilemap layer
/// holds cells, not pixels, so it is skipped as well.
pub fn lint_layer(
    layer: &Layer,
    frame: usize,
    (w, h): (usize, usize),
    color: impl Fn(u32) -> u32,
) -> Vec<Finding> {
    match layer.kind {
        LayerKind::Paint => lint_cel(layer.cel(frame), w, h, color),
        LayerKind::Reference | LayerKind::Group | LayerKind::Tilemap(_) => Vec::new(),
    }
}

/// A finding from a scan that began `margin` pixels before the canvas, in canvas positions and
/// cut to it, or `None` if it lies wholly past the edge.
fn cut_to_canvas(f: Finding, margin: i32, w: i32, h: i32) -> Option<Finding> {
    let (x0, y0) = ((f.x - margin).max(0), (f.y - margin).max(0));
    let (x1, y1) = ((f.x - margin + f.w).min(w), (f.y - margin + f.h).min(h));
    (x0 < x1 && y0 < y1).then_some(Finding {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
        from: (f.from.0 - margin, f.from.1 - margin),
        ..f
    })
}

/// Every finding, with no limit. Lets evaluation see what a capped list would hide.
pub fn lint_all(cells: &[u32], w: usize, h: usize, color: impl Fn(u32) -> u32) -> Vec<Finding> {
    let mut findings = Vec::new();
    let view = Cells {
        cells,
        w,
        h,
        swapped: false,
    };
    scan_cells(view, &mut findings);
    for swapped in [false, true] {
        for rightward in [true, false] {
            scan_stairs(Cells { swapped, ..view }, rightward, &mut findings);
        }
    }
    scan_colors(cells, w, &color, &mut findings);
    findings
}

#[cfg(test)]
#[path = "lint_margin_tests.rs"]
mod margin_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const INK: u32 = 7;

    fn run_lint(points: &[(usize, usize)]) -> Vec<Finding> {
        const W: usize = 48;
        let mut grid = vec![0; W * W];
        for &(x, y) in points {
            grid[y * W + x] = INK;
        }
        lint(&grid, W, W, |c| c)
    }

    fn kinds(findings: &[Finding], kind: LintKind) -> usize {
        findings.iter().filter(|f| f.kind == kind).count()
    }

    /// A stair going right and down, with the given horizontal run lengths.
    fn stairs(runs: &[usize]) -> Vec<(usize, usize)> {
        let (mut x, mut points) = (2, Vec::new());
        for (y, &length) in runs.iter().enumerate() {
            for _ in 0..length {
                points.push((x, y + 2));
                x += 1;
            }
        }
        points
    }

    #[test]
    fn a_lone_pixel_is_a_stray() {
        let f = run_lint(&[(10, 10)]);
        assert_eq!(kinds(&f, LintKind::Stray), 1);
        assert_eq!((f[0].x, f[0].y), (10, 10));
    }

    #[test]
    fn touching_pixels_are_not_strays() {
        assert_eq!(kinds(&run_lint(&[(10, 10), (11, 11)]), LintKind::Stray), 0);
    }

    #[test]
    fn a_clean_stair_has_no_jags() {
        for runs in [
            [2usize, 2, 2, 2, 2, 2].as_slice(),
            &[1, 1, 1, 1, 1],
            &[5, 3, 2, 2, 1, 1],
            &[1, 2, 1, 2, 1, 2],
            &[3, 2, 3, 2, 3],
        ] {
            assert_eq!(
                kinds(&run_lint(&stairs(runs)), LintKind::Jag),
                0,
                "{runs:?}"
            );
        }
    }

    #[test]
    fn a_run_out_of_step_is_a_jag() {
        for runs in [
            [3usize, 3, 1, 3, 3].as_slice(),
            &[1, 1, 3, 1, 1],
            &[4, 2, 4],
        ] {
            assert_eq!(
                kinds(&run_lint(&stairs(runs)), LintKind::Jag),
                1,
                "{runs:?}"
            );
        }
    }

    #[test]
    fn jags_are_found_in_steep_lines_too() {
        let swapped: Vec<_> = stairs(&[3, 3, 1, 3, 3])
            .iter()
            .map(|&(x, y)| (y, x))
            .collect();
        assert_eq!(kinds(&run_lint(&swapped), LintKind::Jag), 1);
    }

    #[test]
    fn jags_are_found_going_either_way() {
        let mirrored: Vec<_> = stairs(&[3, 3, 1, 3, 3])
            .iter()
            .map(|&(x, y)| (40 - x, y))
            .collect();
        assert_eq!(kinds(&run_lint(&mirrored), LintKind::Jag), 1);
    }

    #[test]
    fn long_runs_tolerate_small_changes() {
        assert_eq!(kinds(&run_lint(&stairs(&[9, 7, 9])), LintKind::Jag), 0);
    }

    #[test]
    fn a_filled_shape_has_no_line_findings() {
        let block: Vec<_> = (5..20).flat_map(|y| (5..20).map(move |x| (x, y))).collect();
        let f = run_lint(&block);
        assert_eq!(f.len(), 0, "{f:?}");
    }

    #[test]
    fn a_doubled_step_is_flagged_but_a_box_corner_is_not() {
        let step = [(3, 3), (4, 2), (5, 2), (5, 1), (6, 0)].map(|p: (usize, usize)| (p.0, p.1 + 6));
        assert_eq!(kinds(&run_lint(&step), LintKind::Double), 1);
        let mut outline: Vec<_> = (5..15).flat_map(|i| [(i, 5), (i, 14)]).collect();
        outline.extend((5..15).flat_map(|i| [(5, i), (14, i)]));
        assert_eq!(kinds(&run_lint(&outline), LintKind::Double), 0);
    }

    #[test]
    fn a_small_l_mark_is_left_alone() {
        assert_eq!(
            kinds(&run_lint(&[(5, 5), (6, 5), (5, 6)]), LintKind::Double),
            0
        );
    }

    #[test]
    fn nearly_equal_colors_are_reported_once_at_the_rarer_one() {
        let a = u32::from_le_bytes([100, 100, 100, 255]);
        let b = u32::from_le_bytes([102, 100, 99, 255]);
        let mut grid = vec![a; 16];
        grid[9] = b;
        let f = lint(&grid, 4, 4, |c| c);
        let near: Vec<_> = f
            .iter()
            .filter(|f| f.kind == LintKind::NearColors)
            .collect();
        assert_eq!(near.len(), 1);
        assert_eq!((near[0].x, near[0].y), (1, 2));
    }

    #[test]
    fn distinct_colors_are_not_near() {
        let a = u32::from_le_bytes([100, 100, 100, 255]);
        let b = u32::from_le_bytes([110, 100, 100, 255]);
        let f = lint(&[a, b], 2, 1, |c| c);
        assert_eq!(kinds(&f, LintKind::NearColors), 0);
    }

    #[test]
    fn palette_cells_are_compared_by_the_colors_they_show() {
        let shown = |id: u32| match id {
            1 => u32::from_le_bytes([50, 50, 50, 255]),
            _ => u32::from_le_bytes([51, 50, 50, 255]),
        };
        let mut grid = vec![1; 16];
        grid[3] = 2;
        let f = lint(&grid, 4, 4, shown);
        assert_eq!(kinds(&f, LintKind::NearColors), 1);
    }

    #[test]
    fn a_spray_of_specks_is_left_alone_but_a_lone_one_is_not() {
        let spray = [(10, 10), (13, 11), (11, 14), (15, 15)];
        assert_eq!(kinds(&run_lint(&spray), LintKind::Stray), 0);
        let apart = [(4, 4), (30, 30)];
        assert_eq!(kinds(&run_lint(&apart), LintKind::Stray), 2);
    }

    #[test]
    fn a_curve_changing_direction_is_not_a_jag() {
        for runs in [
            [6usize, 2, 3].as_slice(),
            &[5, 1, 3],
            &[8, 4, 2, 1],
            &[1, 2, 4, 8],
        ] {
            assert_eq!(
                kinds(&run_lint(&stairs(runs)), LintKind::Jag),
                0,
                "{runs:?}"
            );
        }
    }

    #[test]
    fn a_doubled_staircase_is_flagged_along_its_whole_length() {
        let mut points = Vec::new();
        for i in 0..12 {
            points.extend([(6 + i, 8 + i), (7 + i, 8 + i)]);
        }
        let f = run_lint(&points);
        assert!(kinds(&f, LintKind::Double) >= 10, "{f:?}");
    }

    #[test]
    fn textured_areas_have_no_doubled_steps() {
        // The doubled-step shape, inside a patch with plenty of the same color around.
        let mut points = vec![(3, 9), (4, 8), (5, 8), (5, 7), (6, 6)];
        points.extend([
            (3, 7),
            (2, 8),
            (3, 8),
            (2, 7),
            (6, 8),
            (7, 8),
            (6, 9),
            (7, 7),
        ]);
        assert_eq!(kinds(&run_lint(&points), LintKind::Double), 0);
    }

    #[test]
    fn two_well_used_near_shades_are_a_choice() {
        let a = u32::from_le_bytes([100, 100, 100, 255]);
        let b = u32::from_le_bytes([102, 100, 99, 255]);
        let grid: Vec<u32> = (0..64).map(|i| if i % 3 == 0 { a } else { b }).collect();
        assert_eq!(kinds(&lint(&grid, 8, 8, |c| c), LintKind::NearColors), 0);
    }

    #[test]
    fn a_rare_color_is_reported_once_however_many_twins_it_has() {
        let shade = |v: u8| u32::from_le_bytes([v, 100, 100, 255]);
        let mut grid = vec![shade(100); 100];
        grid[50..75].fill(shade(102));
        grid[7] = shade(101);
        let f = lint(&grid, 10, 10, |c| c);
        assert_eq!(kinds(&f, LintKind::NearColors), 1);
    }

    #[test]
    fn findings_are_capped() {
        const SIDE: usize = 600;
        let mut grid = vec![0u32; SIDE * SIDE];
        for y in (0..SIDE).step_by(10) {
            for x in (0..SIDE).step_by(10) {
                grid[y * SIDE + x] = INK;
            }
        }
        assert_eq!(lint(&grid, SIDE, SIDE, |c| c).len(), MAX_FINDINGS);
    }

    #[test]
    fn edges_of_the_image_are_safe() {
        let f = lint(&[INK; 1], 1, 1, |c| c);
        assert_eq!(kinds(&f, LintKind::Stray), 1);
        assert!(lint(&[], 0, 0, |c| c).is_empty());
    }
}
