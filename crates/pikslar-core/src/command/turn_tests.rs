//! Turning a whole drawing: every pixel, on the canvas and off it, goes where a hand-made map
//! puts it, and the canvas part matches turning a selection of the whole canvas.

use super::{CelCmd, FloatingCmd, FrameCmd, SelectionCmd, StepCmd};
use crate::color::rgba;
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::Editor;
use crate::layer::{Cel, Turn, OFFSET_LIMIT};
use crate::offset_tests::{image_of, world};
use std::collections::{BTreeMap, HashMap};

const TURNS: [Turn; 4] = [
    Turn::FlipHorizontal,
    Turn::FlipVertical,
    Turn::Clockwise,
    Turn::CounterClockwise,
];

/// The rule, written out again by hand.
fn expected(turn: Turn, w: i32, h: i32, (x, y): (i32, i32)) -> (i32, i32) {
    let (ox, oy) = ((w - h) / 2, (h - w) / 2);
    match turn {
        Turn::FlipHorizontal => (w - 1 - x, y),
        Turn::FlipVertical => (x, h - 1 - y),
        Turn::Clockwise => (ox + (h - 1 - y), oy + x),
        Turn::CounterClockwise => (ox + y, oy + (w - 1 - x)),
    }
}

fn mapped(
    pixels: &BTreeMap<(i32, i32), u32>,
    turn: Turn,
    w: i32,
    h: i32,
) -> BTreeMap<(i32, i32), u32> {
    pixels
        .iter()
        .map(|(&p, &v)| (expected(turn, w, h, p), v))
        .collect()
}

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

fn editor_with(w: usize, h: usize, local: &HashMap<(i32, i32), u32>, at: (i32, i32)) -> Editor {
    let mut e = Editor::new(Document::new(w, h, ColorMode::Rgba).expect("valid"));
    let cel = Cel::at(image_of(local), at.0, at.1).expect("in range");
    e.doc.layers[0].put_cel(0, cel).expect("frame");
    e
}

fn turn(e: &mut Editor, frame: usize, turn: Turn) -> Result<bool, LimitError> {
    let layer = e.layer_id(0).expect("layer");
    e.apply(CelCmd::Turn { layer, frame, turn })
        .map(|o| o.changed)
}

fn pixels(e: &Editor, frame: usize) -> BTreeMap<(i32, i32), u32> {
    world(e.doc.layers[0].cel(frame))
}

fn on_canvas(p: &BTreeMap<(i32, i32), u32>, w: i32, h: i32) -> Vec<((i32, i32), u32)> {
    p.iter()
        .filter(|((x, y), _)| (0..w).contains(x) && (0..h).contains(y))
        .map(|(&k, &v)| (k, v))
        .collect()
}

#[test]
fn every_stored_pixel_goes_where_the_rule_puts_it() {
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let (w, h) = (rng.between(1, 70), rng.between(1, 70));
        let mut local = HashMap::new();
        for _ in 0..rng.below(200) + 1 {
            let p = (rng.between(-40, w + 40), rng.between(-40, h + 40));
            local.insert(p, rgba(rng.below(255) as u8, 9, 9, 255));
        }
        let at = (rng.between(-60, 60), rng.between(-60, 60));
        let mut e = editor_with(w as usize, h as usize, &local, at);
        let start = pixels(&e, 0);
        let mut want = start.clone();
        for step in 0..6 {
            let t = TURNS[rng.below(4) as usize];
            assert!(turn(&mut e, 0, t).expect("turn"), "seed {seed} step {step}");
            want = mapped(&want, t, w, h);
            let what = format!("seed {seed} step {step}: {t:?} on {w}x{h} at {at:?}");
            assert_eq!(pixels(&e, 0), want, "{what}");
        }
        e.apply(StepCmd::ScrubTo { depth: 0 }).expect("scrub");
        assert_eq!(pixels(&e, 0), start, "seed {seed}: undo");
    }
}

#[test]
fn four_quarter_turns_and_two_flips_give_back_the_picture() {
    for (w, h) in [(16, 16), (17, 5), (5, 32), (1, 1), (64, 9)] {
        let local = HashMap::from([((0, 0), 1), ((-30, 3), 2), ((w + 9, h), 3), ((2, -17), 4)]);
        let mut e = editor_with(w as usize, h as usize, &local, (7, -3));
        let start = pixels(&e, 0);
        for _ in 0..4 {
            turn(&mut e, 0, Turn::Clockwise).expect("turn");
        }
        assert_eq!(pixels(&e, 0), start, "{w}x{h} clockwise");
        for _ in 0..4 {
            turn(&mut e, 0, Turn::CounterClockwise).expect("turn");
        }
        assert_eq!(pixels(&e, 0), start, "{w}x{h} counter-clockwise");
        for t in [Turn::FlipHorizontal, Turn::FlipVertical] {
            turn(&mut e, 0, t).expect("turn");
            turn(&mut e, 0, t).expect("turn");
            assert_eq!(pixels(&e, 0), start, "{w}x{h} {t:?} twice");
        }
    }
}

#[test]
fn the_canvas_part_matches_turning_a_selection_of_the_whole_canvas() {
    let commands = [
        (Turn::FlipHorizontal, FloatingCmd::FlipHorizontal),
        (Turn::FlipVertical, FloatingCmd::FlipVertical),
        (Turn::Clockwise, FloatingCmd::Rotate { clockwise: true }),
        (
            Turn::CounterClockwise,
            FloatingCmd::Rotate { clockwise: false },
        ),
    ];
    for (w, h) in [(12, 12), (13, 6), (6, 11)] {
        // Opaque everywhere, so the selection's bounds are the canvas.
        let mut local = HashMap::new();
        for y in 0..h {
            for x in 0..w {
                local.insert((x, y), rgba(x as u8 * 9, y as u8 * 9, 1, 255));
            }
        }
        for (t, floating) in commands.clone() {
            let mut whole = editor_with(w as usize, h as usize, &local, (0, 0));
            turn(&mut whole, 0, t).expect("turn");
            let mut selected = editor_with(w as usize, h as usize, &local, (0, 0));
            selected.apply(SelectionCmd::All).expect("select");
            selected.apply(floating).expect("turn");
            selected.apply(FloatingCmd::Anchor).expect("anchor");
            assert_eq!(
                on_canvas(&pixels(&whole, 0), w, h),
                on_canvas(&pixels(&selected, 0), w, h),
                "{t:?} {w}x{h}"
            );
        }
    }
}

#[test]
fn an_aligned_cel_stays_aligned_when_it_can() {
    let local = HashMap::from([((3, 3), 1), ((40, -9), 2)]);
    let mut e = editor_with(32, 32, &local, (0, 0));
    turn(&mut e, 0, Turn::FlipHorizontal).expect("turn");
    assert_eq!(e.doc.layers[0].cel(0).position(), (0, 0));
    let mut e = editor_with(32, 20, &local, (-48, 16));
    turn(&mut e, 0, Turn::Clockwise).expect("turn");
    let (x, y) = e.doc.layers[0].cel(0).position();
    assert_eq!((x % 16, y % 16), (0, 0));
}

#[test]
fn a_turn_at_the_range_limit_keeps_every_pixel_or_changes_nothing() {
    let far = HashMap::from([((-OFFSET_LIMIT, 0), 1), ((0, 0), 2), ((7, 7), 3)]);
    for at in [
        (OFFSET_LIMIT, 0),
        (-OFFSET_LIMIT, OFFSET_LIMIT),
        (0, -OFFSET_LIMIT),
    ] {
        for t in TURNS {
            let mut e = editor_with(8192, 8192, &far, at);
            let start = pixels(&e, 0);
            let before = e.doc.clone();
            match turn(&mut e, 0, t) {
                Ok(true) => assert_eq!(pixels(&e, 0), mapped(&start, t, 8192, 8192)),
                Err(LimitError::OutOfReach) => assert!(e.doc == before, "{t:?} at {at:?}"),
                other => panic!("{t:?} at {at:?}: {other:?}"),
            }
        }
    }
}

#[test]
fn a_drawing_spread_too_far_is_refused_and_left_alone() {
    // Pixels as far apart as an image can hold: a flip that spreads them past what any offset
    // in range can reach is refused.
    let wide = HashMap::from([((-32_768, 0), 1), ((32_767, 0), 2)]);
    let mut e = editor_with(8, 8, &wide, (-OFFSET_LIMIT, 0));
    let before = e.doc.clone();
    assert_eq!(
        turn(&mut e, 0, Turn::FlipHorizontal),
        Err(LimitError::OutOfReach)
    );
    assert!(e.doc == before && !e.history().can_undo());
}

#[test]
fn a_hold_turns_together_and_a_scattered_hold_is_refused() {
    let local = HashMap::from([((1, 2), 5), ((-9, 4), 6)]);
    let mut e = editor_with(16, 16, &local, (3, 3));
    let layer = e.layer_id(0).expect("layer");
    for at in 1..3 {
        e.apply(FrameCmd::Add { at, copy: true }).expect("frame");
    }
    e.apply(FrameCmd::HoldAcross {
        layer,
        source: 0,
        from: 0,
        to: 1,
    })
    .expect("hold");
    let start = pixels(&e, 0);
    turn(&mut e, 1, Turn::FlipVertical).expect("turn");
    let want = mapped(&start, Turn::FlipVertical, 16, 16);
    assert_eq!(pixels(&e, 0), want);
    assert_eq!(pixels(&e, 1), want);
    let cels = &e.doc.layers[0];
    assert!(cels.cel(0).same_image(cels.cel(1)), "still one drawing");
    assert_eq!(pixels(&e, 2), start, "outside the hold");

    let image = e.doc.layers[0].cel(0).image().clone();
    let apart = Cel::at(image, 40, 0).expect("in range");
    e.doc.layers[0].put_cel(1, apart).expect("frame");
    let before = e.doc.clone();
    assert_eq!(
        turn(&mut e, 0, Turn::Clockwise),
        Err(LimitError::UnequalHold)
    );
    assert!(e.doc == before);
}

#[test]
fn turning_nothing_is_no_step() {
    let mut e = editor_with(8, 8, &HashMap::new(), (0, 0));
    assert_eq!(turn(&mut e, 0, Turn::Clockwise), Ok(false));
    assert!(!e.history().can_undo());
}

#[test]
fn a_drawing_that_barely_fits_takes_the_aligned_offset_inside_its_window() {
    // Spread over all but 20 pixels of an image's reach. Flipped on a canvas one pixel wide,
    // only offsets 1 to 21 hold it, and 16 is the one on a tile edge.
    let wide = HashMap::from([((-32_768, 0), 1), ((32_747, 0), 2)]);
    let mut e = editor_with(1, 8, &wide, (0, 0));
    let start = pixels(&e, 0);
    assert_eq!(turn(&mut e, 0, Turn::FlipHorizontal), Ok(true));
    assert_eq!(e.doc.layers[0].cel(0).position(), (16, 0));
    assert_eq!(pixels(&e, 0), mapped(&start, Turn::FlipHorizontal, 1, 8));
}
