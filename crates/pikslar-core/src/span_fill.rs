//! The scanline flood shared by the fill tool and the magic wand.

/// Visits every cell of the 4-connected region around `start` that `open` accepts.
///
/// The stack holds one entry per run of cells rather than one per cell, so a blank
/// 8192 by 8192 layer needs a few kilobytes of stack instead of gigabytes.
pub(crate) fn scan_fill(
    (w, h): (i32, i32),
    start: (i32, i32),
    open: impl Fn(i32, i32) -> bool,
    mut visit: impl FnMut(i32, i32),
) {
    let mut seen = vec![false; (w * h) as usize];
    let mut stack = vec![start];
    while let Some((sx, sy)) = stack.pop() {
        let matches = |px: i32, seen: &[bool]| {
            px >= 0 && px < w && !seen[(sy * w + px) as usize] && open(px, sy)
        };
        if !matches(sx, &seen) {
            continue;
        }
        let mut left = sx;
        while matches(left - 1, &seen) {
            left -= 1;
        }
        let mut px = left;
        let (mut above, mut below) = (false, false);
        while matches(px, &seen) {
            seen[(sy * w + px) as usize] = true;
            visit(px, sy);
            let free = |ny: i32| ny >= 0 && ny < h && !seen[(ny * w + px) as usize] && open(px, ny);
            note_span(&mut above, free(sy - 1), &mut stack, px, sy - 1);
            note_span(&mut below, free(sy + 1), &mut stack, px, sy + 1);
            px += 1;
        }
    }
}

/// Remembers the first cell of each run on a neighbouring row.
fn note_span(inside_run: &mut bool, open: bool, stack: &mut Vec<(i32, i32)>, x: i32, y: i32) {
    if open && !*inside_run {
        stack.push((x, y));
    }
    *inside_run = open;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(rows: &[&str], start: (i32, i32)) -> Vec<(i32, i32)> {
        let (w, h) = (rows[0].len() as i32, rows.len() as i32);
        let open = |x: i32, y: i32| rows[y as usize].as_bytes()[x as usize] == b'.';
        let mut found = Vec::new();
        scan_fill((w, h), start, open, |x, y| found.push((x, y)));
        found.sort_unstable();
        found
    }

    #[test]
    fn fills_around_corners_and_stops_at_walls() {
        let rows = ["..#.", "#.#.", "...#"];
        let expected = [(0, 0), (0, 2), (1, 0), (1, 1), (1, 2), (2, 2)];
        assert_eq!(region(&rows, (0, 0)), expected);
    }

    #[test]
    fn diagonal_neighbours_are_not_connected() {
        assert_eq!(region(&[".#", "#."], (0, 0)), [(0, 0)]);
    }

    #[test]
    fn a_blocked_start_visits_nothing() {
        assert!(region(&["#."], (0, 0)).is_empty());
    }
}
