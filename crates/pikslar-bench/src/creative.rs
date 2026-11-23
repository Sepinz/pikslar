//! The creative tools' own measurements (M8 and on). They live apart from `workloads::measure`
//! because `check` fails on any metric with no baseline line: these are judged by
//! `creative-check` against `perf/creative-baseline.txt`, which later milestones only append to.

use crate::gate::{Kind, Metric};
use crate::total_allocs;
use crate::workloads::{best_of, document, percentile, push, ramp, walk, Canvas, Rng};
use pikslar_core::custom_brush::{Brush, BrushChoice, BrushPaint};
use pikslar_core::{DrawCmd, Editor, Mask, SelectOp, SelectionCmd, Tool};
use pikslar_format::brush_lib::{read_library, write_library, Handle, LibraryBrush, Paint};
use std::hint::black_box;
use std::time::Instant;

const CANVAS: usize = 512;
const EVENTS_32: usize = 3000;
const EVENTS_256: usize = 800;
/// Brushes in the library whose load is timed: the most a library may hold.
const LIBRARY_BRUSHES: usize = 64;

/// The colour of one cell of a test brush: a round blob of ramp colours, clear outside it.
fn cell(x: usize, y: usize, side: usize, seed: u32) -> u32 {
    let (dx, dy) = (
        2 * x as i64 + 1 - side as i64,
        2 * y as i64 + 1 - side as i64,
    );
    if dx * dx + dy * dy > (side * side) as i64 {
        return 0;
    }
    let band = ((x + y) * 3 / side) as u32;
    ramp(seed + (x * 4 / side) as u32, band)
}

fn test_brush(side: usize) -> Brush {
    let colors: Vec<u32> = (0..side * side)
        .map(|i| cell(i % side, i / side, side, 0))
        .collect();
    Brush::from_parts("bench", side, side, &colors, None, BrushPaint::OwnColors).expect("a brush")
}

/// Event times and allocations of a pencil stroke with a square brush of `side`. The full set
/// of three lines is kept for one size; the other gives its median.
fn stroke_with_brush(side: usize, events: usize, name: &str, full: bool, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(document(CANVAS));
    let mut canvas = Canvas::new(CANVAS);
    canvas.present(&mut editor);
    editor.set_brush(BrushChoice::Custom(test_brush(side).into()));
    let mut points = vec![(0, 0); events];
    walk(&mut Rng(7), CANVAS as i32, &mut points);
    editor.layer = 2;
    let begin = DrawCmd::Begin {
        tool: Tool::Pencil,
        x: points[0].0,
        y: points[0].1,
    };
    editor.apply(begin).expect("begin");
    canvas.present(&mut editor);
    let mut times = Vec::with_capacity(events);
    let allocs_before = total_allocs();
    for &(x, y) in &points[1..] {
        let start = Instant::now();
        editor.apply(DrawCmd::To { x, y }).expect("drag");
        canvas.present(&mut editor);
        times.push(start.elapsed().as_secs_f64() * 1e6);
    }
    let allocs = total_allocs() - allocs_before;
    editor.apply(DrawCmd::Commit).expect("commit");
    times.sort_by(f64::total_cmp);
    let median = percentile(&times, 0.5);
    push(out, &format!("{name}_event_median"), Kind::Micros, median);
    if full {
        let p99 = percentile(&times, 0.99);
        push(out, &format!("{name}_event_p99"), Kind::Micros, p99);
        let per_thousand = allocs as f64 * 1000.0 / times.len() as f64;
        let allocs_name = format!("{name}_allocs_per_1000_events");
        push(out, &allocs_name, Kind::Allocs, per_thousand);
    }
}

/// Making a 256 by 256 brush from a selection of drawn cells.
fn capture_256(out: &mut Vec<Metric>) {
    let mut editor = Editor::new(document(CANVAS));
    editor.layer = 2;
    let shape = Box::new(Mask::rect(100, 100, 256, 256));
    let select = SelectionCmd::Select {
        shape,
        op: SelectOp::Replace,
    };
    editor.apply(select).expect("select");
    let time = best_of(|| {
        editor.capture_brush(BrushPaint::OwnColors).expect("brush");
        black_box(editor.brushes());
    });
    push(out, "brush_capture_256", Kind::Micros, time);
}

fn library_brush(n: usize) -> LibraryBrush {
    let side = 32;
    let rgba = (0..side * side)
        .flat_map(|i| cell(i % side, i / side, side, n as u32).to_le_bytes())
        .collect();
    LibraryBrush {
        name: format!("brush {n}"),
        width: side as u16,
        height: side as u16,
        paint: Paint::OwnColors,
        orientation: (n % 8) as u8,
        handle: Handle::Centre,
        rgba,
    }
}

/// Reading a full library of 32 by 32 brushes from memory, as the first use of the menu does.
fn library_load(out: &mut Vec<Metric>) {
    let brushes: Vec<LibraryBrush> = (0..LIBRARY_BRUSHES).map(library_brush).collect();
    let bytes = write_library(&brushes).expect("a library");
    let time = best_of(|| {
        let library = read_library(black_box(&bytes)).expect("reads");
        assert_eq!(library.brushes.len(), LIBRARY_BRUSHES);
        black_box(library);
    });
    push(out, "brush_library_load_64", Kind::Micros, time);
}

pub fn measure() -> Vec<Metric> {
    let mut out = Vec::new();
    stroke_with_brush(32, EVENTS_32, "brush32", true, &mut out);
    stroke_with_brush(256, EVENTS_256, "brush256", false, &mut out);
    capture_256(&mut out);
    library_load(&mut out);
    crate::creative_palette::measure(&mut out);
    crate::creative_tiles::measure(&mut out);
    crate::creative_terrain::measure(&mut out);
    crate::creative_blend::measure(&mut out);
    out
}
