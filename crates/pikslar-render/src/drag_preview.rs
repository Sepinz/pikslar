//! Showing a large moving selection without touching the document's pixels.
//!
//! Compositing costs time in proportion to the area that changes, and dragging a big
//! selection changes all of it. Only what reaches the screen matters, so this samples
//! the document once per on-screen pixel and caches what never changes during a drag:
//! the layers under the floating pixels, and the layers over them. Each frame then
//! costs the same whatever the size of the selection.
//!
//! Layers are folded in the compositor's order with the rules in [`crate::pixel`], so a
//! preview is the same picture the compositor would produce, pixel for pixel. The caches hold
//! the document's blending's own pixels, straight for Aseprite-compatible blending, and only
//! the finished image is made what the screen shows.

use crate::compose::Compositor;
use crate::pixel::{blend_cell_as, covers, ghost_cell, screen_as, Lut};
use crate::view::{blit_framed, document_on_screen, Rect, Surface, View};
use pikslar_core::blend_straight::from_screen;
use pikslar_core::{stretched, Blending, Document, Editor, Ghost, Layer};

mod cel;
pub use cel::CEL_EPOCH;

/// Below this many cells, moving through the compositor is already quick.
pub const PREVIEW_MIN_CELLS: usize = 1 << 18;

/// Nothing above changes this pixel. Never a stored color, those are all opaque.
const PASS_THROUGH: u32 = 0;
/// Layers above are translucent here, so each frame has to blend through them.
const BLEND_ABOVE: u32 = 1;

/// Which document cells the on-screen pixels sample.
#[derive(Clone, Copy)]
struct Sampling {
    x: i32,
    y: i32,
    step: i32,
    cols: usize,
    rows: usize,
}

impl Sampling {
    fn cells(&self) -> usize {
        self.cols * self.rows
    }
}

/// First and last cell a run of screen positions samples, within `0..size`.
fn visible_cells(from: i32, to: i32, size: i32, map: impl Fn(i32) -> i32) -> Option<(i32, i32)> {
    let mut seen = (from..to).map(map).filter(|&c| c >= 0 && c < size);
    let first = seen.next()?;
    Some((first, seen.next_back().unwrap_or(first)))
}

fn sampling(view: View, area: Rect, width: i32, height: i32) -> Option<Sampling> {
    let across = |sx: i32| view.to_doc(sx, 0).0;
    let down = |sy: i32| view.to_doc(0, sy).1;
    let (x0, x1) = visible_cells(area.x, area.right(), width, across)?;
    let (y0, y1) = visible_cells(area.y, area.bottom(), height, down)?;
    Some(Sampling {
        x: x0,
        y: y0,
        step: view.den,
        cols: ((x1 - x0) / view.den + 1) as usize,
        rows: ((y1 - y0) / view.den + 1) as usize,
    })
}

pub struct DragPreview {
    view: View,
    area: Rect,
    /// The whole document on screen, for the edge line around it.
    document: Rect,
    sample: Sampling,
    /// Everything under the floating pixels' layer.
    below: Vec<u32>,
    /// `below` with what the active layer keeps, for where nothing floats.
    base: Vec<u32>,
    /// A finished color, or `PASS_THROUGH`, or `BLEND_ABOVE`.
    above: Vec<u32>,
    image: Vec<u32>,
    /// One row of floating cells, reused so a frame allocates nothing.
    scan: Vec<u32>,
    /// Where each column lands in the lifted image while scaling, or -1.
    columns: Vec<i32>,
    lut: Box<Lut>,
    /// Which layers the screen draws, by position from the bottom.
    shown: Vec<bool>,
    drawn: Option<(u64, Option<Target>)>,
    epoch: u64,
}

/// A size not applied yet: x, y, width and height.
pub type Target = (i32, i32, i32, i32);

/// Blends the previous frame faintly, as the compositor does under the current frame, in the
/// document's blending, Aseprite-compatible when `STRAIGHT`.
fn onion_under<const STRAIGHT: bool>(
    (editor, shown): (&Editor, &[&Layer]),
    comp: &Compositor,
    ghosts: &[Ghost],
    x: usize,
    y: usize,
) -> u32 {
    let mut out = 0;
    for ghost in ghosts {
        let mut flat = 0;
        for layer in shown.iter().copied() {
            let cel = &layer.cel(ghost.frame);
            if !cel.looks_the_same(layer.cel(editor.frame)) {
                let cell = editor
                    .doc()
                    .layer_pixel(layer, ghost.frame, x as i32, y as i32);
                flat = blend_cell_as::<STRAIGHT>(flat, cell, layer, &comp.lut);
            }
        }
        // A ghost is drawn on the screen's picture, whichever blending built it.
        let flat = screen_as::<STRAIGHT>(flat);
        out = ghost_cell(out, flat, ghost.distance, &comp.onion_style);
    }
    if STRAIGHT {
        from_screen(out)
    } else {
        out
    }
}

/// Works out what the layers above do to whatever ends up beneath them.
fn classify_above<const STRAIGHT: bool>(
    (doc, shown): (&Document, &[&Layer]),
    frame: usize,
    (x, y): (usize, usize),
    lut: &Lut,
) -> u32 {
    let mut state = PASS_THROUGH;
    for layer in shown.iter().copied() {
        let cell = doc.layer_pixel(layer, frame, x as i32, y as i32);
        if cell == 0 {
            continue;
        }
        state = if covers(layer, cell, lut) {
            blend_cell_as::<STRAIGHT>(0, cell, layer, lut)
        } else if state > BLEND_ABOVE {
            blend_cell_as::<STRAIGHT>(state, cell, layer, lut)
        } else {
            BLEND_ABOVE
        };
    }
    state
}

/// The layers `shown` takes, which lines up with `layers` from its first.
fn shown_only<'a>(layers: &'a [Layer], shown: &'a [bool]) -> impl Iterator<Item = &'a Layer> {
    layers
        .iter()
        .zip(shown)
        .filter(|(_, s)| **s)
        .map(|(l, _)| l)
}

impl DragPreview {
    /// Prepares a preview of the floating pixels. `None` if there is nothing to show.
    pub fn build(
        comp: &mut Compositor,
        editor: &Editor,
        view: View,
        area: Rect,
    ) -> Option<DragPreview> {
        comp.refresh_lut(editor, false);
        let floating = editor.floating_view()?;
        let (width, height) = (editor.doc().width as i32, editor.doc().height as i32);
        let sample = sampling(view, area, width, height)?;
        let size = (width, height);
        let mut preview = DragPreview::empty((view, area, size), sample, comp, floating.epoch);
        let active = editor.doc().layers()[floating.layer].cel(editor.frame);
        let kept = |x: i32, y: i32| {
            let hole = !floating.copy && floating.source.contains(x, y);
            if hole {
                0
            } else {
                active.get(x, y)
            }
        };
        preview.fill_caches(comp, editor, floating.layer, kept);
        Some(preview)
    }

    /// A preview of `sample` with nothing rendered yet, for a `size` document seen through
    /// `view` in `area`.
    fn empty(
        (view, area, size): (View, Rect, (i32, i32)),
        sample: Sampling,
        comp: &Compositor,
        epoch: u64,
    ) -> Self {
        DragPreview {
            view,
            area,
            document: document_on_screen(view, size.0, size.1),
            sample,
            below: vec![0; sample.cells()],
            base: vec![0; sample.cells()],
            above: vec![0; sample.cells()],
            image: vec![0; sample.cells()],
            scan: vec![0; sample.cols],
            columns: vec![0; sample.cols],
            lut: comp.lut.clone(),
            shown: Vec::new(),
            drawn: None,
            epoch,
        }
    }

    /// Caches what stays still: the layers under `layer`, with the onion skin, what `layer`
    /// keeps where nothing moves over it (`kept`), and the layers above.
    fn fill_caches(
        &mut self,
        comp: &Compositor,
        editor: &Editor,
        layer: usize,
        kept: impl Fn(i32, i32) -> u32,
    ) {
        match editor.doc().blending {
            Blending::Pikslar => self.fill_caches_as::<false>(comp, editor, layer, kept),
            Blending::Aseprite => self.fill_caches_as::<true>(comp, editor, layer, kept),
        }
    }

    /// [`Self::fill_caches`] in one blending, Aseprite-compatible when `STRAIGHT`.
    fn fill_caches_as<const STRAIGHT: bool>(
        &mut self,
        comp: &Compositor,
        editor: &Editor,
        layer: usize,
        kept: impl Fn(i32, i32) -> u32,
    ) {
        let s = self.sample;
        let layers = editor.doc().layers();
        let (under, rest) = layers.split_at(layer);
        let Some((active, over)) = rest.split_first() else {
            return;
        };
        let frame = editor.frame;
        let ghosts = comp.ghosts(editor);
        let shown = editor.doc().screen_layers();
        let (shown_under, shown_rest) = shown.split_at(layer.min(shown.len()));
        let active_shown = shown_rest.first().copied().unwrap_or(false);
        let shown_over = shown_rest.get(1..).unwrap_or_default();
        let all: Vec<&Layer> = shown_only(layers, &shown).collect();
        let under_shown: Vec<&Layer> = shown_only(under, shown_under).collect();
        let over_shown: Vec<&Layer> = shown_only(over, shown_over).collect();
        for row in 0..s.rows {
            let y = s.y + row as i32 * s.step;
            for col in 0..s.cols {
                let x = s.x + col as i32 * s.step;
                let (ux, uy) = (x as usize, y as usize);
                let mut below = onion_under::<STRAIGHT>((editor, &all), comp, &ghosts, ux, uy);
                for layer in under_shown.iter().copied() {
                    let cell = editor.doc().layer_pixel(layer, frame, ux as i32, uy as i32);
                    below = blend_cell_as::<STRAIGHT>(below, cell, layer, &comp.lut);
                }
                let base = if active_shown {
                    blend_cell_as::<STRAIGHT>(below, kept(x, y), active, &comp.lut)
                } else {
                    below
                };
                let index = row * s.cols + col;
                self.below[index] = below;
                self.base[index] = base;
                let at = (ux, uy);
                self.above[index] =
                    classify_above::<STRAIGHT>((editor.doc(), &over_shown), frame, at, &comp.lut);
            }
        }
        self.shown = shown;
    }

    /// Which floating selection this preview was built for.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// True if this preview shows the same floating pixels in the same place on screen.
    pub fn is_for(&self, epoch: u64, view: View, area: Rect) -> bool {
        self.epoch == epoch && self.view == view && self.area == area
    }

    fn is_drawn(&mut self, editor: &Editor, target: Option<Target>) -> bool {
        let state = (editor.selection_version(), target);
        if self.drawn == Some(state) {
            return true;
        }
        self.drawn = Some(state);
        false
    }

    /// Draws the floating pixels where they are now. Returns false if they have not moved.
    pub fn render(&mut self, editor: &Editor) -> bool {
        if self.is_drawn(editor, None) {
            return false;
        }
        let Some(floating) = editor.floating_view() else {
            return false;
        };
        let s = self.sample;
        let stride = floating.w as usize;
        self.compose(editor, floating.layer, |y, scan| {
            let local = y - floating.y;
            if local < 0 || local >= floating.h {
                return;
            }
            let cells = &floating.placed[local as usize * stride..][..stride];
            for (col, out) in scan.iter_mut().enumerate() {
                let local = s.x + col as i32 * s.step - floating.x;
                if local >= 0 && local < floating.w {
                    *out = cells[local as usize];
                }
            }
        });
        true
    }

    /// Draws the floating pixels stretched over `target`, without computing them all.
    pub fn render_scaled(&mut self, editor: &Editor, target: Target) -> bool {
        if self.is_drawn(editor, Some(target)) {
            return false;
        }
        let Some(floating) = editor.floating_view() else {
            return false;
        };
        let s = self.sample;
        let (x, y, w, h) = target;
        let (image_w, image_h) = floating.oriented_size();
        let mut columns = std::mem::take(&mut self.columns);
        for (col, out) in columns.iter_mut().enumerate() {
            *out = stretched(s.x + col as i32 * s.step, x, w, image_w);
        }
        self.compose(editor, floating.layer, |row_y, scan| {
            let row = stretched(row_y, y, h, image_h);
            if row < 0 {
                return;
            }
            for (out, &column) in scan.iter_mut().zip(&columns) {
                if column >= 0 {
                    *out = floating.oriented_cell(column, row);
                }
            }
        });
        self.columns = columns;
        true
    }

    /// Folds a frame. `fill` writes each row's floating cells into a zeroed scan line.
    fn compose(&mut self, editor: &Editor, layer: usize, fill: impl FnMut(i32, &mut [u32])) {
        match editor.doc().blending {
            Blending::Pikslar => self.compose_as::<false>(editor, layer, fill),
            Blending::Aseprite => self.compose_as::<true>(editor, layer, fill),
        }
    }

    /// [`Self::compose`] in one blending, Aseprite-compatible when `STRAIGHT`.
    fn compose_as<const STRAIGHT: bool>(
        &mut self,
        editor: &Editor,
        layer: usize,
        mut fill: impl FnMut(i32, &mut [u32]),
    ) {
        let s = self.sample;
        let layers = editor.doc().layers();
        let active = &layers[layer];
        let over = &layers[layer + 1..];
        let shown = std::mem::take(&mut self.shown);
        let active_shown = shown.get(layer).copied().unwrap_or(false);
        let shown_over = shown.get(layer + 1..).unwrap_or_default();
        let mut scan = std::mem::take(&mut self.scan);
        for row in 0..s.rows {
            let y = s.y + row as i32 * s.step;
            scan.fill(0);
            if active_shown {
                fill(y, &mut scan);
            }
            for (col, &cell) in scan.iter().enumerate() {
                let index = row * s.cols + col;
                let cover = self.above[index];
                if cover > BLEND_ABOVE {
                    self.image[index] = screen_as::<STRAIGHT>(cover);
                    continue;
                }
                let x = s.x + col as i32 * s.step;
                let mut out = if cell != 0 {
                    blend_cell_as::<STRAIGHT>(self.below[index], cell, active, &self.lut)
                } else {
                    self.base[index]
                };
                if cover == BLEND_ABOVE {
                    let (ux, uy) = (x as usize, y as usize);
                    for layer in shown_only(over, shown_over) {
                        let cell =
                            editor
                                .doc()
                                .layer_pixel(layer, editor.frame, ux as i32, uy as i32);
                        out = blend_cell_as::<STRAIGHT>(out, cell, layer, &self.lut);
                    }
                }
                self.image[index] = screen_as::<STRAIGHT>(out);
            }
        }
        self.scan = scan;
        self.shown = shown;
    }

    /// Copies the preview to the screen, with the same backdrop as the canvas.
    pub fn blit(&self, target: &mut Surface<'_>, xmap: &mut Vec<i32>) {
        let s = self.sample;
        let placed = View {
            num: self.view.num,
            den: 1,
            x: (s.x * self.view.num).div_euclid(self.view.den) + self.view.x,
            y: (s.y * self.view.num).div_euclid(self.view.den) + self.view.y,
        };
        blit_framed(
            &self.image,
            (s.cols as i32, s.rows as i32),
            placed,
            target,
            (self.area, self.document),
            xmap,
        );
    }
}

#[cfg(test)]
#[path = "drag_preview_tests.rs"]
mod tests;
