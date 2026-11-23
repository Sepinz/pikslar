//! Lint through a cel's offset: art that continues past the canvas edge is judged as it would be
//! on a larger canvas, and only what reaches the canvas is reported.

use super::{cut_to_canvas, lint_all, lint_cel, Finding, LintKind, LINT_MARGIN};
use crate::layer::Cel;
use crate::offset_tests::image_of;
use std::collections::HashMap;

const INK: u32 = 7;
const SHADE: u32 = 8;

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }

    fn between(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u64) as i32
    }
}

/// Stepped lines, specks and a little shading, in canvas positions inside `area`.
fn random_art(rng: &mut Rng, area: (i32, i32, i32, i32)) -> HashMap<(i32, i32), u32> {
    let (x0, y0, x1, y1) = area;
    let mut art = HashMap::new();
    for _ in 0..rng.below(6) {
        let (mut x, mut y) = (rng.between(x0, x1), rng.between(y0, y1));
        let run = rng.between(1, 4);
        for step in 0..rng.between(2, 12) {
            for _ in 0..run + (step % 3 == 2) as i32 {
                art.insert((x, y), INK);
                x += 1;
            }
            y += 1;
        }
    }
    for _ in 0..rng.below(12) {
        let value = if rng.below(8) == 0 { SHADE } else { INK };
        art.insert((rng.between(x0, x1), rng.between(y0, y1)), value);
    }
    art.retain(|&(x, y), _| (x0..=x1).contains(&x) && (y0..=y1).contains(&y));
    art
}

/// A cel showing `art`, given in canvas positions, from an offset.
fn placed(art: &HashMap<(i32, i32), u32>, at: (i32, i32)) -> Cel {
    let local = art
        .iter()
        .map(|(&(x, y), &v)| ((x - at.0, y - at.1), v))
        .collect();
    Cel::at(image_of(&local), at.0, at.1).expect("in range")
}

fn shown(v: u32) -> u32 {
    match v {
        INK => 0x2020_20ff,
        SHADE => 0x2121_21ff,
        other => other,
    }
}

fn sorted(mut found: Vec<Finding>) -> Vec<(i32, i32, i32, i32, u8)> {
    let key = |f: &Finding| (f.x, f.y, f.w, f.h, f.kind as u8);
    found.sort_by_key(key);
    found.iter().map(key).collect()
}

#[test]
fn with_nothing_past_the_edge_lint_finds_what_it_always_found() {
    let mut total = 0;
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D));
        let (w, h) = (rng.between(4, 48), rng.between(4, 48));
        let art = random_art(&mut rng, (0, 0, w - 1, h - 1));
        let at = (rng.between(-40, 40), rng.between(-40, 40));
        let cel = placed(&art, at);
        let (w, h) = (w as usize, h as usize);
        let before = lint_all(&cel.to_cells(w, h), w, h, shown);
        total += before.len();
        assert_eq!(
            sorted(lint_cel(&cel, w, h, shown)),
            sorted(before),
            "seed {seed}"
        );
    }
    assert!(total > 300, "the art gives findings to compare: {total}");
}

#[test]
fn art_past_the_edge_is_judged_as_on_a_larger_canvas() {
    let m = LINT_MARGIN as i32;
    let mut total = 0;
    for seed in 1..=1_000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let (w, h) = (rng.between(4, 40), rng.between(4, 40));
        let art = random_art(&mut rng, (-m, -m, w - 1 + m, h - 1 + m));
        let at = (rng.between(-40, 40), rng.between(-40, 40));
        let cel = placed(&art, at);
        // The same art drawn on a canvas larger by the margin on every side, by hand.
        let (wide, tall) = ((w + 2 * m) as usize, (h + 2 * m) as usize);
        let mut larger = vec![0u32; wide * tall];
        for (&(x, y), &v) in &art {
            larger[(y + m) as usize * wide + (x + m) as usize] = v;
        }
        let wanted: Vec<Finding> = lint_all(&larger, wide, tall, shown)
            .into_iter()
            .filter_map(|f| cut_to_canvas(f, m, w, h))
            .collect();
        let found = lint_cel(&cel, w as usize, h as usize, shown);
        // Each finding also knows where it began on the larger canvas, worked out by hand.
        let whole = lint_all(&larger, wide, tall, shown);
        for f in &found {
            let began = whole
                .iter()
                .any(|g| g.kind == f.kind && (g.x - m, g.y - m) == f.from);
            assert!(began, "seed {seed}: {f:?} began nowhere");
        }
        total += wanted.len();
        assert_eq!(sorted(found), sorted(wanted), "seed {seed} at {at:?}");
    }
    assert!(total > 300, "the art gives findings to compare: {total}");
}

#[test]
fn a_line_running_off_the_canvas_is_no_stray_and_art_past_it_is_never_reported() {
    // The canvas shows one pixel of a line that continues to the left, past the edge.
    let line = HashMap::from([
        ((-3, 5), INK),
        ((-2, 5), INK),
        ((-1, 5), INK),
        ((0, 5), INK),
    ]);
    let cel = placed(&line, (-6, 2));
    let alone = lint_all(&cel.to_cells(10, 10), 10, 10, shown);
    assert!(
        alone.iter().any(|f| f.kind == LintKind::Stray),
        "cut at the edge it looks lone"
    );
    assert!(lint_cel(&cel, 10, 10, shown).is_empty());

    let past = HashMap::from([((-4, -4), INK), ((12, 3), INK)]);
    assert!(lint_cel(&placed(&past, (5, 5)), 10, 10, shown).is_empty());
}

#[test]
fn a_finding_that_reaches_the_canvas_is_cut_to_it() {
    // Found at (2, 9) in a scan that began 8 pixels before the canvas: canvas x -6 to 3, y 1.
    let f = Finding {
        kind: LintKind::Jag,
        x: 2,
        y: 9,
        w: 10,
        h: 1,
        from: (2, 9),
    };
    let cut = cut_to_canvas(f, 8, 20, 20).expect("reaches the canvas");
    assert_eq!((cut.x, cut.y, cut.w, cut.h), (0, 1, 4, 1));
    assert_eq!(cut.from, (-6, 1), "where it began, past the edge");
    assert_eq!(
        cut_to_canvas(f, 8, 20, 1),
        None,
        "wholly below a canvas one row tall"
    );
    assert_eq!(
        cut_to_canvas(f, 8, 20, 20).map(|c| c.kind),
        Some(LintKind::Jag)
    );
}

/// A stepped line whose long middle run crosses the left edge, starting at canvas x `x0`.
pub(crate) fn jag_across_the_edge(x0: i32) -> HashMap<(i32, i32), u32> {
    let mut art = HashMap::new();
    let mut x = x0;
    for (row, run) in [2, 2, 2, 6, 2, 2, 2].into_iter().enumerate() {
        for _ in 0..run {
            art.insert((x, 4 + row as i32), INK);
            x += 1;
        }
    }
    art
}

#[test]
fn a_finding_across_the_edge_knows_where_it_began_wherever_its_cel_is() {
    let mut seen = Vec::new();
    for at in [(-12, 0), (-11, 0), (-12, 3)] {
        let art = jag_across_the_edge(-10);
        let moved: HashMap<_, _> = art
            .iter()
            .map(|(&(x, y), &v)| ((x + at.0 + 12, y + at.1), v))
            .collect();
        let cel = placed(&moved, at);
        let jags: Vec<Finding> = lint_cel(&cel, 20, 20, shown)
            .into_iter()
            .filter(|f| f.kind == LintKind::Jag)
            .collect();
        assert_eq!(jags.len(), 1, "at {at:?}: {jags:?}");
        let f = jags[0];
        assert_eq!(f.x, 0, "cut at the edge");
        assert!(f.from.0 < 0, "began past it: {f:?}");
        seen.push((f.from.0 - at.0, f.from.1 - at.1));
    }
    assert!(
        seen.windows(2).all(|p| p[0] == p[1]),
        "same place in the cel: {seen:?}"
    );
}
