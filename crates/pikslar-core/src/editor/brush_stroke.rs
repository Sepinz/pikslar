//! Strokes with a brush other than the round one: the stamp is made ready once, at `Begin`, and
//! each input event only writes its cells into the overlay.

use super::{Editor, Tool};
use crate::color::ColorTable;
use crate::custom_brush::{BrushChoice, BrushPaint, Prepared, Stamp};
use crate::document::{ColorMode, LimitError};
use std::sync::Arc;

/// Bytes a stroke may need for each cell of the brush: the prepared stamp, the cells resolved
/// for the layer, and the extra overlay tiles a wide stamp reaches.
const BYTES_PER_BRUSH_CELL: usize = 12;

/// A table to go back to if the stroke comes to nothing.
type TableBefore = Option<Arc<ColorTable>>;

impl Editor {
    /// True when `tool` paints with a stamp of the chosen brush instead of the round brush.
    /// A preset at size 1 is the single cell the pencil already paints, so it stays the pencil.
    pub(super) fn uses_stamp(&self, tool: Tool) -> bool {
        if !matches!(tool, Tool::Pencil | Tool::Eraser | Tool::Line | Tool::Rect) {
            return false;
        }
        match self.brushes.active() {
            BrushChoice::Round => false,
            BrushChoice::Preset(_) => self.brush_size > 1,
            BrushChoice::Custom(_) => true,
        }
    }

    /// True when the stamp brings its own colours, so the current colour is not painted.
    pub(super) fn stamp_has_own_colors(&self, tool: Tool) -> bool {
        tool != Tool::Eraser
            && self.uses_stamp(tool)
            && self
                .brushes
                .active_custom()
                .is_some_and(|b| b.paint() == BrushPaint::OwnColors)
    }

    /// Refuses a stroke whose colours would not fit, before anything has happened. Nothing is
    /// kept: the real stamp is made once the stroke has started.
    pub(super) fn check_stamp_fits(&self, tool: Tool) -> Result<(), LimitError> {
        if self.mode() == ColorMode::Palette && self.stamp_has_own_colors(tool) {
            self.prepare_stamp_for(tool, 0)?;
        }
        Ok(())
    }

    fn prepare_stamp_for(&self, tool: Tool, fill: u32) -> Result<Prepared, LimitError> {
        let (mode, table) = (self.mode(), &self.doc.colors);
        match self.brushes.active() {
            // Erasing takes the brush's shape, whatever colours it holds.
            BrushChoice::Custom(brush) if tool == Tool::Eraser => {
                brush.with_paint(BrushPaint::Shape).prepare(mode, table, 0)
            }
            choice => choice.prepare(self.brush_size, mode, table, fill),
        }
    }

    /// The cells a stroke with `tool` would write around each stroke point, or `None` when the
    /// stroke uses the round brush. Colours do not matter to the cells, so this never fails for
    /// want of room and never touches the table.
    pub fn brush_footprint(&self, tool: Tool) -> Option<Stamp> {
        if !self.uses_stamp(tool) {
            return None;
        }
        let table = &self.doc.colors;
        let prepared = match self.brushes.active() {
            BrushChoice::Custom(brush) => {
                brush
                    .with_paint(BrushPaint::Shape)
                    .prepare(ColorMode::Rgba, table, 1)
            }
            choice => choice.prepare(self.brush_size, ColorMode::Rgba, table, 1),
        };
        prepared.ok().map(|p| p.stamp)
    }

    /// Makes the stamp for a stroke that is starting. Colours the brush brings are put in the
    /// document's table, which `before` (or, if it was not yet kept, the table as it was)
    /// returns to if the stroke is cancelled.
    pub(super) fn begin_stamp(
        &mut self,
        tool: Tool,
        fill: u32,
        before: TableBefore,
    ) -> Result<(Option<Box<Stamp>>, TableBefore), LimitError> {
        if !self.uses_stamp(tool) {
            return Ok((None, before));
        }
        let prepared = match self.prepare_stamp_for(tool, fill) {
            Ok(prepared) => prepared,
            Err(e) => {
                if let Some(table) = before {
                    self.doc.colors = table;
                }
                return Err(e);
            }
        };
        let Some(grown) = prepared.colors else {
            return Ok((Some(Box::new(prepared.stamp)), before));
        };
        let before = before.or_else(|| Some(self.doc.colors.clone()));
        self.doc.colors = grown;
        Ok((Some(Box::new(prepared.stamp)), before))
    }

    /// Writes a stamp's cells around a stroke point.
    pub(super) fn stamp_cells(&mut self, stamp: &Stamp, x: i32, y: i32) {
        for run in stamp.runs() {
            let (left, row) = (x.saturating_add(run.dx), y.saturating_add(run.dy));
            for (i, &value) in run.values.iter().enumerate() {
                self.plot_unmarked(left.saturating_add(i as i32), row, value);
            }
        }
        let ((left, top), (w, h)) = (stamp.offset(), stamp.size());
        self.mark_area(x.saturating_add(left), y.saturating_add(top), w, h);
    }

    /// Continues a path of stamp points with `point`. The point before it only writes the cells
    /// `point` does not cover (see [`Stamp::for_each_uncovered_by`]); call [`Self::stamp_path_end`]
    /// after the last point. The result is the same as writing the whole stamp at every point.
    pub(super) fn stamp_path_point(
        &mut self,
        prev: &mut Option<(i32, i32)>,
        stamp: &Stamp,
        point: (i32, i32),
    ) {
        let Some(from) = prev.replace(point) else {
            return;
        };
        let step = (
            point.0.saturating_sub(from.0),
            point.1.saturating_sub(from.1),
        );
        stamp.for_each_uncovered_by(step, |dx, dy, values| {
            let (left, row) = (from.0.saturating_add(dx), from.1.saturating_add(dy));
            for (i, &value) in values.iter().enumerate() {
                self.plot(left.saturating_add(i as i32), row, value);
            }
        });
    }

    /// Writes the last point of a path in full.
    pub(super) fn stamp_path_end(&mut self, prev: Option<(i32, i32)>, stamp: &Stamp) {
        if let Some((x, y)) = prev {
            self.stamp_cells(stamp, x, y);
        }
    }

    /// What a stroke with the chosen brush needs beyond one with a single pixel.
    pub(crate) fn brush_stroke_bytes(&self, tool: Tool) -> usize {
        if !self.uses_stamp(tool) {
            return 0;
        }
        let area = match self.brushes.active() {
            BrushChoice::Custom(brush) => {
                let (w, h) = brush.size();
                w.saturating_mul(h)
            }
            _ => {
                let n = self.brush_size.clamp(1, 256) as usize;
                n * n
            }
        };
        area.saturating_mul(BYTES_PER_BRUSH_CELL)
    }
}
