//! Selection gestures, transform handles and the clipboard.

use crate::app::{App, Gesture, ToolChoice};
use pikslar_core::{Clip, DrawCmd, FloatingCmd, Mask, SelectOp, SelectionCmd};
use pikslar_platform::{clipboard_stamp, copy_image, paste_image, Cursor, Mods, Window};
use pikslar_render::{screen_rect, Rect, PREVIEW_MIN_CELLS};

/// How close the pointer must be to grab a handle, in interface units.
const HANDLE_REACH: i32 = 6;
pub const HANDLE_SIZE: i32 = 7;

/// Which sides a handle moves: -1 left or top, 1 right or bottom, 0 neither.
pub type Handle = (i32, i32);

pub const HANDLES: [Handle; 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// The screen position of a handle on a rectangle.
pub fn handle_point(r: Rect, handle: Handle) -> (i32, i32) {
    (
        r.x + r.w * (handle.0 + 1) / 2,
        r.y + r.h * (handle.1 + 1) / 2,
    )
}

/// The rectangle after dragging a handle to a document position, never smaller than a pixel.
pub fn resized(
    bounds: (i32, i32, i32, i32),
    handle: Handle,
    x: i32,
    y: i32,
) -> (i32, i32, i32, i32) {
    let (bx, by, bw, bh) = bounds;
    let (mut left, mut top, mut right, mut bottom) = (bx, by, bx + bw, by + bh);
    match handle.0 {
        -1 => left = x.min(right - 1),
        1 => right = (x + 1).max(left + 1),
        _ => {}
    }
    match handle.1 {
        -1 => top = y.min(bottom - 1),
        1 => bottom = (y + 1).max(top + 1),
        _ => {}
    }
    (left, top, right - left, bottom - top)
}

/// Shift adds and Alt removes; with neither, the context bar's mode.
fn select_op(mods: Mods, base: SelectOp) -> SelectOp {
    match (mods.shift, mods.alt) {
        (true, _) => SelectOp::Add,
        (false, true) => SelectOp::Subtract,
        (false, false) => base,
    }
}

impl App {
    /// The selection's bounds on screen.
    pub fn selection_on_screen(&self) -> Option<Rect> {
        let (x, y, w, h) = self.shown_bounds()?;
        Some(screen_rect(&self.view, x, y, w, h))
    }

    /// Where the selection appears to be, including a scale not applied yet.
    pub fn shown_bounds(&self) -> Option<(i32, i32, i32, i32)> {
        self.scale_target
            .or_else(|| Some(self.editor.selection()?.bounds()))
    }

    /// Places a scale the drag had been previewing.
    pub fn apply_scale_target(&mut self) {
        if let Some((x, y, w, h)) = self.scale_target.take() {
            self.run_on_canvas(FloatingCmd::Scale { x, y, w, h });
        }
    }

    pub(crate) fn deselect(&mut self) {
        self.run(SelectionCmd::None);
    }

    fn select_shape(&mut self, shape: Mask, op: SelectOp) {
        let shape = Box::new(shape);
        self.run(SelectionCmd::Select { shape, op });
    }

    /// Lifts the selected pixels, returning false if there are none.
    fn lift_pixels(&mut self, copy: bool) -> bool {
        self.run(FloatingCmd::Lift { copy })
            .is_some_and(|o| o.changed)
    }

    /// Large selections wait for the release, so a scale costs what the screen costs.
    fn scale_to(&mut self, bounds: (i32, i32, i32, i32), (x, y, w, h): (i32, i32, i32, i32)) {
        let (w, h) = self.editor.clamped_scale(w, h);
        let cells = |w: i32, h: i32| w as usize * h as usize;
        if cells(bounds.2, bounds.3).max(cells(w, h)) >= PREVIEW_MIN_CELLS {
            if self.scale_target != Some((x, y, w, h)) {
                self.scale_target = Some((x, y, w, h));
                self.ui_dirty = true;
            }
            return;
        }
        self.scale_target = None;
        self.run_on_canvas(FloatingCmd::Scale { x, y, w, h });
    }

    /// Where handles appear. Too small a selection has none, so it can still be dragged.
    pub fn handles_rect(&self) -> Option<Rect> {
        if self.tool != ToolChoice::Move {
            return None;
        }
        let smallest = 3 * HANDLE_SIZE * self.ui.scale;
        self.selection_on_screen()
            .filter(|r| r.w.min(r.h) >= smallest)
    }

    fn handle_at(&self, sx: i32, sy: i32) -> Option<Handle> {
        let reach = HANDLE_REACH * self.ui.scale;
        let r = self.handles_rect()?;
        HANDLES.iter().copied().find(|&handle| {
            let (hx, hy) = handle_point(r, handle);
            (sx - hx).abs() <= reach && (sy - hy).abs() <= reach
        })
    }

    pub fn selection_down(&mut self, sx: i32, sy: i32, mods: Mods) {
        let (x, y) = self.view.to_doc(sx, sy);
        if let Some(handle) = self.handle_at(sx, sy) {
            self.begin_scale(handle);
            return;
        }
        // The bar's mode is for the selection tools; Move starts from Replace.
        let base = match self.tool {
            ToolChoice::Move => SelectOp::Replace,
            _ => self.bar.select,
        };
        let op = select_op(mods, base);
        let inside = self.editor.selection().is_some_and(|s| s.contains(x, y));
        if inside && op == SelectOp::Replace {
            self.gesture = Gesture::Grab {
                x,
                y,
                copy: mods.ctrl,
                auto: false,
            };
            return;
        }
        match self.tool {
            ToolChoice::Marquee => {
                self.gesture = Gesture::Marquee { start: (x, y), op };
                self.drag_point = (x, y);
            }
            ToolChoice::Lasso => {
                self.lasso.clear();
                self.lasso.push((x, y));
                self.gesture = Gesture::Lasso { op };
            }
            ToolChoice::Wand => self.apply_wand(x, y, op, self.wand_contiguous(mods.ctrl)),
            ToolChoice::Move => self.move_tool_miss(x, y, mods),
            ToolChoice::Paint(_) | ToolChoice::Text => {}
        }
        self.full_canvas = true;
    }

    fn apply_wand(&mut self, x: i32, y: i32, op: SelectOp, contiguous: bool) {
        match self.editor.wand_shape(x, y, contiguous) {
            Some(shape) => self.select_shape(shape, op),
            None if op == SelectOp::Replace => self.deselect(),
            None => {}
        }
        self.ui_dirty = true;
    }

    /// The Move tool outside any selection: apply, or move the whole cel. Ctrl picks up a
    /// floating copy of everything on the canvas instead.
    fn move_tool_miss(&mut self, x: i32, y: i32, mods: Mods) {
        if self.editor.selection().is_some() {
            self.deselect();
            return;
        }
        if !mods.ctrl {
            self.begin_cel_drag(x, y, mods.alt != self.bar.only_cel);
            return;
        }
        let shape = self.editor.opaque_shape();
        if !shape.is_empty() {
            let shape = Box::new(shape);
            self.run(SelectionCmd::Temporary { shape });
            self.gesture = Gesture::Grab {
                x,
                y,
                copy: mods.ctrl,
                auto: true,
            };
        }
    }

    /// A press inside the selection that never moved is an ordinary click for the tool.
    fn click_in_selection(&mut self, x: i32, y: i32, ctrl: bool) {
        match self.tool {
            ToolChoice::Marquee | ToolChoice::Lasso => self.deselect(),
            ToolChoice::Wand => {
                self.apply_wand(x, y, SelectOp::Replace, self.wand_contiguous(ctrl))
            }
            ToolChoice::Move | ToolChoice::Paint(_) | ToolChoice::Text => {}
        }
    }

    fn begin_scale(&mut self, handle: Handle) {
        let lifted = !self.editor.is_floating();
        if !self.lift_pixels(false) {
            return;
        }
        self.run(FloatingCmd::GestureBegin);
        if let Some(selection) = self.editor.selection() {
            self.gesture = Gesture::Scale {
                handle,
                bounds: selection.bounds(),
                lifted,
            };
        }
    }

    pub fn selection_move(&mut self, sx: i32, sy: i32) {
        let (x, y) = self.view.to_doc(sx, sy);
        match self.gesture {
            Gesture::Marquee { .. } => {
                self.drag_point = (x, y);
                self.full_canvas = true;
            }
            Gesture::Lasso { .. } => {
                if self.lasso.last() != Some(&(x, y)) {
                    self.lasso.push((x, y));
                    self.full_canvas = true;
                }
            }
            Gesture::Grab {
                x: lx, y: ly, copy, ..
            } => {
                let lifted = !self.editor.is_floating();
                if (x, y) != (lx, ly) && self.lift_pixels(copy) {
                    self.run(FloatingCmd::GestureBegin);
                    self.run_on_canvas(FloatingCmd::Move {
                        dx: x - lx,
                        dy: y - ly,
                        join: true,
                    });
                    self.gesture = Gesture::Move { x, y, lifted };
                }
            }
            Gesture::Move {
                x: lx,
                y: ly,
                lifted,
            } => {
                self.run_on_canvas(FloatingCmd::Move {
                    dx: x - lx,
                    dy: y - ly,
                    join: true,
                });
                self.gesture = Gesture::Move { x, y, lifted };
            }
            Gesture::Scale { handle, bounds, .. } => {
                self.scale_to(bounds, resized(bounds, handle, x, y));
            }
            _ => {}
        }
    }

    /// Turns the finished drag into a selection.
    pub fn finish_selecting(&mut self) {
        let shape = match self.gesture {
            Gesture::Grab { auto: true, .. } => {
                self.deselect();
                None
            }
            Gesture::Grab { x, y, copy, .. } => {
                self.click_in_selection(x, y, copy);
                None
            }
            Gesture::Marquee { start, op } if start == self.drag_point => {
                if op == SelectOp::Replace {
                    self.deselect();
                }
                None
            }
            Gesture::Marquee { start, op } => {
                Some((self.editor.rect_shape(start, self.drag_point), op))
            }
            Gesture::Lasso { op } if self.lasso.len() >= 3 => {
                Some((self.editor.lasso_shape(&self.lasso), op))
            }
            Gesture::Lasso {
                op: SelectOp::Replace,
            } => {
                self.deselect();
                None
            }
            _ => None,
        };
        if let Some((shape, op)) = shape {
            self.select_shape(shape, op);
        }
        self.lasso.clear();
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    /// The rectangle being dragged out, in outline form.
    pub fn draft_outline(&self) -> Option<Vec<pikslar_core::Edge>> {
        match self.gesture {
            Gesture::Marquee { start, .. } => {
                Some(self.editor.rect_shape(start, self.drag_point).outline())
            }
            _ => None,
        }
    }

    /// Which cursor the selection tools want at a screen position.
    pub fn selection_cursor(&self, sx: i32, sy: i32) -> Cursor {
        if let Some(handle) = self.handle_at(sx, sy) {
            return match handle {
                (0, _) => Cursor::SizeNs,
                (_, 0) => Cursor::SizeWe,
                (-1, -1) | (1, 1) => Cursor::SizeNwse,
                _ => Cursor::SizeNesw,
            };
        }
        let (x, y) = self.view.to_doc(sx, sy);
        let moves_cel = self.tool == ToolChoice::Move && self.editor.selection().is_none();
        if moves_cel || self.editor.selection().is_some_and(|s| s.contains(x, y)) {
            Cursor::Move
        } else {
            Cursor::Cross
        }
    }

    /// The document position at the middle of what is on screen.
    pub(crate) fn view_center(&self) -> (i32, i32) {
        let c = self.layout.canvas;
        self.view.to_doc(c.x + c.w / 2, c.y + c.h / 2)
    }

    pub fn copy(&mut self, window: Window) -> bool {
        self.finish_gesture();
        let Some(clip) = self.editor.copy_selected() else {
            self.notify("Select something to copy".to_string());
            return false;
        };
        if !copy_image(window, clip.width, clip.height, &clip.colors) {
            self.notify("Copied inside Pikslar only: the clipboard was busy".to_string());
        }
        self.clip = Some(clip);
        self.clip_stamp = clipboard_stamp();
        true
    }

    pub fn cut(&mut self, window: Window) {
        if self.copy(window) {
            self.run(DrawCmd::DeleteSelected);
        }
    }

    /// Prefers our own copy, which keeps palette links, unless something newer was copied.
    fn clipboard_contents(&self, window: Window) -> Option<(Clip, bool)> {
        if let Some(clip) = self
            .clip
            .as_ref()
            .filter(|_| self.clip_stamp == clipboard_stamp())
        {
            return Some((clip.clone(), true));
        }
        let image = paste_image(window)?;
        Some((Clip::from_image(image.width, image.height, image.px), false))
    }

    pub fn paste(&mut self, window: Window) {
        self.finish_gesture();
        self.playing = false;
        let Some((clip, ours)) = self.clipboard_contents(window) else {
            self.notify("There is no image to paste".to_string());
            return;
        };
        let (x, y) = self.paste_position(&clip, ours);
        let clip = std::sync::Arc::new(clip);
        if self.run(FloatingCmd::Paste { clip, x, y }).is_some() {
            self.tool = ToolChoice::Move;
        }
    }

    /// Our own pixels return to where they came from if that is in view.
    fn paste_position(&self, clip: &Clip, ours: bool) -> (i32, i32) {
        let c = self.layout.canvas;
        let origin = screen_rect(&self.view, clip.x, clip.y, 1, 1);
        if ours && c.contains(origin.x, origin.y) {
            return (clip.x, clip.y);
        }
        let (cx, cy) = self.view_center();
        (cx - clip.width as i32 / 2, cy - clip.height as i32 / 2)
    }

    /// Moves the selection. A held key is one step of undo, however far it goes.
    pub fn nudge(&mut self, dx: i32, dy: i32, repeat: bool) {
        if self.lift_pixels(false) {
            self.run(FloatingCmd::Move {
                dx,
                dy,
                join: repeat,
            });
        }
    }

    pub fn has_selection(&self) -> bool {
        self.editor
            .selection()
            .is_some_and(|s: &std::sync::Arc<Mask>| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_during_a_drag_undoes_only_that_drag() {
        let mut app = App::launch(None, Some(32), None);
        app.resized(800, 600, 1);
        app.editor
            .apply(SelectionCmd::Select {
                shape: Box::new(Mask::rect(2, 2, 4, 4)),
                op: SelectOp::Replace,
            })
            .expect("select");
        app.editor
            .apply(FloatingCmd::Lift { copy: false })
            .expect("lift");
        app.editor
            .apply(FloatingCmd::Move {
                dx: 3,
                dy: 0,
                join: true,
            })
            .expect("move");
        app.gesture = Gesture::Grab {
            x: 5,
            y: 2,
            copy: false,
            auto: false,
        };
        let cell = app.view.num / app.view.den;
        app.selection_move(
            app.view.x + 8 * cell + cell / 2,
            app.view.y + 2 * cell + cell / 2,
        );
        assert_eq!(app.editor.selection().map(|m| m.bounds().0), Some(8));
        app.cancel_gesture();
        assert!(app.editor.is_floating());
        assert_eq!(app.editor.selection().map(|m| m.bounds().0), Some(5));
    }

    #[test]
    fn corner_handles_move_two_sides_and_edge_handles_one() {
        assert_eq!(resized((10, 10, 4, 4), (1, 1), 19, 17), (10, 10, 10, 8));
        assert_eq!(resized((10, 10, 4, 4), (-1, -1), 8, 9), (8, 9, 6, 5));
        assert_eq!(resized((10, 10, 4, 4), (1, 0), 11, 90), (10, 10, 2, 4));
    }

    #[test]
    fn a_handle_cannot_turn_the_rectangle_inside_out() {
        assert_eq!(resized((10, 10, 4, 4), (1, 1), 0, 0), (10, 10, 1, 1));
        assert_eq!(resized((10, 10, 4, 4), (-1, -1), 99, 99), (13, 13, 1, 1));
    }

    #[test]
    fn handles_sit_on_corners_and_midpoints() {
        let r = Rect::new(10, 20, 40, 80);
        assert_eq!(handle_point(r, (-1, -1)), (10, 20));
        assert_eq!(handle_point(r, (0, 1)), (30, 100));
        assert_eq!(handle_point(r, (1, 0)), (50, 60));
    }
}
