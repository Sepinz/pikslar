//! Shows where the brush will land before the button goes down.
//!
//! It costs a few cells of redrawing when the pointer changes cell, and nothing while a stroke
//! is under way, because it is hidden then: the ink itself is the feedback. A custom brush
//! shows the outline of exactly the cells one stamp paints.

use crate::app::{App, Gesture, ToolChoice};
use crate::footprint_shape::Shape;
use pikslar_core::Tool;
use pikslar_render::{blit_canvas, draw_ants, screen_rect, Rect, Surface};
use std::cell::RefCell;

#[derive(Default)]
pub struct Footprint {
    /// The outline of the brush in use, made when the brush or its size changes.
    pub(crate) shape: RefCell<Option<Shape>>,
    /// The screen area it was last drawn over.
    pub(crate) shown: Rect,
    /// Where it was last drawn: the stroke cell and which shape. A brush bigger than the canvas
    /// can move without the area it covers changing.
    placed: Option<(i32, i32, u64)>,
    /// The pointer has left the window, which no pointer position can show.
    pub pointer_away: bool,
}

impl App {
    fn brush_tool(&self) -> bool {
        matches!(
            self.tool,
            ToolChoice::Paint(Tool::Pencil | Tool::Eraser | Tool::Line | Tool::Rect)
        )
    }

    /// The cell the stroke would be at, if the brush should be shown.
    fn footprint_cell(&self) -> Option<(i32, i32)> {
        let (x, y) = self.ui.mouse;
        let hovering = self.gesture == Gesture::None
            && !self.ui.down
            && self.preview.is_none()
            && !self.space_held
            && !self.footprint.pointer_away
            && !self.panel_open()
            && self.on_canvas(x, y)
            && self.ui.widget_at(x, y) == 0;
        if !hovering || !self.brush_tool() {
            return None;
        }
        let (cx, cy) = self.view.to_doc(x, y);
        let ((bx, by, bw, bh), _) = self.shape_bounds();
        let (left, top) = (cx + bx, cy + by);
        let (w, h) = (
            self.editor.doc().width as i32,
            self.editor.doc().height as i32,
        );
        (left < w && top < h && left + bw > 0 && top + bh > 0).then_some((cx, cy))
    }

    /// The screen area the brush outline covers, empty if it is not showing.
    pub(crate) fn footprint_area(&self) -> Rect {
        self.footprint_cell().map_or_else(Rect::default, |(x, y)| {
            let ((bx, by, bw, bh), _) = self.shape_bounds();
            screen_rect(&self.view, x + bx, y + by, bw, bh)
                .inset(-1)
                .intersect(self.layout.canvas)
        })
    }

    #[cfg(test)]
    /// The edges of the outline now showing, in document cells, or none if it is hidden.
    pub(crate) fn footprint_edges(&self) -> Vec<pikslar_core::Edge> {
        self.footprint_cell()
            .map_or_else(Vec::new, |(x, y)| self.outline_at(x, y))
    }

    /// Removes the old outline and draws the new one. `touched` is what was just repainted.
    /// Returns what this repainted in turn.
    pub(crate) fn draw_footprint(
        &mut self,
        surface: &mut Surface<'_>,
        touched: Rect,
        dirty: &mut Vec<Rect>,
    ) -> Rect {
        let wanted = self.footprint_area();
        let generation = self.shape_bounds().1;
        let placed = self.footprint_cell().map(|(x, y)| (x, y, generation));
        let shown = self.footprint.shown;
        let changed = wanted != shown || placed != self.footprint.placed;
        let mut painted = Rect::default();
        if changed && !shown.is_empty() {
            painted = self.restore_canvas(surface, shown, dirty);
        }
        self.footprint.shown = wanted;
        self.footprint.placed = placed;
        if let Some((x, y, _)) = placed.filter(|_| changed || overlaps(wanted, touched)) {
            let edges = self.outline_at(x, y);
            draw_ants(surface, &self.view, wanted, &edges, 0);
            dirty.push(wanted);
            painted = painted.union(wanted);
        }
        painted
    }

    /// Paints the canvas back over `shown`, which an outline or the true-size preview covered.
    /// The window may have shrunk since. Returns the area painted.
    pub(crate) fn restore_canvas(
        &mut self,
        surface: &mut Surface<'_>,
        shown: Rect,
        dirty: &mut Vec<Rect>,
    ) -> Rect {
        let canvas = self.layout.canvas;
        let area = shown.intersect(canvas);
        if let Some(preview) = self.preview.as_mut() {
            preview.blit(surface, &mut self.xmap);
            self.draw_marks(surface, canvas);
            dirty.push(canvas);
            return canvas;
        }
        let (w, h) = (self.comp.width() as i32, self.comp.height() as i32);
        let view = self.view;
        blit_canvas(
            self.comp.pixels(),
            w,
            h,
            view,
            surface,
            area,
            &mut self.xmap,
        );
        self.draw_marks(surface, area);
        dirty.push(area);
        area
    }
}

fn overlaps(a: Rect, b: Rect) -> bool {
    !a.intersect(b).is_empty()
}
