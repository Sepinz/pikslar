//! The prepared stamp: a brush resolved for one layer and one stroke.

use super::brush::{Brush, BrushPaint, Handle};
use super::resolve::resolve_cells;
use crate::color::ColorTable;
use crate::document::{ColorMode, LimitError};
use crate::mask::Mask;
use crate::orient::Orientation;
use std::sync::Arc;

/// One row run of a stamp: `values` are written from (`dx`, `dy`) away from the stroke point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StampRun<'a> {
    pub dx: i32,
    pub dy: i32,
    pub values: &'a [u32],
}

/// Cell values, turned, as row runs relative to the stroke point. Built once at the start of a
/// stroke; reading it allocates nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    runs: Vec<(i16, i16, u16)>,
    values: Vec<u32>,
    w: u16,
    h: u16,
    left: i16,
    top: i16,
}

/// A stamp, and the colour table it needs when that is not the document's own.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub stamp: Stamp,
    /// Present only when the brush brought colours the layer's table lacks. The document's
    /// table is untouched until the caller has checked there is room and chooses to use this.
    pub colors: Option<Arc<ColorTable>>,
}

impl Stamp {
    /// Row runs, top to bottom, left to right, without allocating.
    pub fn runs(&self) -> impl Iterator<Item = StampRun<'_>> {
        let mut at = 0usize;
        self.runs.iter().map(move |&(dx, dy, len)| {
            let values = &self.values[at..at + usize::from(len)];
            at += usize::from(len);
            StampRun {
                dx: dx.into(),
                dy: dy.into(),
                values,
            }
        })
    }

    /// Calls `put(dx, dy, values)` for each stretch of cells this stamp covers that the same
    /// stamp moved by `step` does not cover. Nothing is allocated.
    ///
    /// Laying a stamp at every point of a path, a cell keeps the value of the last point that
    /// covers it. A point therefore only has to write the cells the next point does not cover:
    /// the last cover always writes, and nothing writes after it.
    pub fn for_each_uncovered_by(&self, step: (i32, i32), mut put: impl FnMut(i32, i32, &[u32])) {
        let mut at = 0usize;
        for &(dx, dy, len) in &self.runs {
            let (dx, dy, len) = (i32::from(dx), i32::from(dy), usize::from(len));
            let values = &self.values[at..at + len];
            at += len;
            let end = dx.saturating_add(len as i32);
            let moved = self.row(dy.saturating_sub(step.1));
            let shifted = |&(rx, _, rl): &(i16, i16, u16)| {
                let start = i32::from(rx).saturating_add(step.0);
                (start, start.saturating_add(i32::from(rl)))
            };
            let mut x = dx;
            for run in &moved[moved.partition_point(|r| shifted(r).1 <= x)..] {
                let (from, to) = shifted(run);
                if from >= end {
                    break;
                }
                if from > x {
                    put(x, dy, &values[(x - dx) as usize..(from - dx) as usize]);
                }
                x = x.max(to);
                if x >= end {
                    break;
                }
            }
            if x < end {
                put(x, dy, &values[(x - dx) as usize..]);
            }
        }
    }

    /// The runs of one row, left to right.
    fn row(&self, dy: i32) -> &[(i16, i16, u16)] {
        let lo = self.runs.partition_point(|r| i32::from(r.1) < dy);
        let hi = self.runs.partition_point(|r| i32::from(r.1) <= dy);
        &self.runs[lo..hi]
    }

    /// Width and height of the turned stamp.
    pub fn size(&self) -> (i32, i32) {
        (self.w.into(), self.h.into())
    }

    /// Where the stamp's top left cell sits relative to the stroke point.
    pub fn offset(&self) -> (i32, i32) {
        (self.left.into(), self.top.into())
    }

    /// How many cells it writes.
    pub fn cell_count(&self) -> usize {
        self.values.len()
    }

    /// Heap bytes held, for the stroke's memory estimate.
    pub fn heap_bytes(&self) -> usize {
        self.runs.capacity() * size_of::<(i16, i16, u16)>()
            + self.values.capacity() * size_of::<u32>()
    }

    /// The cells it covers as a mask around a stroke point at (0, 0), for the outline. Allocates.
    pub fn mask(&self) -> Mask {
        let (left, top) = self.offset();
        let (w, h) = self.size();
        let mut mask = Mask::blank(left, top, w, h);
        for run in self.runs() {
            for i in 0..run.values.len() as i32 {
                mask.set(run.dx + i, run.dy);
            }
        }
        mask
    }

    /// Lays out a `w` by `h` source, turned by `o`. `cell(x, y)` gives the value at a source
    /// position, or `None` where the stamp does not reach.
    pub(super) fn build(
        (w, h): (usize, usize),
        o: Orientation,
        handle: Handle,
        cell: impl Fn(usize, usize) -> Option<u32>,
    ) -> Stamp {
        let (ow, oh) = o.turned_size(w, h);
        let back = o.inverse();
        let (hx, hy) = handle_point(handle, o, (w, h));
        let (left, top) = (-(hx as i32), -(hy as i32));
        let (mut runs, mut values) = (Vec::new(), Vec::new());
        for y in 0..oh {
            let mut start: Option<usize> = None;
            let mut from = values.len();
            for x in 0..ow {
                let (sx, sy) = back.apply(x, y, ow, oh);
                match cell(sx, sy) {
                    Some(v) => {
                        if start.is_none() {
                            (start, from) = (Some(x), values.len());
                        }
                        values.push(v);
                    }
                    None => close(&mut runs, &mut start, left, top, y, values.len() - from),
                }
            }
            close(&mut runs, &mut start, left, top, y, values.len() - from);
        }
        Stamp {
            runs,
            values,
            w: ow as u16,
            h: oh as u16,
            left: left as i16,
            top: top as i16,
        }
    }
}

fn close(
    runs: &mut Vec<(i16, i16, u16)>,
    start: &mut Option<usize>,
    left: i32,
    top: i32,
    y: usize,
    len: usize,
) {
    if let Some(x) = start.take() {
        runs.push((
            (left + x as i32) as i16,
            (top + y as i32) as i16,
            len as u16,
        ));
    }
}

/// The handle's cell within the turned picture.
fn handle_point(handle: Handle, o: Orientation, (w, h): (usize, usize)) -> (usize, usize) {
    match handle {
        Handle::Centre => {
            let (ow, oh) = o.turned_size(w, h);
            ((ow - 1) / 2, (oh - 1) / 2)
        }
        Handle::Corner(n) => {
            let cx = if n & 1 != 0 { w - 1 } else { 0 };
            let cy = if n & 2 != 0 { h - 1 } else { 0 };
            o.apply(cx, cy, w, h)
        }
    }
}

impl Brush {
    /// Makes the stamp for a layer of `mode` whose colours are `table`. `fill` is the cell value
    /// a shape brush paints (the current colour's, or 0 to erase). A palette layer may need
    /// colours added; they go into a copy of the table, so a refusal changes nothing.
    pub fn prepare(
        &self,
        mode: ColorMode,
        table: &Arc<ColorTable>,
        fill: u32,
    ) -> Result<Prepared, LimitError> {
        let size = self.source_size();
        let w = size.0;
        let colors = &self.colors;
        if self.paint == BrushPaint::Shape {
            let stamp = Stamp::build(size, self.orient, self.handle, |x, y| {
                (colors[y * w + x] != 0).then_some(fill)
            });
            return Ok(Prepared {
                stamp,
                colors: None,
            });
        }
        let (cells, grown) = match mode {
            ColorMode::Rgba => (colors.to_vec(), None),
            ColorMode::Palette => resolve_cells(colors, self.ids.as_deref(), table)?,
        };
        let stamp = Stamp::build(size, self.orient, self.handle, |x, y| {
            let i = y * w + x;
            (colors[i] != 0).then_some(cells[i])
        });
        Ok(Prepared {
            stamp,
            colors: grown,
        })
    }
}
