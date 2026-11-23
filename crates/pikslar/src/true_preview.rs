//! The frame at true size, one screen pixel for each pixel, in a corner of the canvas.
//!
//! It shows the composited frame the canvas already holds. Only while the onion skin is drawn
//! into that frame does it composite its own copy without the skin, and that copy is capped at
//! [`PREVIEW_CAP`] pixels a side, so it costs at most 256 KB and nothing while it is not needed.

use crate::app::App;
use pikslar_render::{blit_true_size, Rect, Surface};
use pikslar_ui::theme;

/// The most document pixels the preview shows each way. A larger sprite shows its middle.
pub(crate) const PREVIEW_CAP: usize = 256;
/// The gap between the preview and the canvas edges, in interface units.
const MARGIN: i32 = 8;

#[derive(Default)]
pub struct TruePreview {
    pub on: bool,
    /// Where it was last drawn, frame included; empty while it does not fit.
    pub rect: Rect,
    /// The frame without onion skin, kept only while the skin is drawn into the canvas.
    pub(crate) plain: Vec<u32>,
}

/// The part of a `w` by `h` sprite shown in a space `room` pixels wide and high: all of it up
/// to the cap, else its middle. As x, y, width and height.
pub(crate) fn shown_part(
    (w, h): (usize, usize),
    room: (usize, usize),
) -> (usize, usize, usize, usize) {
    let pw = w.min(PREVIEW_CAP).min(room.0);
    let ph = h.min(PREVIEW_CAP).min(room.1);
    ((w - pw) / 2, (h - ph) / 2, pw, ph)
}

/// The part shown as a document rectangle.
fn part_rect((x, y, w, h): (usize, usize, usize, usize)) -> Rect {
    Rect::new(x as i32, y as i32, w as i32, h as i32)
}

impl App {
    /// Shows or hides the frame at true size. P.
    pub fn toggle_true_preview(&mut self) {
        let p = &mut self.true_preview;
        p.on = !p.on;
        if !p.on {
            p.plain = Vec::new();
            p.rect = Rect::default();
        }
        // The canvas is drawn again whole, under the preview and over where it was.
        self.full_canvas = true;
        self.ui_dirty = true;
        let said = if self.true_preview.on {
            "Preview at true size"
        } else {
            "Preview hidden"
        };
        let said = self
            .keymap
            .hint(said, crate::shortcuts::Action::TruePreview, "");
        self.notify(said);
    }

    /// Where the document pixels go, and which of them, in the canvas's bottom right corner.
    fn preview_place(&self) -> Option<(Rect, (usize, usize, usize, usize))> {
        let (s, canvas) = (self.ui.scale, self.layout.canvas);
        let room = |side: i32| (side / 2 - 2 * MARGIN * s).max(0) as usize;
        let doc = self.editor.doc();
        let part = shown_part((doc.width, doc.height), (room(canvas.w), room(canvas.h)));
        if part.2 == 0 || part.3 == 0 {
            return None;
        }
        let inner = Rect::new(
            canvas.right() - MARGIN * s - part.2 as i32,
            canvas.bottom() - MARGIN * s - part.3 as i32,
            part.2 as i32,
            part.3 as i32,
        );
        Some((inner, part))
    }

    /// Draws the preview over the canvas when what it shows may have changed or the canvas
    /// was drawn over it. `touched` is what the canvas drew this pass, `changed` whether the
    /// composited frame changed.
    pub(crate) fn draw_true_preview(
        &mut self,
        surface: &mut Surface<'_>,
        (touched, changed): (Rect, bool),
        dirty: &mut Vec<Rect>,
    ) {
        if !self.true_preview.on {
            return;
        }
        // Tiles that changed off screen are in no `changed` list, though the preview shows
        // the middle of the sprite, wherever the view is.
        let deferred = self.comp.take_deferred();
        let place = self.preview_place();
        let framed = place.map_or_else(Rect::default, |(inner, _)| inner.inset(-self.ui.scale));
        let old = std::mem::replace(&mut self.true_preview.rect, framed);
        let moved = framed != old;
        if moved && !old.is_empty() {
            // Where it was shows canvas again.
            self.restore_canvas(surface, old, dirty);
        }
        let Some((inner, part)) = place else {
            return;
        };
        let covered = !touched.intersect(framed).is_empty();
        let stale = !deferred.intersect(part_rect(part)).is_empty();
        if !(moved || covered || stale || (changed && self.shows_change(part))) {
            return;
        }
        surface.fill(framed, theme::edge());
        self.blit_preview(surface, inner, part);
        dirty.push(framed);
    }

    /// Whether the frame changed inside the part shown.
    fn shows_change(&self, part: (usize, usize, usize, usize)) -> bool {
        let shown = part_rect(part);
        self.changed.iter().any(|r| !r.intersect(shown).is_empty())
    }

    /// Names the canvas and the preview in the layout dump, for window scripts and screen sheets.
    pub(crate) fn name_canvas(&mut self) {
        if !self.ui.naming() {
            return;
        }
        self.ui.name_rect(self.layout.canvas, "Canvas");
        // Where it goes this pass: the interface is named before the canvas is drawn.
        if let Some((inner, _)) = self.preview_place().filter(|_| self.true_preview.on) {
            self.ui
                .name_rect(inner.inset(-self.ui.scale), "True-size preview");
        }
    }

    /// True over the canvas where drawing goes: not over the true-size preview.
    pub(crate) fn on_canvas(&self, x: i32, y: i32) -> bool {
        self.layout.canvas.contains(x, y) && !self.true_preview.rect.contains(x, y)
    }

    pub(crate) fn blit_preview(
        &mut self,
        surface: &mut Surface<'_>,
        inner: Rect,
        part: (usize, usize, usize, usize),
    ) {
        let (x0, y0, w, h) = part;
        let at = (inner.x, inner.y);
        if self.comp.ghosting() {
            let mut plain = std::mem::take(&mut self.true_preview.plain);
            self.comp.compose_plain(&self.editor, part, &mut plain);
            blit_true_size(&plain, w, (w, h), surface, at);
            self.true_preview.plain = plain;
            return;
        }
        // The copy is not needed while the canvas holds the frame as it is.
        self.true_preview.plain = Vec::new();
        // The part may be off screen, where the canvas composites only once it shows.
        self.comp.settle(&self.editor, part);
        let stride = self.comp.width();
        let pixels = &self.comp.pixels()[y0 * stride + x0..];
        blit_true_size(pixels, stride, (w, h), surface, at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_sprite_shows_whole_and_a_large_one_its_middle() {
        assert_eq!(shown_part((32, 16), (500, 500)), (0, 0, 32, 16));
        assert_eq!(shown_part((1000, 300), (500, 500)), (372, 22, 256, 256));
        assert_eq!(
            shown_part((64, 64), (40, 100)),
            (12, 0, 40, 64),
            "kept to the room"
        );
        assert_eq!(
            shown_part((64, 64), (0, 100)).2,
            0,
            "no room, nothing shown"
        );
    }
}
