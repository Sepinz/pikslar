//! Editing a cel placed away from the canvas origin gives what editing the same picture at the
//! origin gives, on the canvas and off it.
//!
//! The reference is the same pixels stored at offset 0, moved by hand. Tools see only canvas
//! positions, so both editors must change the same canvas pixels and leave the same pixels
//! beyond the canvas untouched.

use crate::color::rgba;
use crate::command::{CelCmd, Command, DrawCmd, FloatingCmd, SelectionCmd, StepCmd};
use crate::document::{ColorMode, Document};
use crate::editor::{Editor, Tool};
use crate::grid::Cell;
use crate::image::Image;
use crate::layer::Cel;
use crate::mask::Mask;
use crate::selection::SelectOp;
use crate::tile::{Tile, TILE_AREA};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn between(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1) as u64) as i32
    }
}

const COLORS: [u32; 3] = [
    rgba(200, 10, 10, 255),
    rgba(10, 200, 10, 255),
    rgba(10, 10, 200, 128),
];

/// An image holding exactly these pixels, given in its own coordinates.
pub(crate) fn image_of(pixels: &HashMap<(i32, i32), u32>) -> Image {
    let mut tiles: BTreeMap<(i32, i32), [u32; TILE_AREA]> = BTreeMap::new();
    for (&(x, y), &p) in pixels {
        let raw = tiles.entry((y >> 4, x >> 4)).or_insert([0; TILE_AREA]);
        raw[(((y & 15) << 4) | (x & 15)) as usize] = p;
    }
    let Some(&(top, _)) = tiles.keys().next() else {
        return Image::blank();
    };
    let bottom = tiles.keys().map(|k| k.0).max().unwrap_or(top);
    let left = tiles.keys().map(|k| k.1).min().unwrap_or(0);
    let right = tiles.keys().map(|k| k.1).max().unwrap_or(0);
    let mut cells: Vec<Cell> = Vec::new();
    for ty in top..=bottom {
        for tx in left..=right {
            cells.push(tiles.get(&(ty, tx)).and_then(Tile::pack).map(Arc::new));
        }
    }
    let wide = (right - left + 1) as u16;
    Image::from_tiles(left as i16, top as i16, wide, cells).expect("within reach")
}

/// Every stored pixel, in canvas positions.
pub(crate) fn world(cel: &Cel) -> BTreeMap<(i32, i32), u32> {
    let (cx, cy) = cel.position();
    let (x0, y0, tw, th) = cel.image().table();
    let mut out = BTreeMap::new();
    for y in y0 * 16..(y0 + th as i32) * 16 {
        for x in x0 * 16..(x0 + tw as i32) * 16 {
            let p = cel.image().get(x, y);
            if p != 0 {
                out.insert((x + cx as i32, y + cy as i32), p);
            }
        }
    }
    out
}

fn random_pixels(rng: &mut Rng, w: i32, h: i32) -> HashMap<(i32, i32), u32> {
    let mut pixels = HashMap::new();
    for _ in 0..rng.below(300) {
        // Some well past the canvas on every side, most on it.
        let (x, y) = (rng.between(-24, w + 24), rng.between(-24, h + 24));
        pixels.insert((x, y), COLORS[rng.below(3) as usize]);
    }
    pixels
}

/// Two editors showing the same picture: one at `offset`, one at the origin.
fn pair(rng: &mut Rng, w: usize, h: usize, offset: (i32, i32)) -> (Editor, Editor) {
    let canvas = random_pixels(rng, w as i32, h as i32);
    let local: HashMap<_, _> = canvas
        .iter()
        .map(|(&(x, y), &p)| ((x - offset.0, y - offset.1), p))
        .collect();
    let make = |cel: Cel| {
        let mut e = Editor::new(Document::new(w, h, ColorMode::Rgba).expect("valid"));
        e.doc.layers[0].put_cel(0, cel).expect("frame exists");
        e
    };
    let placed = Cel::at(image_of(&local), offset.0, offset.1).expect("in range");
    (make(placed), make(Cel::from_image(image_of(&canvas))))
}

fn random_command(rng: &mut Rng, w: i32, h: i32) -> crate::command::Command {
    let point = |rng: &mut Rng| (rng.between(-6, w + 6), rng.between(-6, h + 6));
    match rng.below(16) {
        0..=3 => {
            let tool = [Tool::Pencil, Tool::Eraser, Tool::Line, Tool::Rect][rng.below(4) as usize];
            let path = (0..rng.between(1, 5)).map(|_| point(rng)).collect();
            DrawCmd::Stroke { tool, path }.into()
        }
        4 => DrawCmd::Stroke {
            tool: Tool::Fill,
            path: vec![point(rng)],
        }
        .into(),
        5 => {
            let (x, y) = point(rng);
            let shape = Mask::rect(x, y, rng.between(1, 20), rng.between(1, 20));
            SelectionCmd::Select {
                shape: Box::new(shape),
                op: SelectOp::Replace,
            }
            .into()
        }
        6 => FloatingCmd::Lift {
            copy: rng.below(2) == 0,
        }
        .into(),
        7 => FloatingCmd::Move {
            dx: rng.between(-20, 20),
            dy: rng.between(-20, 20),
            join: false,
        }
        .into(),
        8 => [
            FloatingCmd::Anchor,
            FloatingCmd::FlipHorizontal,
            FloatingCmd::Rotate { clockwise: true },
        ][rng.below(3) as usize]
            .clone()
            .into(),
        9 => DrawCmd::DeleteSelected.into(),
        10 => StepCmd::Undo.into(),
        11 => StepCmd::Redo.into(),
        12 | 13 => CelCmd::Move {
            layer: crate::LayerId(0),
            frame: 0,
            dx: rng.between(-20, 20),
            dy: rng.between(-20, 20),
        }
        .into(),
        14 => StepCmd::EndDrag.into(),
        _ => CelCmd::Turn {
            layer: crate::LayerId(0),
            frame: 0,
            turn: [
                crate::Turn::FlipHorizontal,
                crate::Turn::FlipVertical,
                crate::Turn::Clockwise,
                crate::Turn::CounterClockwise,
            ][rng.below(4) as usize],
        }
        .into(),
    }
}

/// Layer ids belong to one session, so a move names each editor's own layer.
fn for_editor(e: &Editor, command: &Command) -> Command {
    match command {
        Command::Cel(CelCmd::Move { frame, dx, dy, .. }) => CelCmd::Move {
            layer: e.layer_id(0).expect("layer"),
            frame: *frame,
            dx: *dx,
            dy: *dy,
        }
        .into(),
        Command::Cel(CelCmd::Turn { frame, turn, .. }) => CelCmd::Turn {
            layer: e.layer_id(0).expect("layer"),
            frame: *frame,
            turn: *turn,
        }
        .into(),
        other => other.clone(),
    }
}

fn assert_same(a: &Editor, b: &Editor, what: &str) {
    let (ca, cb) = (a.doc.layers[0].cel(0), b.doc.layers[0].cel(0));
    assert_eq!(world(ca), world(cb), "{what}: stored pixels differ");
    assert_eq!(a.selection(), b.selection(), "{what}: selection differs");
}

#[test]
fn editing_at_an_offset_matches_editing_the_same_picture_at_the_origin() {
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D));
        let (w, h) = (rng.between(8, 60) as usize, rng.between(8, 60) as usize);
        let offset = match rng.below(3) {
            0 => (rng.between(-4, 4) * 16, rng.between(-4, 4) * 16),
            _ => (rng.between(-50, 50), rng.between(-50, 50)),
        };
        let (mut a, mut b) = pair(&mut rng, w, h, offset);
        a.color = COLORS[0];
        b.color = COLORS[0];
        let start = world(b.doc.layers[0].cel(0));
        for step in 0..30 {
            let command = random_command(&mut rng, w as i32, h as i32);
            let what = format!("seed {seed} step {step} at {offset:?}: {command:?}");
            let (ra, rb) = (
                a.apply(for_editor(&a, &command)),
                b.apply(for_editor(&b, &command)),
            );
            assert_eq!(ra, rb, "{what}: outcome differs");
            assert_same(&a, &b, &what);
            a.color = COLORS[rng.below(3) as usize];
            b.color = a.color;
        }
        a.apply(FloatingCmd::Anchor).expect("anchor");
        b.apply(FloatingCmd::Anchor).expect("anchor");
        a.apply(StepCmd::ScrubTo { depth: 0 }).expect("scrub");
        b.apply(StepCmd::ScrubTo { depth: 0 }).expect("scrub");
        assert_same(&a, &b, &format!("seed {seed} after undoing everything"));
        assert_eq!(
            world(b.doc.layers[0].cel(0)),
            start,
            "seed {seed}: undo lost pixels"
        );
    }
}

#[test]
fn a_stroke_never_touches_what_lies_past_the_canvas() {
    let mut rng = Rng(99);
    let (mut a, _) = pair(&mut rng, 20, 20, (-7, 5));
    let beyond = |e: &Editor| {
        world(e.doc.layers[0].cel(0))
            .into_iter()
            .filter(|&((x, y), _)| !(0..20).contains(&x) || !(0..20).contains(&y))
            .collect::<Vec<_>>()
    };
    let before = beyond(&a);
    assert!(!before.is_empty(), "the test needs pixels past the canvas");
    a.apply(DrawCmd::Stroke {
        tool: Tool::Eraser,
        path: vec![(-30, -30), (50, 50), (-30, 50), (50, -30)],
    })
    .expect("stroke");
    a.apply(SelectionCmd::All).expect("select");
    a.apply(DrawCmd::DeleteSelected).expect("delete");
    a.apply(DrawCmd::Stroke {
        tool: Tool::Fill,
        path: vec![(3, 3)],
    })
    .expect("fill");
    assert_eq!(beyond(&a), before);
}

#[test]
fn a_stroke_that_would_grow_a_table_is_refused_without_memory() {
    let mut e = Editor::new(Document::new(8192, 8192, ColorMode::Rgba).expect("valid"));
    let corner = HashMap::from([((-24_576, -24_576), COLORS[0])]);
    let cel = Cel::at(image_of(&corner), 24_576, 24_576).expect("in range");
    e.doc.layers[0].put_cel(0, cel).expect("frame exists");
    e.color = COLORS[0];
    assert_eq!(e.doc.layers[0].cel(0).get(0, 0), COLORS[0]);
    let before = e.doc.clone();
    e.set_memory_check(|_| false);
    let far = DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(8191, 8191)],
    };
    assert_eq!(e.apply(far.clone()), Err(crate::LimitError::NoMemory));
    assert!(e.doc == before, "the document changed");
    e.set_memory_check(|_| true);
    e.apply(far).expect("room");
    assert_eq!(e.doc.layers[0].cel(0).get(8191, 8191), COLORS[0]);
    assert!(
        e.widest_table >= 512 * 512 * 8,
        "the grown table is remembered"
    );
}

#[test]
fn the_drawn_bounds_hold_exactly_every_stored_pixel() {
    let mut rng = Rng(0x5DEE_CE66_D1CE_4E5B);
    for seed in 0..2_000 {
        let (w, h) = (rng.between(1, 80), rng.between(1, 80));
        let mut pixels = random_pixels(&mut rng, w, h);
        if rng.below(4) == 0 {
            // Sparse: one or two pixels, tiles apart or in one tile.
            pixels.retain(|_, _| rng.below(40) == 0);
        }
        if rng.below(3) == 0 {
            // Whole tiles of one color, stored solid, sometimes at the drawing's edge.
            let (bx, by) = (rng.between(-2, 6) * 16, rng.between(-2, 6) * 16);
            let (bw, bh) = (rng.between(1, 3) * 16, rng.between(1, 3) * 16);
            for (x, y) in (bx..bx + bw).flat_map(|x| (by..by + bh).map(move |y| (x, y))) {
                pixels.insert((x, y), COLORS[1]);
            }
        }
        let at = (rng.between(-500, 500), rng.between(-500, 500));
        let cel = Cel::at(image_of(&pixels), at.0, at.1).expect("in range");
        let seen = world(&cel);
        let naive = seen
            .keys()
            .fold(None, |b: Option<(i32, i32, i32, i32)>, &(x, y)| {
                Some(b.map_or((x, y, x, y), |(a, c, d, e)| {
                    (a.min(x), c.min(y), d.max(x), e.max(y))
                }))
            });
        let want = naive.map(|(x0, y0, x1, y1)| (x0, y0, x1 - x0 + 1, y1 - y0 + 1));
        assert_eq!(cel.drawn_bounds(), want, "seed {seed} at {at:?}");
    }
}

#[test]
fn a_turn_that_makes_a_wide_table_is_remembered_for_the_memory_check() {
    let mut e = Editor::new(Document::new(64, 64, ColorMode::Rgba).expect("valid"));
    // Placed after the editor was made, so only the turn can tell it how wide the table is.
    let far = HashMap::from([((0, 0), COLORS[0]), ((3000, 2000), COLORS[1])]);
    e.doc.layers[0]
        .put_cel(0, Cel::at(image_of(&far), 0, 0).expect("in range"))
        .expect("frame");
    let layer = e.layer_id(0).expect("layer");
    e.apply(CelCmd::Turn {
        layer,
        frame: 0,
        turn: crate::Turn::Clockwise,
    })
    .expect("turn");
    let turned = e.doc.layers[0].cel(0).table_bytes();
    assert!(turned >= 180 * 120 * 8, "the table is wide: {turned}");
    assert!(
        e.widest_table >= turned,
        "remembered {} of {turned}",
        e.widest_table
    );
}
