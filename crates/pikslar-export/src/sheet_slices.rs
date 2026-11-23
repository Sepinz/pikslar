//! Slices in a sprite sheet's description.
//!
//! Each slice has one key, at frame 0, as Aseprite writes a slice that does not change. What
//! is described is what the exported picture shows: the bounds are cut to the canvas, and
//! everything is multiplied by the scale.

use crate::json;
use pikslar_core::{Slice, SliceRect};
use std::fmt::Write;

/// A rectangle in the exported picture's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x: i64,
    y: i64,
    w: i64,
    h: i64,
}

/// Where `a` and `b` overlap, if they do. `a` and `b` are `(start, length)` spans.
fn overlap(a: (i64, i64), b: (i64, i64)) -> Option<(i64, i64)> {
    let start = a.0.max(b.0);
    let end = (a.0 + a.1).min(b.0 + b.1);
    (end > start).then_some((start, end - start))
}

fn wide(r: SliceRect) -> Rect {
    Rect {
        x: i64::from(r.x),
        y: i64::from(r.y),
        w: i64::from(r.width),
        h: i64::from(r.height),
    }
}

/// A slice cut to the canvas, still in canvas pixels.
struct Clipped {
    bounds: Rect,
    pivot: Option<(i64, i64)>,
    center: Option<Rect>,
}

/// The slice cut to a canvas of `size`, or `None` if none of it is on the canvas. The pivot and
/// centre stay relative to the top left corner of the cut bounds, so they move by what the cut
/// took off the left and top, and the centre is cut to the bounds as well.
fn clip(slice: &Slice, size: (i64, i64)) -> Option<Clipped> {
    let b = wide(slice.bounds);
    let (x, w) = overlap((b.x, b.w), (0, size.0))?;
    let (y, h) = overlap((b.y, b.h), (0, size.1))?;
    let (dx, dy) = (x - b.x, y - b.y);
    let center = slice.center.map(wide).and_then(|c| {
        let (cx, cw) = overlap((c.x - dx, c.w), (0, w))?;
        let (cy, ch) = overlap((c.y - dy, c.h), (0, h))?;
        Some(Rect {
            x: cx,
            y: cy,
            w: cw,
            h: ch,
        })
    });
    Some(Clipped {
        bounds: Rect { x, y, w, h },
        pivot: slice
            .pivot
            .map(|(px, py)| (i64::from(px) - dx, i64::from(py) - dy)),
        center,
    })
}

fn push_rect(out: &mut String, r: Rect, scale: i64) {
    let _ = write!(
        out,
        "{{\"x\":{},\"y\":{},\"w\":{},\"h\":{}}}",
        r.x * scale,
        r.y * scale,
        r.w * scale,
        r.h * scale
    );
}

fn push_slice(out: &mut String, slice: &Slice, clipped: &Clipped, scale: i64) {
    out.push_str("{\"name\":");
    json::push_string(out, &slice.name);
    let _ = write!(
        out,
        ",\"color\":\"#{:08x}\",\"keys\":[{{\"frame\":0,\"bounds\":",
        slice.color
    );
    push_rect(out, clipped.bounds, scale);
    if let Some(center) = clipped.center {
        out.push_str(",\"center\":");
        push_rect(out, center, scale);
    }
    if let Some((x, y)) = clipped.pivot {
        let _ = write!(
            out,
            ",\"pivot\":{{\"x\":{},\"y\":{}}}",
            x * scale,
            y * scale
        );
    }
    out.push_str("}]}");
}

/// Appends `,"slices":[...]` to the meta of a description, if any slice is left on the canvas
/// of `canvas` pixels, each pixel exported as `scale` by `scale`.
pub(crate) fn push_slices(
    out: &mut String,
    slices: &[Slice],
    canvas: (usize, usize),
    scale: usize,
) {
    let size = (canvas.0 as i64, canvas.1 as i64);
    let scale = scale as i64;
    let mut any = false;
    for (slice, clipped) in slices.iter().filter_map(|s| clip(s, size).map(|c| (s, c))) {
        out.push_str(if any { "," } else { ",\"slices\":[" });
        any = true;
        push_slice(out, slice, &clipped, scale);
    }
    if any {
        out.push(']');
    }
}
