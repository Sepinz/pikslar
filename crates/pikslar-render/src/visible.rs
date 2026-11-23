//! Compositing only what is on screen.
//!
//! A window shows part of a large canvas, or all of it scaled down. Whoever draws the screen
//! can tell the compositor which part of the document is shown ([`Compositor::set_visible`]);
//! a canvas tile that changes off screen is then marked stale instead of composited, and is
//! composited on the first update that finds it shown. Until then [`Compositor::pixels`] holds
//! an older picture there, so anything that reads pixels off screen first calls
//! [`Compositor::settle`] for the part it reads.
//!
//! Without a visible part, which is how a compositor starts, everything is composited at once
//! as before. Every tile holds what a full composite would give it as soon as it is shown or
//! settled: it is composited from the document as it is then, by the same code.

use super::Compositor;
use crate::view::{Rect, View};
use pikslar_core::{Editor, TILE};

/// Canvas tiles from column `.0` and row `.1` up to, not including, column `.2` and row `.3`.
pub(crate) type TileSpan = (usize, usize, usize, usize);

/// The document pixels a view of `area` shows, as a rectangle inside a document of `size`:
/// every pixel the canvas blit reads there.
pub fn shown_region(view: View, area: Rect, size: (usize, usize)) -> Rect {
    if area.is_empty() {
        return Rect::default();
    }
    let (x0, y0) = view.to_doc(area.x, area.y);
    let (x1, y1) = view.to_doc(area.right() - 1, area.bottom() - 1);
    let doc = Rect::new(0, 0, size.0 as i32, size.1 as i32);
    Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1).intersect(doc)
}

impl Compositor {
    /// Says which document pixels are on screen, or `None` for all of them. Tiles that
    /// changed while hidden are composited on the next update that shows them.
    pub fn set_visible(&mut self, region: Option<Rect>) {
        self.shown = region.map(|r| self.tile_span(r));
    }

    /// How many canvas tiles wait to be composited until they show.
    pub fn stale_tiles(&self) -> usize {
        self.stale_count
    }

    /// The canvas tiles a document rectangle touches, inside the canvas.
    fn tile_span(&self, r: Rect) -> TileSpan {
        let r = r.intersect(Rect::new(0, 0, self.width as i32, self.height as i32));
        if r.is_empty() {
            return (0, 0, 0, 0);
        }
        let side = TILE as i32;
        let (x0, y0) = ((r.x / side) as usize, (r.y / side) as usize);
        let (x1, y1) = ((r.right() - 1) / side + 1, (r.bottom() - 1) / side + 1);
        (x0, y0, x1 as usize, y1 as usize)
    }

    pub(super) fn shown_span(&self) -> TileSpan {
        self.shown
            .unwrap_or((0, 0, self.tiles_wide, self.tiles_high))
    }

    #[inline]
    fn shows(&self, index: usize) -> bool {
        let Some((x0, y0, x1, y1)) = self.shown else {
            return true;
        };
        let (tx, ty) = self.tile_position(index);
        (x0..x1).contains(&tx) && (y0..y1).contains(&ty)
    }

    #[inline]
    fn mark_fresh(&mut self, index: usize) {
        if std::mem::take(&mut self.stale[index]) {
            self.stale_count -= 1;
        }
    }

    /// Collects tile `index` to be composited at the end of the update if it is shown, and
    /// says so; otherwise marks it stale.
    pub(super) fn compose_or_defer(&mut self, index: usize) -> bool {
        if self.shows(index) {
            self.pending.push(index as u32);
            self.mark_fresh(index);
            return true;
        }
        if !std::mem::replace(&mut self.stale[index], true) {
            self.stale_count += 1;
        }
        self.deferred = self.deferred.union(self.tile_rect(index));
        false
    }

    /// The document area, as one rectangle around them, of the tiles that changed off screen
    /// since the last call, and so are not in the `changed` list of any update. Whatever shows
    /// pixels off screen, as the true-size preview does, checks its part against it.
    pub fn take_deferred(&mut self) -> Rect {
        std::mem::take(&mut self.deferred)
    }

    /// Collects the stale tiles of `span` to be composited and appends their areas to
    /// `changed`.
    pub(super) fn catch_up(&mut self, (x0, y0, x1, y1): TileSpan, changed: &mut Vec<Rect>) {
        if self.stale_count == 0 {
            return;
        }
        for ty in y0..y1 {
            for tx in x0..x1 {
                let index = ty * self.tiles_wide + tx;
                if self.stale[index] {
                    self.pending.push(index as u32);
                    self.mark_fresh(index);
                    changed.push(self.tile_rect(index));
                }
            }
        }
    }

    /// Brings `region` of [`Compositor::pixels`] (x, y, width and height in document pixels)
    /// up to date, shown or not. Whatever reads pixels off screen calls this first.
    pub fn settle(&mut self, editor: &Editor, (x, y, w, h): (usize, usize, usize, usize)) {
        if self.stale_count == 0 {
            return;
        }
        let span = self.tile_span(Rect::new(x as i32, y as i32, w as i32, h as i32));
        let ghosts = self.ghosts(editor);
        let (doc, now, over) = (editor.doc(), editor.frame, editor.overlay());
        let mut drawn = std::mem::take(&mut self.drawn);
        doc.screen_layers_into(&mut drawn);
        self.refresh_lut(editor, false);
        let mut changed = Vec::new();
        self.catch_up(span, &mut changed);
        self.compose_pending((doc, &drawn), over, (now, &ghosts));
        self.drawn = drawn;
    }
}

#[cfg(test)]
#[path = "visible_tests.rs"]
mod tests;
