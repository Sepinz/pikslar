//! Moving and turning a whole cel from the canvas, and showing where its drawing reaches past the
//! canvas edge.
//!
//! The Move tool dragged with nothing selected moves the active cel, and so do the arrow keys.
//! Flip and rotate with nothing selected turn the whole drawing. Pixels past the edge are kept by
//! both, and a dotted outline shows how far the drawing reaches beyond the canvas.

use crate::app::{App, Gesture};
use pikslar_core::{CelCmd, Edge, StepCmd, Turn};
use pikslar_render::{draw_ants, DragPreview, Rect, Surface, CEL_EPOCH, PREVIEW_MIN_CELLS};

const AT_THE_LIMIT: &str = "The drawing is as far from the canvas as it can go";

/// The extent of the active cel's drawing, worked out again only when the cel changes.
#[derive(Default)]
pub struct OffCanvas {
    /// The revision, layer and frame the extent was found for.
    key: Option<(u64, usize, usize)>,
    image: usize,
    /// In the image's own coordinates, so a move does not need it found again.
    local: Option<(i32, i32, i32, i32)>,
    /// The drawing's extent on the canvas grid, when part of it lies past the edge.
    pub shown: Option<Rect>,
}

/// The four sides of a rectangle, as an outline.
fn sides(r: Rect) -> [Edge; 4] {
    let side = |x, y, len, horizontal| Edge {
        x,
        y,
        len,
        horizontal,
    };
    [
        side(r.x, r.y, r.w, true),
        side(r.x, r.bottom(), r.w, true),
        side(r.x, r.y, r.h, false),
        side(r.right(), r.y, r.h, false),
    ]
}

impl App {
    fn active_cel(&self) -> Option<&pikslar_core::Cel> {
        let layer = self.editor.doc().layers().get(self.editor.layer)?;
        Some(layer.cel(self.editor.frame))
    }

    fn active_position(&self) -> (i32, i32) {
        self.active_cel().map_or((0, 0), |cel| {
            let (x, y) = cel.position();
            (x as i32, y as i32)
        })
    }

    /// Moves the active cel and its hold, or only the cel when `alone` is set. Says so when
    /// the offset range stops it short.
    fn move_active_cel(&mut self, dx: i32, dy: i32, alone: bool) {
        let Some(layer) = self.editor.layer_id(self.editor.layer) else {
            return;
        };
        let frame = self.editor.frame;
        let before = self.active_position();
        let step = match alone {
            true => CelCmd::MoveOne {
                layer,
                frame,
                dx,
                dy,
            },
            false => CelCmd::Move {
                layer,
                frame,
                dx,
                dy,
            },
        };
        if let Err(e) = self.editor.apply(step) {
            self.notify(e.to_string());
            return;
        }
        let after = self.active_position();
        if (after.0 - before.0, after.1 - before.1) != (dx, dy) {
            self.notify(AT_THE_LIMIT.to_string());
        }
    }

    /// The Move tool pressed with nothing selected: starts moving the active cel, alone with
    /// Alt. A blank cel has nothing to move.
    pub(crate) fn begin_cel_drag(&mut self, x: i32, y: i32, alone: bool) {
        if self.active_cel().is_none_or(|cel| cel.is_blank()) {
            return;
        }
        // Earlier arrow moves are their own step.
        self.run(StepCmd::EndDrag);
        self.gesture = Gesture::Cel {
            x,
            y,
            from: self.active_position(),
            alone,
        };
    }

    /// Follows the pointer from where the drag began, so a move stopped at the range limit
    /// does not drift from the pointer.
    pub(crate) fn cel_drag_to(&mut self, sx: i32, sy: i32) {
        let Gesture::Cel { x, y, from, alone } = self.gesture else {
            return;
        };
        let (px, py) = self.view.to_doc(sx, sy);
        let now = self.active_position();
        let (dx, dy) = (from.0 + px - x - now.0, from.1 + py - y - now.1);
        if (dx, dy) != (0, 0) {
            self.move_active_cel(dx, dy, alone);
        }
    }

    /// The arrow keys with the Move tool and nothing selected, moving only this cel with Alt.
    /// A held key is one step. A blank cel stays where it is, as it does for a drag.
    pub(crate) fn nudge_cel(&mut self, (dx, dy): (i32, i32), alone: bool, repeat: bool) {
        if self.active_cel().is_none_or(|cel| cel.is_blank()) {
            if !repeat {
                self.notify("This frame has nothing on the layer to move".to_string());
            }
            return;
        }
        if !repeat {
            self.run(StepCmd::EndDrag);
        }
        self.move_active_cel(dx, dy, alone);
        self.ui_dirty = true;
    }

    /// Flips or rotates the whole drawing of the active frame, pixels past the edge included.
    pub(crate) fn turn_cel(&mut self, turn: Turn) {
        let Some(layer) = self.editor.layer_id(self.editor.layer) else {
            return;
        };
        let frame = self.editor.frame;
        if self.active_cel().is_none_or(|cel| cel.is_blank()) {
            self.notify("This frame has nothing on the layer to turn".to_string());
            return;
        }
        self.finish_gesture();
        self.run(CelCmd::Turn { layer, frame, turn });
    }

    /// Finds the drawing's extent again if the cel changed, and asks for the whole canvas to be
    /// drawn when the outline moves, since it can lie outside the canvas.
    pub(crate) fn refresh_off_canvas(&mut self) {
        let (layer, frame) = (self.editor.layer, self.editor.frame);
        let key = Some((self.editor.revision(), layer, frame));
        let Some(cel) = self.active_cel() else {
            return;
        };
        let (image, (cx, cy)) = (cel.image_id(), cel.position());
        let last = self.off_canvas.key;
        if last != key {
            // A drag only moves the cel, so the drawing itself has not changed.
            let same_cel = last.is_some_and(|(_, l, f)| (l, f) == (layer, frame));
            let moving = matches!(self.gesture, Gesture::Cel { .. });
            if !(moving && same_cel && image == self.off_canvas.image) {
                self.off_canvas.local = cel.image().drawn_bounds();
            }
            self.off_canvas.image = image;
            self.off_canvas.key = key;
        }
        let (w, h) = (
            self.editor.doc().width as i32,
            self.editor.doc().height as i32,
        );
        let shown = self.off_canvas.local.and_then(|(x, y, bw, bh)| {
            let r = Rect::new(x + cx as i32, y + cy as i32, bw, bh);
            let inside = r.x >= 0 && r.y >= 0 && r.right() <= w && r.bottom() <= h;
            (!inside).then_some(r)
        });
        if shown != self.off_canvas.shown {
            self.off_canvas.shown = shown;
            self.full_canvas = true;
        }
    }

    pub(crate) fn draw_off_canvas(&self, surface: &mut Surface<'_>, area: Rect) {
        if let Some(r) = self.off_canvas.shown {
            draw_ants(surface, &self.view, area, &sides(r), 0);
        }
    }

    /// Shows a large cel being dragged without compositing it. `None` to composite instead.
    pub(crate) fn draw_cel_preview(
        &mut self,
        surface: &mut Surface<'_>,
        area: Rect,
    ) -> Option<Rect> {
        let cells = self.editor.doc().width * self.editor.doc().height;
        if cells < PREVIEW_MIN_CELLS {
            self.end_preview();
            return None;
        }
        if self
            .preview
            .as_ref()
            .is_some_and(|p| !p.is_for(CEL_EPOCH, self.view, area))
        {
            self.end_preview();
        }
        if self.preview.is_none() {
            self.preview = DragPreview::build_cel(&mut self.comp, &self.editor, self.view, area);
        }
        let preview = self.preview.as_mut()?;
        let moved = preview.render_cel(&self.editor);
        let everything = std::mem::take(&mut self.full_canvas);
        if !moved && !everything {
            return Some(Rect::default());
        }
        preview.blit(surface, &mut self.xmap);
        Some(area)
    }
}
