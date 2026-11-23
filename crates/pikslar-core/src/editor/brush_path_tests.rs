//! Line and rectangle strokes with a stamp, against a reference that lays the whole stamp down
//! at every point of the path.

use super::draw::for_line;
use super::*;
use crate::color::rgba;
use crate::custom_brush::{Brush, BrushChoice, BrushPaint};
use crate::document::{ColorMode, Document};
use crate::orient::Orientation;
use std::collections::BTreeMap;
use std::sync::Arc;

const SIDE: usize = 40;

/// A small deterministic generator: the tests need variety, not quality.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, below: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as u32) % below
    }
}

/// A brush with holes and many colours, turned and with its handle somewhere.
fn random_brush(rng: &mut Lcg, paint: BrushPaint) -> Brush {
    let (w, h) = (1 + rng.next(12) as usize, 1 + rng.next(12) as usize);
    let hole = 1 + rng.next(4);
    let mut colors: Vec<u32> = (0..w * h)
        .map(|i| {
            if rng.next(hole + 1) == 0 {
                0
            } else {
                rgba(1 + (i % 250) as u8, rng.next(256) as u8, 7, 255)
            }
        })
        .collect();
    colors[0] = rgba(9, 9, 9, 255);
    let brush = Brush::from_parts("random", w, h, &colors, None, paint).expect("brush");
    let turned = brush.turned(Orientation::from_index(rng.next(8) as u8).expect("index"));
    turned.with_handle(crate::custom_brush::Handle::Corner(rng.next(4) as u8))
}

fn point(rng: &mut Lcg) -> (i32, i32) {
    (
        rng.next(SIDE as u32 + 20) as i32 - 10,
        rng.next(SIDE as u32 + 20) as i32 - 10,
    )
}

type Pixels = BTreeMap<(i32, i32), u32>;

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

/// The whole stamp at each point in turn; whatever is written last stays.
fn reference(brush: &Brush, e: &Editor, points: &[(i32, i32)]) -> Pixels {
    let stamp = brush
        .prepare(ColorMode::Rgba, &e.doc.colors, 0)
        .expect("stamp")
        .stamp;
    let mut out = Pixels::new();
    for &(px, py) in points {
        for run in stamp.runs() {
            for (i, &v) in run.values.iter().enumerate() {
                let at = (px + run.dx + i as i32, py + run.dy);
                if at.0 >= 0 && at.1 >= 0 && at.0 < SIDE as i32 && at.1 < SIDE as i32 {
                    out.insert(at, v);
                }
            }
        }
    }
    out
}

fn line_points(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for_line(a.0, a.1, b.0, b.1, |x, y| out.push((x, y)));
    out
}

fn rect_points(a: (i32, i32), b: (i32, i32)) -> Vec<(i32, i32)> {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    [
        ((x0, y0), (x1, y0)),
        ((x0, y1), (x1, y1)),
        ((x0, y0), (x0, y1)),
        ((x1, y0), (x1, y1)),
    ]
    .iter()
    .flat_map(|&(p, q)| line_points(p, q))
    .collect()
}

fn stroke(brush: &Brush, tool: Tool, a: (i32, i32), b: (i32, i32), via: (i32, i32)) -> Editor {
    let mut e = Editor::new(Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid"));
    e.pixel_perfect = false;
    e.set_brush(BrushChoice::Custom(Arc::new(brush.clone())));
    e.begin(tool, a.0, a.1).expect("begin");
    // An earlier mouse position, so the preview is redrawn from scratch at least once.
    e.drag(via.0, via.1);
    e.drag(b.0, b.1);
    e.commit();
    e
}

#[test]
fn lines_and_rectangles_match_the_whole_stamp_at_every_point() {
    let mut rng = Lcg(0x5eed);
    for round in 0..300 {
        let paint = if round % 3 == 0 {
            BrushPaint::Shape
        } else {
            BrushPaint::OwnColors
        };
        let brush = random_brush(&mut rng, paint);
        let (a, b, via) = (point(&mut rng), point(&mut rng), point(&mut rng));
        let tool = if round % 2 == 0 {
            Tool::Line
        } else {
            Tool::Rect
        };
        let e = stroke(&brush, tool, a, b, via);
        let points = if tool == Tool::Line {
            line_points(a, b)
        } else {
            rect_points(a, b)
        };
        let fill = if paint == BrushPaint::Shape {
            e.color
        } else {
            0
        };
        let mut want = reference(&brush, &e, &points);
        if paint == BrushPaint::Shape {
            want.values_mut().for_each(|v| *v = fill);
        }
        assert_eq!(drawn(&e), want, "round {round}: {tool:?} {a:?} to {b:?}");
    }
}

#[test]
fn a_freehand_drag_matches_too() {
    let mut rng = Lcg(77);
    for round in 0..100 {
        let brush = random_brush(&mut rng, BrushPaint::OwnColors);
        let path: Vec<_> = (0..4).map(|_| point(&mut rng)).collect();
        let mut e = Editor::new(Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid"));
        e.pixel_perfect = false;
        e.set_brush(BrushChoice::Custom(Arc::new(brush.clone())));
        e.begin(Tool::Pencil, path[0].0, path[0].1).expect("begin");
        for p in &path[1..] {
            e.drag(p.0, p.1);
        }
        e.commit();
        let mut points = vec![path[0]];
        for pair in path.windows(2) {
            let mut seg = line_points(pair[0], pair[1]);
            seg.retain(|p| points.last() != Some(p));
            points.extend(seg);
        }
        assert_eq!(drawn(&e), reference(&brush, &e, &points), "round {round}");
    }
}

/// `cargo test --release -p pikslar-core line_drag_timing -- --ignored --nocapture`
#[test]
#[ignore = "a measurement, not a check"]
fn line_drag_timing() {
    let side = 256;
    let colors: Vec<u32> = (0..side * side)
        .map(|i| rgba(1 + (i % 200) as u8, 5, 9, 255))
        .collect();
    let brush =
        Brush::from_parts("big", side, side, &colors, None, BrushPaint::OwnColors).expect("brush");
    let mut e = Editor::new(Document::new(1200, 700, ColorMode::Rgba).expect("valid"));
    e.set_brush(BrushChoice::Custom(Arc::new(brush)));
    e.begin(Tool::Line, 200, 200).expect("begin");
    let started = std::time::Instant::now();
    for end in [(700, 230), (702, 231), (705, 235)] {
        let each = std::time::Instant::now();
        e.drag(end.0, end.1);
        println!("drag to {end:?}: {:?}", each.elapsed());
    }
    println!("total {:?}", started.elapsed());
}
