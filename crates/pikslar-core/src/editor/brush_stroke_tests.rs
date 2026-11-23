//! Strokes with a custom brush, checked against a pixel model that does not use the stamp code.

use super::draw::for_line;
use super::*;
use crate::color::rgba;
use crate::custom_brush::{Brush, BrushChoice, BrushPaint, Preset};
use crate::document::{ColorMode, Document, LimitError};
use crate::orient::Orientation;
use crate::{DrawCmd, StepCmd};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

const RED: u32 = rgba(255, 0, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);
const GREEN: u32 = rgba(0, 255, 0, 255);
const INK: u32 = rgba(200, 100, 50, 255);
const SIDE: usize = 32;
const MODES: [ColorMode; 2] = [ColorMode::Palette, ColorMode::Rgba];
/// A 3 by 2 brush that no turn leaves looking the same.
const SOURCE: [u32; 6] = [RED, 0, BLUE, GREEN, BLUE, 0];

type Pixels = BTreeMap<(i32, i32), u32>;

fn editor(mode: ColorMode) -> Editor {
    let mut e = Editor::new(Document::new(SIDE, SIDE, mode).expect("valid"));
    e.pixel_perfect = false;
    e.color = INK;
    e
}

fn brush(paint: BrushPaint, o: Orientation) -> Brush {
    Brush::from_parts("test", 3, 2, &SOURCE, None, paint)
        .expect("brush")
        .turned(o)
}

fn choose(e: &mut Editor, paint: BrushPaint, o: Orientation) {
    e.set_brush(BrushChoice::Custom(std::sync::Arc::new(brush(paint, o))));
}

fn drawn(e: &Editor) -> Pixels {
    let mut out = Pixels::new();
    for y in 0..SIDE as i32 {
        for x in 0..SIDE as i32 {
            if e.color_at(x, y) != 0 {
                out.insert((x, y), e.color_at(x, y));
            }
        }
    }
    out
}

/// The brush laid down at each point in turn, by forward mapping of the source pixels.
fn model(paint: BrushPaint, o: Orientation, fill: u32, points: &[(i32, i32)]) -> Pixels {
    let (ow, oh) = o.turned_size(3, 2);
    let (hx, hy) = (((ow - 1) / 2) as i32, ((oh - 1) / 2) as i32);
    let mut out = Pixels::new();
    for &(px, py) in points {
        for (i, &c) in SOURCE.iter().enumerate().filter(|(_, &c)| c != 0) {
            let (tx, ty) = o.apply(i % 3, i / 3, 3, 2);
            let value = if paint == BrushPaint::Shape { fill } else { c };
            out.insert((px + tx as i32 - hx, py + ty as i32 - hy), value);
        }
    }
    out
}

fn path(points: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut out = vec![points[0]];
    for pair in points.windows(2) {
        for_line(pair[0].0, pair[0].1, pair[1].0, pair[1].1, |x, y| {
            out.push((x, y))
        });
    }
    out
}

fn stroke(e: &mut Editor, tool: Tool, pts: &[(i32, i32)]) {
    e.apply(DrawCmd::Begin {
        tool,
        x: pts[0].0,
        y: pts[0].1,
    })
    .expect("begin");
    for p in &pts[1..] {
        e.apply(DrawCmd::To { x: p.0, y: p.1 }).expect("to");
    }
    e.apply(DrawCmd::Commit).expect("commit");
}

const WANDER: [(i32, i32); 4] = [(10, 10), (17, 12), (13, 19), (20, 24)];

#[test]
fn pencil_matches_the_model_in_every_orientation_mode_and_paint() {
    for mode in MODES {
        for paint in [BrushPaint::OwnColors, BrushPaint::Shape] {
            for o in Orientation::ALL {
                let mut e = editor(mode);
                choose(&mut e, paint, o);
                stroke(&mut e, Tool::Pencil, &WANDER);
                let want = model(paint, o, INK, &path(&WANDER));
                assert_eq!(drawn(&e), want, "{mode:?} {paint:?} {o:?}");
            }
        }
    }
}

#[test]
fn line_and_rectangle_stamp_along_their_shapes() {
    for mode in MODES {
        for o in [Orientation::IDENTITY, Orientation::rotate90()] {
            let mut e = editor(mode);
            choose(&mut e, BrushPaint::OwnColors, o);
            stroke(&mut e, Tool::Line, &[(6, 6), (12, 9), (24, 20)]);
            let want = model(BrushPaint::OwnColors, o, INK, &path(&[(6, 6), (24, 20)]));
            assert_eq!(drawn(&e), want, "line {mode:?} {o:?}");

            let mut e = editor(mode);
            choose(&mut e, BrushPaint::Shape, o);
            stroke(&mut e, Tool::Rect, &[(8, 8), (14, 14), (22, 18)]);
            let sides = [
                [(8, 8), (22, 8)],
                [(8, 18), (22, 18)],
                [(8, 8), (8, 18)],
                [(22, 8), (22, 18)],
            ];
            let points: Vec<_> = sides.iter().flat_map(|s| path(s)).collect();
            assert_eq!(drawn(&e), model(BrushPaint::Shape, o, INK, &points), "rect");
        }
    }
}

#[test]
fn the_eraser_takes_the_shape_of_a_brush_with_its_own_colours() {
    for mode in MODES {
        let mut e = editor(mode);
        e.set_brush(BrushChoice::Round);
        for y in 5..25 {
            stroke(&mut e, Tool::Line, &[(5, y), (25, y)]);
        }
        choose(
            &mut e,
            BrushPaint::OwnColors,
            Orientation::flip_horizontal(),
        );
        let before = drawn(&e);
        stroke(&mut e, Tool::Eraser, &[(12, 12), (16, 14)]);
        let mut want = before;
        for k in model(
            BrushPaint::Shape,
            Orientation::flip_horizontal(),
            0,
            &path(&[(12, 12), (16, 14)]),
        )
        .keys()
        {
            want.remove(k);
        }
        assert_eq!(drawn(&e), want, "{mode:?}");
    }
}

#[test]
fn a_preset_paints_the_current_colour_and_size_one_stays_the_pencil() {
    let mut e = editor(ColorMode::Rgba);
    e.set_brush(BrushChoice::Preset(Preset::Square));
    e.brush_size = 3;
    stroke(&mut e, Tool::Pencil, &[(10, 10)]);
    assert_eq!(drawn(&e).len(), 9);
    assert!(drawn(&e).values().all(|&c| c == INK));

    let (mut a, mut b) = (editor(ColorMode::Rgba), editor(ColorMode::Rgba));
    a.pixel_perfect = true;
    b.pixel_perfect = true;
    b.set_brush(BrushChoice::Preset(Preset::Diamond));
    let zigzag = [(2, 2), (3, 2), (3, 3), (4, 3), (4, 4)];
    stroke(&mut a, Tool::Pencil, &zigzag);
    stroke(&mut b, Tool::Pencil, &zigzag);
    assert_eq!(drawn(&a), drawn(&b));
}

#[test]
fn a_stroke_is_one_undo_step_and_undo_redo_give_identical_pixels() {
    for mode in MODES {
        let mut e = editor(mode);
        let blank = drawn(&e);
        choose(&mut e, BrushPaint::OwnColors, Orientation::rotate270());
        stroke(&mut e, Tool::Pencil, &WANDER);
        let painted = drawn(&e);
        let table = e.doc().colors.colors().to_vec();
        assert_eq!(e.history().undo_depth(), 1);
        e.apply(StepCmd::Undo).expect("undo");
        assert_eq!(drawn(&e), blank);
        assert_eq!(e.doc().colors.len(), 1, "{mode:?}: colours given back");
        e.apply(StepCmd::Redo).expect("redo");
        assert_eq!(drawn(&e), painted);
        assert_eq!(e.doc().colors.colors(), &table[..]);
    }
}

#[test]
fn replaying_the_same_input_gives_the_same_document() {
    for mode in MODES {
        let run = || {
            let mut e = editor(mode);
            choose(&mut e, BrushPaint::OwnColors, Orientation::transpose());
            stroke(&mut e, Tool::Pencil, &WANDER);
            e.set_brush_paint(BrushPaint::Shape);
            stroke(&mut e, Tool::Line, &[(3, 3), (28, 9)]);
            (drawn(&e), e.doc().colors.colors().to_vec())
        };
        assert_eq!(run(), run());
    }
}

#[test]
fn cancel_gives_back_the_colour_table() {
    let mut e = editor(ColorMode::Palette);
    let before = e.doc().colors.clone();
    choose(&mut e, BrushPaint::OwnColors, Orientation::IDENTITY);
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 10,
        y: 10,
    })
    .expect("begin");
    assert!(e.doc().colors.len() > before.len());
    e.apply(DrawCmd::To { x: 14, y: 14 }).expect("to");
    e.apply(DrawCmd::Cancel).expect("cancel");
    assert!(Arc::ptr_eq(&e.doc().colors, &before));
    assert!(drawn(&e).is_empty());
    assert_eq!(e.history().undo_depth(), 0);
}

#[test]
fn a_full_table_refuses_before_anything_changes() {
    let mut e = editor(ColorMode::Palette);
    let table = Arc::make_mut(&mut e.doc.colors);
    let mut n = 0u32;
    while table.id_for(rgba(n as u8, (n >> 8) as u8, 9, 255)).is_ok() {
        n += 1;
    }
    choose(&mut e, BrushPaint::OwnColors, Orientation::IDENTITY);
    let r = e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 4,
        y: 4,
    });
    assert_eq!(r.err(), Some(LimitError::TableFull));
    assert!(!e.is_stroking());
    assert_eq!(e.doc().colors.len(), crate::color::MAX_COLORS);
    // A shape brush paints a colour the table already holds, so it is not refused.
    choose(&mut e, BrushPaint::Shape, Orientation::IDENTITY);
    e.color = rgba(1, 0, 9, 255);
    stroke(&mut e, Tool::Pencil, &[(4, 4)]);
    assert_eq!(drawn(&e).len(), 4);
}

#[test]
fn a_locked_layer_refuses_a_brush_stroke() {
    let mut e = editor(ColorMode::Rgba);
    choose(&mut e, BrushPaint::OwnColors, Orientation::IDENTITY);
    e.doc.layers[0].locked = true;
    let r = e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 4,
        y: 4,
    });
    assert_eq!(r.err(), Some(LimitError::Locked));
    assert!(!e.is_stroking());
    assert!(drawn(&e).is_empty());
}

static ASKED: AtomicUsize = AtomicUsize::new(0);

fn note(bytes: usize) -> bool {
    ASKED.store(bytes, Ordering::SeqCst);
    true
}

fn asked_for_begin(e: &mut Editor) -> usize {
    e.set_memory_check(note);
    ASKED.store(0, Ordering::SeqCst);
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 100,
        y: 100,
    })
    .expect("begin");
    e.apply(DrawCmd::Cancel).expect("cancel");
    ASKED.load(Ordering::SeqCst)
}

#[test]
fn the_memory_allowance_grows_with_the_brush_area() {
    let mut e = Editor::new(Document::new(512, 512, ColorMode::Rgba).expect("valid"));
    let round = asked_for_begin(&mut e);
    let side = 200;
    let big = Brush::from_parts(
        "big",
        side,
        side,
        &vec![RED; side * side],
        None,
        BrushPaint::OwnColors,
    )
    .expect("brush");
    e.set_brush(BrushChoice::Custom(Arc::new(big)));
    let with_brush = asked_for_begin(&mut e);
    assert!(
        with_brush >= round + side * side * 4,
        "{round} {with_brush}"
    );
}

#[test]
fn a_wide_stamp_marks_each_tile_it_reaches_once() {
    // Rgba only: a palette stroke that adds colours redraws the whole canvas instead.
    let mut e = Editor::new(Document::new(64, 64, ColorMode::Rgba).expect("valid"));
    let side = 20;
    let colors = vec![RED; side * side];
    let wide = Brush::from_parts("wide", side, side, &colors, None, BrushPaint::OwnColors);
    e.set_brush(BrushChoice::Custom(std::sync::Arc::new(
        wide.expect("brush"),
    )));
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 24,
        y: 24,
    })
    .expect("begin");
    e.take_damage();
    e.apply(DrawCmd::To { x: 25, y: 24 }).expect("drag");
    let damage = e.take_damage();
    let Damage::Tiles(tiles) = damage else {
        panic!("a stamp damages tiles, not {damage:?}");
    };
    // Cells 16..36 on each side: tiles 1 and 2 across and down of a 4 by 4 canvas.
    let mut sorted = tiles.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), tiles.len(), "a tile listed twice");
    assert_eq!(sorted, vec![1, 2, 5, 6, 9, 10]);
    e.apply(DrawCmd::Commit).expect("commit");
    assert_eq!(e.color_at(16, 15), RED);
    assert_eq!(e.color_at(35, 34), RED);
}
