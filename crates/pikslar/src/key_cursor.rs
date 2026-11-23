//! The keyboard cursor: painting without a pointer.
//!
//! Its shortcut (C) turns it on and off. While it is on, the arrows move it a pixel, ten with
//! Shift, and Space is the pointer's button at it: a tap applies the current tool there, and
//! holding Space while the arrows move it drags, for strokes, lines, rectangles, selections
//! and moves. It goes through the same press, move and release as the pointer, so every tool
//! behaves as it does under the mouse. Off, the arrows keep their usual meaning.

use crate::app::App;
use pikslar_platform::{Button, Key, Mods};
use pikslar_render::{screen_rect, Rect, Surface};

/// How far Shift with an arrow moves the cursor.
const FAR: i32 = 10;
const RING_OUTER: u32 = 0xff00_0000;
const RING_INNER: u32 = 0xffff_ffff;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyCursor {
    pub on: bool,
    /// The document pixel it is on.
    pub at: (i32, i32),
    /// Space is down: the press went to the canvas and its release has not.
    pub held: bool,
    /// The screen area its ring was last drawn over.
    pub shown: Rect,
}

impl App {
    /// Turns the keyboard cursor on, in the middle of the sprite, or off.
    pub(crate) fn toggle_key_cursor(&mut self) {
        self.key_cursor_release();
        let on = !self.key_cursor.on;
        let (w, h) = self.doc_size();
        self.key_cursor.on = on;
        if on {
            self.key_cursor.at = (w / 2, h / 2);
            self.key_cursor_follow();
            self.notify(
                "Keyboard cursor on: arrows move it, Shift by 10, Space applies the tool, \
                 hold Space to drag"
                    .to_string(),
            );
        } else {
            self.notify("Keyboard cursor off".to_string());
        }
        self.full_canvas = true;
    }

    fn doc_size(&self) -> (i32, i32) {
        let doc = self.editor.doc();
        (doc.width as i32, doc.height as i32)
    }

    /// The screen point at the middle of the cursor's pixel.
    pub(crate) fn key_cursor_point(&self) -> (i32, i32) {
        let (x, y) = self.key_cursor.at;
        let r = screen_rect(&self.view, x, y, 1, 1);
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    /// The keys the cursor takes while it is on: the arrows and Space. Returns whether it took
    /// the key. A widget with keyboard focus keeps its keys.
    pub(crate) fn key_cursor_key(&mut self, key: Key, mods: Mods, repeat: bool) -> bool {
        if !self.key_cursor.on || self.ui.focus.focused().is_some() {
            return false;
        }
        let step = if mods.shift { FAR } else { 1 };
        let by = match key {
            Key::Left => (-step, 0),
            Key::Right => (step, 0),
            Key::Up => (0, -step),
            Key::Down => (0, step),
            Key::Space => {
                if !repeat && !self.key_cursor.held {
                    self.key_cursor_press(mods);
                }
                return true;
            }
            _ => return false,
        };
        self.key_cursor_move(by);
        true
    }

    fn key_cursor_move(&mut self, (dx, dy): (i32, i32)) {
        let (w, h) = self.doc_size();
        let (x, y) = self.key_cursor.at;
        self.key_cursor.at = (
            (x + dx).clamp(0, (w - 1).max(0)),
            (y + dy).clamp(0, (h - 1).max(0)),
        );
        self.key_cursor_follow();
        let (sx, sy) = self.key_cursor_point();
        if self.key_cursor.held {
            self.ui.mouse = (sx, sy);
            self.canvas_move(sx, sy);
        } else {
            // The brush outline follows the cursor as it follows the pointer.
            self.ui.track(sx, sy);
        }
    }

    /// Space pressed: the pointer's press at the cursor, unless the mouse already holds a press
    /// or drag, which Space then leaves alone, its release too.
    fn key_cursor_press(&mut self, mods: Mods) {
        if self.gesture != crate::app::Gesture::None || self.ui.down {
            return;
        }
        let (w, h) = self.doc_size();
        let (x, y) = self.key_cursor.at;
        self.key_cursor.at = (x.clamp(0, (w - 1).max(0)), y.clamp(0, (h - 1).max(0)));
        self.key_cursor_follow();
        let (sx, sy) = self.key_cursor_point();
        self.pointer_down(sx, sy, Button::Left, mods);
        self.key_cursor.held = true;
    }

    /// Space let go: the press ends where the cursor is.
    pub(crate) fn key_cursor_release(&mut self) {
        if std::mem::take(&mut self.key_cursor.held) {
            let (sx, sy) = self.key_cursor_point();
            self.pointer_up(sx, sy, Button::Left);
        }
    }

    /// Scrolls the view so the cursor's pixel is inside the canvas area.
    fn key_cursor_follow(&mut self) {
        let (x, y) = self.key_cursor.at;
        let pixel = screen_rect(&self.view, x, y, 1, 1);
        let area = self.layout.canvas.inset(4 * self.ui.scale.max(1));
        let shift = |lo: i32, hi: i32, from: i32, to: i32| {
            if lo < from {
                from - lo
            } else if hi > to {
                to - hi
            } else {
                0
            }
        };
        let dx = shift(pixel.x, pixel.right(), area.x, area.right());
        let dy = shift(pixel.y, pixel.bottom(), area.y, area.bottom());
        if (dx, dy) != (0, 0) {
            self.view.x += dx;
            self.view.y += dy;
            self.full_canvas = true;
        }
    }

    /// The ring's screen area, empty while the cursor is off.
    fn key_cursor_area(&self) -> Rect {
        if !self.key_cursor.on || self.panel_open() {
            return Rect::default();
        }
        let (x, y) = self.key_cursor.at;
        let t = self.ui.scale.max(1);
        screen_rect(&self.view, x, y, 1, 1)
            .inset(-2 * t)
            .intersect(self.layout.canvas)
    }

    /// Removes the old ring and draws the new one, as the brush outline is. `touched` is what
    /// was just repainted. Returns what this repainted in turn.
    pub(crate) fn draw_key_cursor(
        &mut self,
        surface: &mut Surface<'_>,
        touched: Rect,
        dirty: &mut Vec<Rect>,
    ) -> Rect {
        let wanted = self.key_cursor_area();
        let shown = self.key_cursor.shown;
        let mut painted = Rect::default();
        if wanted != shown && !shown.is_empty() {
            painted = self.restore_canvas(surface, shown, dirty);
        }
        self.key_cursor.shown = wanted;
        let stale = wanted != shown || !wanted.intersect(touched.union(painted)).is_empty();
        if !wanted.is_empty() && stale {
            let (x, y) = self.key_cursor.at;
            let t = self.ui.scale.max(1);
            let pixel = screen_rect(&self.view, x, y, 1, 1);
            ring(surface, pixel.inset(-2 * t), t, RING_OUTER, wanted);
            ring(surface, pixel.inset(-t), t, RING_INNER, wanted);
            dirty.push(wanted);
            painted = painted.union(wanted);
        }
        painted
    }
}

/// A frame `t` thick just inside `r`, clipped to `clip`.
fn ring(surface: &mut Surface<'_>, r: Rect, t: i32, color: u32, clip: Rect) {
    let sides = [
        Rect::new(r.x, r.y, r.w, t),
        Rect::new(r.x, r.bottom() - t, r.w, t),
        Rect::new(r.x, r.y, t, r.h),
        Rect::new(r.right() - t, r.y, t, r.h),
    ];
    for side in sides {
        surface.fill(side.intersect(clip), color);
    }
}

#[cfg(test)]
#[path = "key_cursor_tests.rs"]
mod tests;
