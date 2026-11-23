//! Pixels lifted off a layer: moving, flipping, rotating, scaling, copying and pasting.
//!
//! A floating selection is a stroke that stays open. Its result lives in the overlay, so
//! the compositor shows it with no extra code, palette and RGBA layers behave identically,
//! and anchoring goes through the same commit as any other edit: one undo step.
//!
//! The lifted pixels are kept untouched and every transform is recomputed from them, so
//! scaling up and back down, or rotating four times, never degrades the art.

use crate::color::ColorTable;
use crate::document::{ColorMode, LimitError, MAX_DIMENSION};
use crate::editor::Editor;
use crate::history::{SavedSelection, Snapshot};
use crate::mask::Mask;
use std::sync::Arc;

/// A flip then some clockwise quarter turns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Orientation {
    flip: bool,
    turns: u8,
}

/// How many transforms of floating pixels can be taken back one at a time.
const MAX_FLOATING_STEPS: usize = 64;

/// One position of the floating pixels. All of it is shared, so keeping one costs almost nothing.
struct FloatingStep {
    selection: Arc<Mask>,
    placed: Arc<Vec<u32>>,
    orientation: Orientation,
    changed: bool,
}

impl FloatingStep {
    fn is_same_place(&self, other: &FloatingStep) -> bool {
        self.orientation == other.orientation
            && self.selection == other.selection
            && self.placed == other.placed
    }
}

#[derive(Default)]
pub(crate) struct FloatingSteps {
    undo: Vec<FloatingStep>,
    redo: Vec<FloatingStep>,
    /// Where a drag or similar began, so it can end as one step back or none.
    gesture: Option<FloatingStep>,
}

/// Floating pixels written into the document ahead of a change that may yet fail.
pub(crate) struct HeldFloating {
    floating: Floating,
    selection: Option<Arc<Mask>>,
    colors: Arc<ColorTable>,
}

impl HeldFloating {
    /// False for pixels picked up and left alone, which make no step when put back.
    pub(crate) fn wrote_pixels(&self) -> bool {
        self.floating.changed
    }
}

pub(crate) struct Floating {
    pub(crate) steps: FloatingSteps,
    /// The selection before this began, restored by undo or cancel.
    before: Option<Arc<Mask>>,
    /// The cells the pixels came from, cleared unless this is a copy.
    source: Arc<Mask>,
    /// The original pixels: row by row over `source`'s bounds, 0 outside its shape.
    pixels: Arc<Vec<u32>>,
    copy: bool,
    pasted: bool,
    changed: bool,
    orientation: Orientation,
    /// The result of the transform, row by row over the selection's bounds.
    placed: Arc<Vec<u32>>,
    /// The overlay no longer matches this. Rendering waits until something looks at it.
    stale: bool,
    /// The color table before a paste added to it, and the table the paste produced.
    paste_colors: Option<(Arc<ColorTable>, Arc<ColorTable>)>,
}

impl FloatingSteps {
    /// A new stop means whatever was undone can no longer be redone.
    fn push(&mut self, step: FloatingStep) {
        self.redo.clear();
        if self.undo.len() == MAX_FLOATING_STEPS {
            self.undo.remove(0);
        }
        self.undo.push(step);
    }
}

/// What Copy puts on the clipboard: self-describing, so it survives leaving the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub x: i32,
    pub y: i32,
    pub width: usize,
    pub height: usize,
    /// Straight-alpha colors, row by row, 0 where nothing was selected.
    pub colors: Vec<u32>,
    /// The palette IDs the colors came from, which keep a paste linked to its swatches.
    pub(crate) ids: Option<Vec<u32>>,
}

impl Clip {
    /// Whether the size and the lists of pixels agree, since the fields can be set from outside.
    pub(crate) fn is_well_formed(&self) -> bool {
        if self.width > MAX_DIMENSION || self.height > MAX_DIMENSION {
            return false;
        }
        let cells = self.width * self.height;
        let ids_agree = self.ids.as_ref().is_none_or(|ids| ids.len() == cells);
        self.colors.len() == cells && ids_agree
    }

    /// Wraps an image from another program.
    pub fn from_image(width: usize, height: usize, colors: Vec<u32>) -> Clip {
        Clip {
            x: 0,
            y: 0,
            width,
            height,
            colors,
            ids: None,
        }
    }
}

/// What a preview needs to show floating pixels without the overlay.
pub struct FloatingView<'a> {
    /// Different for every lift and paste, so a preview cannot outlive its pixels.
    pub epoch: u64,
    pub layer: usize,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Row by row over the area above, 0 where nothing lands.
    pub placed: &'a [u32],
    /// The cells that show as holes, unless this is a copy.
    pub source: &'a Mask,
    pub copy: bool,
    /// The lifted image, for sampling it at a size not yet applied.
    pixels: &'a [u32],
    orientation: Orientation,
}

impl FloatingView<'_> {
    /// The size of the lifted image after flips and turns.
    pub fn oriented_size(&self) -> (i32, i32) {
        let (_, _, sw, sh) = self.source.bounds();
        if self.orientation.turns % 2 == 1 {
            (sh, sw)
        } else {
            (sw, sh)
        }
    }

    /// One cell of the oriented image, 0 where nothing was lifted.
    #[inline]
    pub fn oriented_cell(&self, x: i32, y: i32) -> u32 {
        let (sx, sy, sw, _) = self.source.bounds();
        let (cx, cy) = source_cell(x, y, self.oriented_size(), self.orientation, sw);
        if self.source.contains(sx + cx, sy + cy) {
            self.pixels[(cy * sw + cx) as usize]
        } else {
            0
        }
    }
}

/// Maps a cell of the oriented image back to the original.
fn source_cell(x: i32, y: i32, size: (i32, i32), o: Orientation, source_w: i32) -> (i32, i32) {
    let (mut x, mut y, mut w, mut h) = (x, y, size.0, size.1);
    for _ in 0..o.turns {
        (x, y) = (y, w - 1 - x);
        std::mem::swap(&mut w, &mut h);
    }
    (if o.flip { source_w - 1 - x } else { x }, y)
}

/// Where a position lands in an image of `size` stretched over `span` from `origin`, or -1
/// outside it. Scaling and the preview of scaling both sample through this.
#[inline]
pub fn stretched(position: i32, origin: i32, span: i32, size: i32) -> i32 {
    let local = position - origin;
    if local < 0 || local >= span {
        return -1;
    }
    (local as i64 * size as i64 / span as i64) as i32
}

/// Resamples the lifted pixels into a `w` by `h` image at the given position.
fn transform(f: &Floating, x: i32, y: i32, w: i32, h: i32) -> (Mask, Vec<u32>) {
    let (sx, sy, sw, sh) = f.source.bounds();
    let oriented = if f.orientation.turns % 2 == 1 {
        (sh, sw)
    } else {
        (sw, sh)
    };
    let mut mask = Mask::blank(x, y, w, h);
    let mut placed = vec![0u32; w as usize * h as usize];
    for dy in 0..h {
        for dx in 0..w {
            let ox = stretched(dx, 0, w, oriented.0);
            let oy = stretched(dy, 0, h, oriented.1);
            let (cx, cy) = source_cell(ox, oy, oriented, f.orientation, sw);
            if f.source.contains(sx + cx, sy + cy) {
                mask.set(x + dx, y + dy);
                placed[(dy * w + dx) as usize] = f.pixels[(cy * sw + cx) as usize];
            }
        }
    }
    (mask, placed)
}

impl Editor {
    pub fn is_floating(&self) -> bool {
        self.floating.is_some()
    }

    /// True if the floating pixels came from Paste rather than from this layer.
    pub fn is_pasting(&self) -> bool {
        self.floating.as_ref().is_some_and(|f| f.pasted)
    }

    fn read_selected(&self, shape: &Mask) -> Vec<u32> {
        let (x0, y0, w, h) = shape.bounds();
        let mut cel = self.cel().reader();
        let mut cells = vec![0u32; w as usize * h as usize];
        shape.for_each(|x, y| {
            cells[((y - y0) * w + (x - x0)) as usize] = cel.get(x, y);
        });
        cells
    }

    /// Lifts the selected pixels so they can be transformed. `copy` leaves the originals.
    ///
    /// Returns false if nothing is selected. Does nothing if pixels are already lifted.
    /// What putting the floating pixels back down could cost.
    pub(crate) fn floating_bytes(&self) -> usize {
        self.floating.as_ref().map_or(0, |f| {
            (f.placed.len() * 4).saturating_add(self.growth_bytes())
        })
    }

    pub(crate) fn lift(&mut self, copy: bool) -> bool {
        if self.floating.is_some() {
            return true;
        }
        self.cancel();
        let Some(source) = self.selection.clone() else {
            return false;
        };
        let before = (!self.selection_temporary).then(|| source.clone());
        self.selection_temporary = false;
        self.end_join();
        self.floating_epoch += 1;
        let pixels = Arc::new(self.read_selected(&source));
        self.floating = Some(Floating {
            steps: FloatingSteps::default(),
            before,
            placed: pixels.clone(),
            source,
            pixels,
            copy,
            pasted: false,
            changed: false,
            orientation: Orientation::default(),
            paste_colors: None,
            stale: true,
        });
        true
    }

    /// True while the floating pixels have moved on from what the overlay holds.
    pub fn floating_is_stale(&self) -> bool {
        self.floating.as_ref().is_some_and(|f| f.stale)
    }

    /// The floating pixels as they are now, whether or not the overlay has caught up.
    pub fn floating_view(&self) -> Option<FloatingView<'_>> {
        let (f, shape) = (self.floating.as_ref()?, self.selection.as_deref()?);
        let (x, y, w, h) = shape.bounds();
        Some(FloatingView {
            epoch: self.floating_epoch,
            layer: self.layer,
            x,
            y,
            w,
            h,
            placed: &f.placed,
            source: &f.source,
            copy: f.copy,
            pixels: &f.pixels,
            orientation: f.orientation,
        })
    }

    /// Brings the overlay up to date with the floating pixels, if it has fallen behind.
    pub fn settle_floating(&mut self) {
        if !self.floating_is_stale() {
            return;
        }
        self.reset_preview();
        let (Some(mut f), Some(shape)) = (self.floating.take(), self.selection.clone()) else {
            return;
        };
        f.stale = false;
        if !f.copy {
            f.source.for_each(|x, y| self.plot_unclipped(x, y, 0));
        }
        let (x0, y0, w, _) = shape.bounds();
        shape.for_each(|x, y| {
            let value = f.placed[((y - y0) * w + (x - x0)) as usize];
            if value != 0 {
                self.plot_unclipped(x, y, value);
            }
        });
        self.floating = Some(f);
    }

    fn floating_step(&self) -> Option<FloatingStep> {
        let f = self.floating.as_ref()?;
        Some(FloatingStep {
            selection: self.selection.clone()?,
            placed: f.placed.clone(),
            orientation: f.orientation,
            changed: f.changed,
        })
    }

    /// Makes the present position of the floating pixels a step that undo can come back to.
    pub(crate) fn mark_floating(&mut self) {
        let step = self.floating_step();
        if let (Some(f), Some(step)) = (&mut self.floating, step) {
            f.steps.push(step);
        }
    }

    /// Remembers where the pixels are, for the end of a drag to compare with.
    pub(crate) fn gesture_begin(&mut self) {
        let step = self.floating_step();
        if let Some(f) = &mut self.floating {
            f.steps.gesture = step;
        }
    }

    /// Ends a drag. A drag that went nowhere leaves things as though it never happened.
    pub(crate) fn gesture_end(&mut self) {
        let current = self.floating_step();
        let Some(f) = &mut self.floating else {
            return;
        };
        let (Some(start), Some(current)) = (f.steps.gesture.take(), current) else {
            return;
        };
        if start.is_same_place(&current) {
            f.changed = start.changed;
        } else if start.changed {
            f.steps.push(start);
        }
    }

    /// Puts the pixels back where the drag found them.
    pub(crate) fn gesture_cancel(&mut self) {
        let current = self.floating_step();
        let Some(f) = &mut self.floating else {
            return;
        };
        let Some(start) = f.steps.gesture.take() else {
            return;
        };
        if current.is_some_and(|c| c.is_same_place(&start)) {
            f.changed = start.changed;
        } else {
            self.show_step(start);
        }
    }

    fn show_step(&mut self, step: FloatingStep) {
        if let Some(f) = &mut self.floating {
            f.placed = step.placed;
            f.orientation = step.orientation;
            f.changed = step.changed;
            f.stale = true;
            self.selection = Some(step.selection);
            self.selection_version += 1;
        }
    }

    /// Goes one step back or forward. Returns false if there is no such step.
    pub(crate) fn step_floating(&mut self, back: bool) -> bool {
        let Some(current) = self.floating_step() else {
            return false;
        };
        let Some(f) = &mut self.floating else {
            return false;
        };
        f.steps.gesture = None;
        let (from, to) = if back {
            (&mut f.steps.undo, &mut f.steps.redo)
        } else {
            (&mut f.steps.redo, &mut f.steps.undo)
        };
        let Some(step) = from.pop() else {
            return false;
        };
        to.push(current);
        self.show_step(step);
        true
    }

    fn after_transform(&mut self) {
        if let Some(f) = &mut self.floating {
            f.changed = true;
            f.stale = true;
        }
        self.selection_version += 1;
    }

    /// Moves the floating pixels by a distance.
    pub(crate) fn move_floating(&mut self, dx: i32, dy: i32) {
        if self.floating.is_none() || (dx == 0 && dy == 0) {
            return;
        }
        if let Some(selection) = &mut self.selection {
            Arc::make_mut(selection).translate(dx, dy);
        }
        self.selection_moved += 1;
        self.after_transform();
    }

    /// Moves the pixels. Unless this joins the last move, it can be taken back alone.
    pub(crate) fn nudge_floating(&mut self, dx: i32, dy: i32, join: bool) {
        if (dx, dy) == (0, 0) {
            return;
        }
        if !join && self.floating.as_ref().is_some_and(|f| f.changed) {
            self.mark_floating();
        }
        self.move_floating(dx, dy);
    }

    /// The size a scale to `w` by `h` would really produce.
    pub fn clamped_scale(&self, w: i32, h: i32) -> (i32, i32) {
        let Some(f) = &self.floating else {
            return (w.max(1), h.max(1));
        };
        // Stretching far past the canvas shows nothing more and costs memory on every move.
        let (_, _, source_w, source_h) = f.source.bounds();
        let canvas = self.doc.width.max(self.doc.height) as i32;
        let limit = (canvas * 2)
            .max(source_w)
            .max(source_h)
            .min(MAX_DIMENSION as i32);
        (w.clamp(1, limit), h.clamp(1, limit))
    }

    /// Stretches the floating pixels to fill a rectangle, using nearest-neighbor sampling
    /// so no new colors appear on palette or RGBA layers.
    pub(crate) fn scale_floating(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let (w, h) = self.clamped_scale(w, h);
        let Some(f) = &self.floating else {
            return;
        };
        let (mask, placed) = transform(f, x, y, w, h);
        self.replace_placed(mask, placed);
    }

    fn replace_placed(&mut self, mask: Mask, placed: Vec<u32>) {
        if let Some(f) = &mut self.floating {
            f.placed = Arc::new(placed);
        }
        self.selection = Some(Arc::new(mask));
        self.after_transform();
    }

    fn reorient(&mut self, change: impl FnOnce(Orientation) -> Orientation, swap: bool) {
        let worth_a_stop = self.floating.as_ref().is_some_and(|f| f.changed);
        if !self.lift(false) {
            return;
        }
        if worth_a_stop {
            self.mark_floating();
        }
        let Some(shape) = self.selection.clone() else {
            return;
        };
        let (mut x, mut y, mut w, mut h) = shape.bounds();
        if swap {
            (x, y) = (x + (w - h) / 2, y + (h - w) / 2);
            std::mem::swap(&mut w, &mut h);
        }
        let Some(f) = &mut self.floating else {
            return;
        };
        f.orientation = change(f.orientation);
        let (mask, placed) = transform(f, x, y, w, h);
        self.replace_placed(mask, placed);
    }

    pub(crate) fn flip_horizontal(&mut self) {
        self.reorient(
            |o| Orientation {
                flip: !o.flip,
                turns: (4 - o.turns) % 4,
            },
            false,
        );
    }

    pub(crate) fn flip_vertical(&mut self) {
        self.reorient(
            |o| Orientation {
                flip: !o.flip,
                turns: (6 - o.turns) % 4,
            },
            false,
        );
    }

    pub(crate) fn rotate(&mut self, clockwise: bool) {
        let quarter = if clockwise { 1 } else { 3 };
        self.reorient(
            |o| Orientation {
                flip: o.flip,
                turns: (o.turns + quarter) % 4,
            },
            true,
        );
    }

    /// Writes the floating pixels into the layer as one undo step.
    pub(crate) fn anchor(&mut self) {
        if self.floating.as_ref().is_some_and(|f| f.changed) {
            self.settle_floating();
        }
        let Some(f) = self.floating.take() else {
            return;
        };
        if !f.changed {
            self.put_back_unchanged(&f);
            return;
        }
        let saved: SavedSelection = Some(f.before);
        let colors = f
            .paste_colors
            .filter(|(_, after)| Arc::ptr_eq(&self.doc.colors, after))
            .map(|(before, _)| before);
        // Trimmed first, so pixels moved off the canvas and back again are not a step.
        let (w, h) = (self.doc.width as i32, self.doc.height as i32);
        let on_canvas = self.selection.take().map(|s| s.within(w, h).trimmed());
        self.set_selection(on_canvas);
        self.commit_overlay(saved, colors);
    }

    /// Nothing moved. A temporary pick-up leaves no selection behind either.
    fn put_back_unchanged(&mut self, floating: &Floating) {
        self.reset_preview();
        if floating.before.is_none() && !floating.pasted {
            self.set_selection(None);
        }
    }

    /// Writes floating pixels into the document as part of the change about to happen, so
    /// the two are one step. Returns what is needed to take that back if the change fails.
    pub(crate) fn absorb_floating(&mut self, snapshot: &mut Snapshot) -> Option<HeldFloating> {
        if !self.floating.as_ref()?.changed {
            let (selection, colors) = (self.selection.clone(), self.doc.colors.clone());
            let floating = self.floating.take()?;
            self.put_back_unchanged(&floating);
            return Some(HeldFloating {
                floating,
                selection,
                colors,
            });
        }
        self.settle_floating();
        let floating = self.floating.take()?;
        let colors_before = floating
            .paste_colors
            .as_ref()
            .filter(|(_, after)| Arc::ptr_eq(&self.doc.colors, after))
            .map(|(before, _)| before.clone());
        snapshot.include_floating(floating.before.clone(), colors_before);
        let selection = self.selection.clone();
        let colors = self.doc.colors.clone();
        drop(self.write_overlay());
        let (w, h) = (self.doc.width as i32, self.doc.height as i32);
        self.set_selection(selection.as_ref().map(|s| s.within(w, h).trimmed()));
        Some(HeldFloating {
            floating,
            selection,
            colors,
        })
    }

    /// Puts floating pixels back after the change they were part of failed.
    pub(crate) fn give_back_floating(&mut self, held: Option<HeldFloating>) {
        let Some(held) = held else {
            return;
        };
        let mut floating = held.floating;
        floating.stale = true;
        self.floating = Some(floating);
        self.selection = held.selection;
        self.selection_version += 1;
        self.doc.colors = held.colors;
        self.damage_all();
    }

    /// Abandons the floating pixels, leaving the layer and selection as they were.
    pub(crate) fn revert_floating(&mut self) {
        if let Some(f) = self.floating.take() {
            self.reset_preview();
            if let Some((before, after)) = f.paste_colors {
                // Anything that edited the palette meanwhile made a new table: keep that.
                if Arc::ptr_eq(&self.doc.colors, &after) {
                    self.doc.colors = before;
                }
            }
            self.selection = f.before;
            self.selection_version += 1;
        }
    }

    /// Deletes the selected pixels as one undo step.
    pub(crate) fn delete_selected(&mut self) {
        if self.is_pasting() {
            self.revert_floating();
            return;
        }
        if let Some(f) = &mut self.floating {
            f.placed = Arc::new(vec![0; f.placed.len()]);
            f.changed = true;
            f.stale = true;
            self.anchor();
            return;
        }
        self.cancel();
        let Some(shape) = self.selection.clone() else {
            return;
        };
        shape.for_each(|x, y| self.plot_unclipped(x, y, 0));
        self.commit_overlay(None, None);
    }

    /// The width and height of the selection's bounds, without reading any pixels.
    pub fn selection_size(&self) -> Option<(usize, usize)> {
        let (_, _, w, h) = self.selection.as_deref()?.bounds();
        Some((w.max(0) as usize, h.max(0) as usize))
    }

    /// Captures the selected pixels. Returns `None` when nothing is selected.
    pub fn copy_selected(&self) -> Option<Clip> {
        let shape = self.selection.as_deref()?;
        let (x, y, w, h) = shape.bounds();
        // A map is copied as the pixels its tiles draw; it never has floating pixels.
        let cells = match &self.floating {
            Some(f) => f.placed.to_vec(),
            None if self.on_tilemap() => self.read_selected_map(shape),
            None => self.read_selected(shape),
        };
        let palette = self.mode() == ColorMode::Palette;
        let colors = if palette {
            cells.iter().map(|&c| self.doc.colors.color(c)).collect()
        } else {
            cells.clone()
        };
        Some(Clip {
            x,
            y,
            width: w as usize,
            height: h as usize,
            colors,
            ids: palette.then_some(cells),
        })
    }

    /// Converts clipboard colors into cell values for the active layer, and returns the
    /// color table those values need. The document's own table is left alone, so a paste
    /// that does not fit changes nothing.
    fn clip_cells(&self, clip: &Clip) -> Result<(Vec<u32>, Arc<ColorTable>), LimitError> {
        self.clip_cells_in(clip, self.doc.colors.clone())
    }

    /// As [`Self::clip_cells`], adding to the given table rather than the document's own.
    fn clip_cells_in(
        &self,
        clip: &Clip,
        mut colors: Arc<ColorTable>,
    ) -> Result<(Vec<u32>, Arc<ColorTable>), LimitError> {
        if self.mode() == ColorMode::Rgba {
            return Ok((clip.colors.clone(), colors));
        }
        let mut cells = Vec::with_capacity(clip.colors.len());
        for (i, &color) in clip.colors.iter().enumerate() {
            if color == 0 {
                cells.push(0);
                continue;
            }
            let original = clip.ids.as_ref().map(|ids| ids[i]);
            let cell = match original.filter(|&id| colors.color(id) == color) {
                Some(id) => id,
                None => match colors.id_for_existing(color) {
                    Some(id) => id,
                    None => Arc::make_mut(&mut colors).id_for(color)?,
                },
            };
            cells.push(cell);
        }
        Ok((cells, colors))
    }

    /// Places clipboard pixels at a position as a new floating selection.
    pub(crate) fn paste(&mut self, clip: &Clip, x: i32, y: i32) -> Result<(), LimitError> {
        if !clip.is_well_formed() {
            return Err(LimitError::Dimensions);
        }
        let table = self.doc.colors.clone();
        let converted = self.clip_cells(clip)?;
        self.anchor();
        self.cancel();
        self.end_join();
        let before = self.doc.colors.clone();
        // Done first so a refusal leaves floating pixels alone, and reused unless they moved it.
        let (cells, colors) = if Arc::ptr_eq(&table, &before) {
            converted
        } else {
            self.clip_cells(clip)?
        };
        let paste_colors = (!Arc::ptr_eq(&before, &colors)).then(|| (before, colors.clone()));
        self.doc.colors = colors;
        let shape = Arc::new(clip_shape(clip, x, y));
        let cells = Arc::new(cells);
        self.floating_epoch += 1;
        self.floating = Some(Floating {
            steps: FloatingSteps::default(),
            before: self.selection.take(),
            source: shape.clone(),
            placed: cells.clone(),
            pixels: cells,
            copy: true,
            pasted: true,
            changed: true,
            orientation: Orientation::default(),
            paste_colors,
            stale: true,
        });
        self.selection = Some(shape);
        self.selection_version += 1;
        Ok(())
    }
}

/// The cells a clip covers when its top-left is at (x, y), in an area the size of the clip.
fn clip_shape(clip: &Clip, x: i32, y: i32) -> Mask {
    let (w, h) = (clip.width as i32, clip.height as i32);
    let mut shape = Mask::blank(x, y, w, h);
    for (i, &color) in clip.colors.iter().enumerate() {
        if color != 0 {
            shape.set(x + i as i32 % w, y + i as i32 / w);
        }
    }
    shape
}

#[path = "floating_replace.rs"]
mod replace;
#[path = "floating_state.rs"]
mod state;
pub use state::FloatingSnapshot;

#[cfg(test)]
#[path = "floating_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "floating_replace_tests.rs"]
mod replace_tests;
