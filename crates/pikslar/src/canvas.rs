//! Pointer gestures on the canvas, and drawing the canvas to the screen.

use crate::app::{App, Gesture, ToolChoice};
use crate::panel_timeline::ID_READOUT;
use crate::select_ui::{handle_point, HANDLES, HANDLE_SIZE};
use pikslar_core::{scale_edges, DrawCmd, FloatingCmd, StepCmd, Tool};
use pikslar_platform::{Button, Cursor, Mods, Window};
use pikslar_render::{
    blit_canvas, draw_ants, draw_frame, draw_path, shown_region, DragPreview, Rect, Surface, View,
    PREVIEW_MIN_CELLS,
};
use pikslar_ui::theme;
use std::time::{Duration, Instant};

const ANT_STEP: Duration = Duration::from_millis(110);
const HANDLE_EDGE: u32 = 0xff00_0000;
const HANDLE_FILL: u32 = 0xffff_ffff;

impl App {
    pub fn canvas_down(&mut self, x: i32, y: i32, button: Button, mods: Mods) {
        if button == Button::Middle || self.space_held {
            self.gesture = Gesture::Pan { x, y };
            return;
        }
        self.playing = false;
        if self.tool == ToolChoice::Text {
            if button == Button::Left {
                self.text_down(x, y);
            }
            return;
        }
        let ToolChoice::Paint(chosen) = self.tool else {
            if button == Button::Left {
                self.selection_down(x, y, mods);
            }
            return;
        };
        if self.tile_canvas_down((x, y), chosen, button, mods) {
            return;
        }
        let tool = match button {
            Button::Right => Tool::Eraser,
            _ if mods.alt => Tool::Picker,
            _ => chosen,
        };
        self.editor.rhythm = self.rhythm || mods.shift;
        let (dx, dy) = self.view.to_doc(x, y);
        let command = DrawCmd::Begin { tool, x: dx, y: dy };
        match self.editor.apply(command) {
            Ok(_) if self.editor.is_stroking() => self.gesture = Gesture::Stroke,
            Ok(_) => self.ui_dirty = true,
            Err(e) => self.notify_error(e.to_string()),
        }
    }

    pub fn canvas_move(&mut self, x: i32, y: i32) {
        match self.gesture {
            Gesture::None => {}
            Gesture::Stroke if self.tile_ui.map_stroke => self.tile_canvas_move(x, y),
            Gesture::Stroke => {
                let (dx, dy) = self.view.to_doc(x, y);
                self.run_on_canvas(DrawCmd::To { x: dx, y: dy });
            }
            Gesture::Pan { x: px, y: py } => {
                self.view.x += x - px;
                self.view.y += y - py;
                self.gesture = Gesture::Pan { x, y };
                self.full_canvas = true;
            }
            Gesture::Marquee { .. }
            | Gesture::Lasso { .. }
            | Gesture::Grab { .. }
            | Gesture::Move { .. }
            | Gesture::Scale { .. } => self.selection_move(x, y),
            Gesture::Cel { .. } => self.cel_drag_to(x, y),
        }
    }

    pub fn finish_gesture(&mut self) {
        match self.gesture {
            Gesture::None | Gesture::Pan { .. } => {}
            Gesture::Stroke => self.commit_stroke(),
            Gesture::Marquee { .. } | Gesture::Lasso { .. } | Gesture::Grab { .. } => {
                self.finish_selecting();
            }
            Gesture::Move { .. } => {
                self.run(FloatingCmd::GestureEnd);
            }
            Gesture::Scale { .. } => {
                self.apply_scale_target();
                self.run(FloatingCmd::GestureEnd);
            }
            Gesture::Cel { .. } => {
                self.run(StepCmd::EndDrag);
            }
        }
        self.gesture = Gesture::None;
    }

    /// Abandons the current drag, leaving the art as it was before the drag.
    pub fn cancel_gesture(&mut self) {
        self.scale_target = None;
        match self.gesture {
            Gesture::Grab { auto: true, .. } => self.deselect(),
            Gesture::Stroke => self.cancel_stroke(),
            Gesture::Move { lifted: true, .. } | Gesture::Scale { lifted: true, .. } => {
                self.run(FloatingCmd::Revert);
            }
            Gesture::Move { .. } | Gesture::Scale { .. } => {
                self.run(FloatingCmd::GestureCancel);
            }
            Gesture::Cel { .. } => {
                self.run(StepCmd::AbortJoin);
            }
            _ => {}
        }
        self.lasso.clear();
        self.gesture = Gesture::None;
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    pub fn zoom(&mut self, steps: i32, x: i32, y: i32) {
        self.view = self.view.zoomed(steps, x, y);
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    pub fn fit_view(&mut self) {
        let doc = &self.editor.doc();
        let margin = 16 * self.ui.scale;
        self.view = View::fit(
            doc.width as i32,
            doc.height as i32,
            self.layout.canvas.inset(margin),
        );
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    pub fn update_cursor(&self, window: Window) {
        window.set_cursor(self.cursor());
    }

    /// The pointer for where it is and what is going on.
    pub(crate) fn cursor(&self) -> Cursor {
        if self.loading.is_some() {
            return Cursor::Busy;
        }
        let (x, y) = self.ui.mouse;
        let panning = self.space_held || matches!(self.gesture, Gesture::Pan { .. });
        let free = self.ui.active == 0 && self.gesture == Gesture::None;
        let timing = self.ui.active == ID_READOUT || (free && self.timeline.readout.contains(x, y));
        if timing {
            Cursor::SizeWe
        } else if !self.layout.canvas.contains(x, y) {
            Cursor::Arrow
        } else if panning {
            Cursor::Move
        } else if matches!(self.tool, ToolChoice::Paint(_)) {
            Cursor::Cross
        } else {
            self.selection_cursor(x, y)
        }
    }

    /// Picks up a changed selection before anything is drawn.
    pub fn sync_selection(&mut self) {
        let version = self.editor.selection_version();
        if version == self.drawn_selection {
            return;
        }
        self.drawn_selection = version;
        let shape = self.editor.selection_shape_version();
        let origin = self.editor.selection().map_or((0, 0), |mask| {
            let (x, y, _, _) = mask.bounds();
            (x, y)
        });
        if shape == self.drawn_shape {
            let (dx, dy) = (
                origin.0 - self.outline_origin.0,
                origin.1 - self.outline_origin.1,
            );
            for edge in &mut self.outline {
                edge.x += dx;
                edge.y += dy;
            }
        } else {
            self.drawn_shape = shape;
            self.outline = self
                .editor
                .selection()
                .map_or_else(Vec::new, |mask| mask.outline());
        }
        self.outline_origin = origin;
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    /// Stretches the outline over a size not applied yet.
    pub fn sync_scale_outline(&mut self) {
        // From the size applied so far, which a small selection grows through on its way.
        let applied = self.editor.selection().map(|m| m.bounds());
        if let (Some(from), Some(target)) = (applied, self.scale_target) {
            self.scale_outline = scale_edges(&self.outline, from, target);
        }
    }

    fn has_marks_to_animate(&self) -> bool {
        (!self.outline.is_empty() && self.text.is_none())
            || !self.lasso.is_empty()
            || self.draft_outline().is_some()
    }

    /// The selection outline marches only while there is one, so idle stays idle.
    pub fn ants_wait(&self) -> Option<Duration> {
        self.has_marks_to_animate()
            .then(|| self.next_ant_at.saturating_duration_since(Instant::now()))
    }

    fn advance_ants(&mut self) -> bool {
        let now = Instant::now();
        if !self.has_marks_to_animate() || now < self.next_ant_at {
            return false;
        }
        self.ant_phase = (self.ant_phase + 1) & 7;
        self.next_ant_at = now + ANT_STEP;
        true
    }

    /// Stops previewing, so the next frame shows the document as it really is.
    pub(crate) fn end_preview(&mut self) {
        if self.preview.take().is_some() {
            self.full_canvas = true;
        }
    }

    /// Shows a large moving selection without compositing it. `None` if it does not apply.
    fn draw_drag_preview(&mut self, surface: &mut Surface<'_>, area: Rect) -> Option<Rect> {
        if matches!(self.gesture, Gesture::Cel { .. }) {
            return self.draw_cel_preview(surface, area);
        }
        let target = self.scale_target;
        let pending = self.editor.floating_is_stale() || target.is_some();
        if !pending || self.editor.has_damage() {
            self.end_preview();
            return None;
        }
        let epoch = self.editor.floating_view().map_or(0, |f| f.epoch);
        if self
            .preview
            .as_ref()
            .is_some_and(|p| !p.is_for(epoch, self.view, area))
        {
            self.end_preview();
        }
        if self.preview.is_none() {
            let cells = self.editor.floating_view().map_or(0, |f| f.placed.len());
            let stretched = target.map_or(0, |(_, _, w, h)| w as usize * h as usize);
            if cells.max(stretched) < PREVIEW_MIN_CELLS {
                return None;
            }
            self.preview = DragPreview::build(&mut self.comp, &self.editor, self.view, area);
        }
        let preview = self.preview.as_mut()?;
        let moved = match target {
            Some(target) => preview.render_scaled(&self.editor, target),
            None => preview.render(&self.editor),
        };
        let everything = std::mem::take(&mut self.full_canvas);
        if !moved && !everything {
            return Some(Rect::default());
        }
        preview.blit(surface, &mut self.xmap);
        Some(area)
    }

    /// Composites what changed and blits only that.
    fn draw_composited(&mut self, surface: &mut Surface<'_>, area: Rect) -> Rect {
        self.changed.clear();
        let damage = self.editor.take_damage();
        let size = (self.comp.width(), self.comp.height());
        // Only what the view shows is composited now; the rest waits until it shows.
        self.comp
            .set_visible(Some(shown_region(self.view, area, size)));
        self.comp.update(&self.editor, damage, &mut self.changed);
        let (w, h) = (size.0 as i32, size.1 as i32);
        let (px, xmap) = (self.comp.pixels(), &mut self.xmap);
        if std::mem::take(&mut self.full_canvas) {
            blit_canvas(px, w, h, self.view, surface, area, xmap);
            return area;
        }
        let mut redrawn = Rect::default();
        for doc_rect in &self.changed {
            let clip = self.view.to_screen(*doc_rect).intersect(area);
            blit_canvas(px, w, h, self.view, surface, clip, xmap);
            redrawn = redrawn.union(clip);
        }
        redrawn
    }

    pub(crate) fn resized(&mut self, w: i32, h: i32, scale: i32) {
        self.size = (w, h);
        self.ui.scale = scale;
        // The document's edge line is one interface unit wide.
        let back = crate::look::backdrop_for(&pikslar_ui::theme::current(), scale);
        pikslar_render::set_backdrop(back);
        let wanted = self.wanted_layout();
        let before = std::mem::replace(&mut self.layout, wanted).canvas;
        self.follow_canvas(before);
        self.full_canvas = true;
        self.ui_dirty = true;
        if std::mem::take(&mut self.needs_fit) {
            self.fit_view();
        }
        if let Some(zoom) = self.start_zoom.take() {
            let (c, doc) = (self.layout.canvas, &self.editor.doc());
            self.view = View {
                num: zoom,
                den: 1,
                x: c.x + (c.w - doc.width as i32 * zoom) / 2,
                y: c.y + (c.h - doc.height as i32 * zoom) / 2,
            };
        }
    }

    /// Keeps the sprite in the same place within the canvas when the canvas moves or resizes.
    pub(crate) fn follow_canvas(&mut self, before: Rect) {
        let now = self.layout.canvas;
        // A stroke or drag in progress is anchored to the view it started with.
        if !before.is_empty() && matches!(self.gesture, Gesture::None) {
            self.view.x += (now.x + now.w / 2) - (before.x + before.w / 2);
            self.view.y += (now.y + now.h / 2) - (before.y + before.h / 2);
        }
    }

    /// Makes what is sized to the canvas again once the canvas changed size, by a resize, a
    /// crop or undoing one, and fits the new canvas on screen.
    pub(crate) fn follow_canvas_size(&mut self) {
        let doc = self.editor.doc();
        let size = (doc.width, doc.height);
        if (self.comp.width(), self.comp.height()) == size {
            return;
        }
        self.comp = self.comp.resized(size.0, size.1);
        self.preview = None;
        self.full_canvas = true;
        self.reset_lint();
        self.fit_view();
    }

    /// Brings the canvas on screen up to date.
    pub fn draw_canvas(&mut self, surface: &mut Surface<'_>, dirty: &mut Vec<Rect>) {
        self.follow_canvas_size();
        self.sync_slices();
        self.follow_tile_grid();
        let area = self.layout.canvas;
        self.refresh_off_canvas();
        let ticked = self.advance_ants();
        let (redrawn, fresh) = match self.draw_drag_preview(surface, area) {
            Some(redrawn) => (redrawn, false),
            None => {
                let redrawn = self.draw_composited(surface, area);
                (redrawn, !self.changed.is_empty())
            }
        };
        self.draw_marks(surface, redrawn);
        if !redrawn.is_empty() {
            dirty.push(redrawn);
        }
        let mut touched = redrawn;
        if ticked {
            let moving = self.marks_area().intersect(area);
            self.draw_marks(surface, moving);
            if !moving.is_empty() {
                dirty.push(moving);
            }
            touched = touched.union(moving);
        }
        let keyed = self.draw_key_cursor(surface, touched, dirty);
        let touched = touched.union(keyed);
        let outlined = self.draw_footprint(surface, touched, dirty);
        // The brush outline may have repainted over the ring, which follows it.
        let again = self.draw_key_cursor(surface, outlined, dirty);
        let touched = touched.union(outlined).union(again);
        self.draw_true_preview(surface, (touched, fresh), dirty);
    }

    /// The screen area that holds the selection outline, or everything while dragging.
    fn marks_area(&self) -> Rect {
        if !self.lasso.is_empty() || self.draft_outline().is_some() {
            return self.layout.canvas;
        }
        self.selection_on_screen()
            .map_or_else(Rect::default, |r| r.inset(-self.ui.scale * HANDLE_SIZE))
    }

    pub(crate) fn draw_marks(&self, surface: &mut Surface<'_>, area: Rect) {
        if area.is_empty() {
            return;
        }
        self.draw_lint_marks(surface, area);
        self.draw_off_canvas(surface, area);
        self.draw_tile_marks(surface, area);
        self.draw_slice_marks(surface, area);
        let (view, phase) = (self.view, self.ant_phase);
        let outline = if self.scale_target.is_some() {
            &self.scale_outline
        } else {
            &self.outline
        };
        if self.text.is_none() {
            draw_ants(surface, &view, area, outline, phase);
        }
        if let Some(edges) = self.draft_outline() {
            draw_ants(surface, &view, area, &edges, phase);
        }
        if !self.lasso.is_empty() {
            draw_path(surface, &view, area, &self.lasso, phase);
        }
        self.draw_handles(surface, area);
        self.draw_text_caret(surface, area);
    }

    fn draw_lint_marks(&self, surface: &mut Surface<'_>, area: Rect) {
        if !self.lint.enabled || self.editor.is_floating() {
            return;
        }
        let focus = self.lint.focus;
        for finding in &self.lint.findings {
            let shape = Rect::new(finding.x, finding.y, finding.w, finding.h);
            let color = if focus == Some(*finding) {
                theme::text()
            } else {
                theme::accent()
            };
            draw_frame(surface, &self.view, area, shape, color);
        }
    }

    fn draw_handles(&self, surface: &mut Surface<'_>, area: Rect) {
        let Some(r) = self.handles_rect() else {
            return;
        };
        let s = self.ui.scale;
        let size = HANDLE_SIZE * s;
        for handle in HANDLES {
            let (x, y) = handle_point(r, handle);
            let outer = Rect::new(x - size / 2, y - size / 2, size, size);
            surface.fill(outer.intersect(area), HANDLE_EDGE);
            surface.fill(outer.inset(s).intersect(area), HANDLE_FILL);
        }
    }
}
