//! Choosing which cells later edits apply to.

use crate::editor::Editor;
use crate::history::Edit;
use crate::mask::Mask;
use crate::span_fill::scan_fill;
use std::sync::Arc;

/// How a new shape combines with the existing selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectOp {
    Replace,
    Add,
    Subtract,
}

impl Editor {
    pub fn selection(&self) -> Option<&Arc<Mask>> {
        self.selection.as_ref()
    }

    /// Changes whenever the selection's shape or position changes.
    pub fn selection_version(&self) -> u64 {
        self.selection_version
    }

    /// Changes when the selection's shape changes, but not when it only moves.
    pub fn selection_shape_version(&self) -> u64 {
        self.selection_version - self.selection_moved
    }

    fn canvas(&self) -> (i32, i32) {
        (self.doc.width as i32, self.doc.height as i32)
    }

    pub(crate) fn set_selection(&mut self, mask: Option<Mask>) {
        self.selection = mask.filter(|m| !m.is_empty()).map(Arc::new);
        self.selection_temporary = false;
        self.selection_version += 1;
    }

    /// Changes the selection as one step, unless applying floating pixels already made it.
    fn select_as_step(&mut self, pushes_before: u64, mask: Option<Mask>) {
        let next = mask.filter(|m| !m.is_empty());
        if self.selection.as_deref() == next.as_ref() {
            return;
        }
        if pushes_before != self.history.pushes() {
            // Applying floating pixels made the step, so undo must also bring this selection back.
            self.history.keep_selection_on_last(self.selection.clone());
        } else if !self.selection_temporary {
            self.record(Edit::Selection(self.selection.clone()));
        }
        self.set_selection(next);
    }

    /// Selects a shape just to pick it up. It leaves no step, and undo will not come back to it.
    pub(crate) fn select_temporarily(&mut self, shape: Mask) {
        self.anchor();
        let (w, h) = self.canvas();
        self.set_selection(Some(shape.within(w, h).trimmed()));
        self.selection_temporary = true;
    }

    /// Applies a new shape, keeping only what lies on the canvas.
    pub(crate) fn select(&mut self, shape: Mask, op: SelectOp) {
        let pushes = self.history.pushes();
        self.anchor();
        let (w, h) = self.canvas();
        let shape = shape.within(w, h);
        let combined = match (op, self.selection.as_deref()) {
            (SelectOp::Add, Some(current)) => current.union(&shape),
            (SelectOp::Subtract, Some(current)) => current.subtract(&shape),
            (SelectOp::Subtract, None) => return,
            (SelectOp::Replace | SelectOp::Add, _) => shape,
        };
        self.select_as_step(pushes, Some(combined.trimmed()));
    }

    pub(crate) fn select_all(&mut self) {
        let (w, h) = self.canvas();
        self.select(Mask::rect(0, 0, w, h), SelectOp::Replace);
    }

    pub(crate) fn deselect(&mut self) {
        let pushes = self.history.pushes();
        self.anchor();
        self.select_as_step(pushes, None);
    }

    pub(crate) fn invert_selection(&mut self) {
        let pushes = self.history.pushes();
        self.anchor();
        let (w, h) = self.canvas();
        let inverted = match self.selection.as_deref() {
            Some(current) => current.inverted(w, h),
            None => Mask::rect(0, 0, w, h),
        };
        self.select_as_step(pushes, Some(inverted.trimmed()));
    }

    /// The rectangle between two corners, clamped to the canvas.
    pub fn rect_shape(&self, a: (i32, i32), b: (i32, i32)) -> Mask {
        let (w, h) = self.canvas();
        let (x0, x1) = (a.0.min(b.0).max(0), a.0.max(b.0).min(w - 1));
        let (y0, y1) = (a.1.min(b.1).max(0), a.1.max(b.1).min(h - 1));
        Mask::rect(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
    }

    /// The area enclosed by a freehand path.
    pub fn lasso_shape(&self, path: &[(i32, i32)]) -> Mask {
        let (w, h) = self.canvas();
        let clamped: Vec<(i32, i32)> = path
            .iter()
            .map(|&(x, y)| (x.clamp(0, w - 1), y.clamp(0, h - 1)))
            .collect();
        Mask::polygon(&clamped)
    }

    /// Every painted cell of the active layer.
    pub fn opaque_shape(&self) -> Mask {
        if self.on_tilemap() {
            return self.opaque_map();
        }
        let (w, h) = self.canvas();
        let mut cel = self.cel().reader();
        let mut mask = Mask::blank(0, 0, w, h);
        for y in 0..h {
            for x in 0..w {
                if cel.get(x, y) != 0 {
                    mask.set(x, y);
                }
            }
        }
        mask
    }

    /// The cells matching the one at a point: joined to it, or anywhere on the layer.
    ///
    /// Palette cells compare by color ID and RGBA cells by exact color, so each mode
    /// selects what the artist sees as one color. A tilemap layer is read as the pixels its
    /// tiles draw.
    pub fn wand_shape(&self, x: i32, y: i32, contiguous: bool) -> Option<Mask> {
        let (w, h) = self.canvas();
        if x < 0 || y < 0 || x >= w || y >= h {
            return None;
        }
        if self.on_tilemap() {
            let shown = |px, py| self.shown_pixel(px, py);
            return Some(wand_mask((w, h), (x, y), contiguous, shown));
        }
        Some(wand_mask((w, h), (x, y), contiguous, self.cel().sampler()))
    }
}

/// The cells of a `w` by `h` canvas equal to the one at a point, joined to it or anywhere.
fn wand_mask(
    (w, h): (i32, i32),
    (x, y): (i32, i32),
    contiguous: bool,
    value_at: impl Fn(i32, i32) -> u32,
) -> Mask {
    let target = value_at(x, y);
    let mut mask = Mask::blank(0, 0, w, h);
    if !contiguous {
        for py in 0..h {
            for px in 0..w {
                if value_at(px, py) == target {
                    mask.set(px, py);
                }
            }
        }
        return mask;
    }
    scan_fill(
        (w, h),
        (x, y),
        |px, py| value_at(px, py) == target,
        |px, py| mask.set(px, py),
    );
    mask
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::rgba;
    use crate::document::{ColorMode, Document};
    use crate::editor::Tool;

    const RED: u32 = rgba(255, 0, 0, 255);
    const BLUE: u32 = rgba(0, 0, 255, 255);

    fn editor(mode: ColorMode) -> Editor {
        let mut e = Editor::new(Document::new(32, 32, mode).expect("valid"));
        e.color = RED;
        e.pixel_perfect = false;
        e
    }

    fn line(e: &mut Editor, a: (i32, i32), b: (i32, i32)) {
        e.begin(Tool::Line, a.0, a.1).expect("begin");
        e.drag(b.0, b.1);
        e.commit();
    }

    fn modes() -> [ColorMode; 2] {
        [ColorMode::Palette, ColorMode::Rgba]
    }

    #[test]
    fn shapes_combine_with_the_current_selection() {
        let mut e = editor(ColorMode::Rgba);
        e.select(Mask::rect(0, 0, 4, 4), SelectOp::Replace);
        e.select(Mask::rect(10, 10, 2, 2), SelectOp::Add);
        assert_eq!(e.selection().expect("set").count(), 20);
        e.select(Mask::rect(0, 0, 2, 2), SelectOp::Subtract);
        assert_eq!(e.selection().expect("set").count(), 16);
    }

    #[test]
    fn subtracting_everything_clears_the_selection() {
        let mut e = editor(ColorMode::Rgba);
        e.select(Mask::rect(0, 0, 4, 4), SelectOp::Replace);
        e.select(Mask::rect(0, 0, 4, 4), SelectOp::Subtract);
        assert!(e.selection().is_none());
    }

    #[test]
    fn selections_outside_the_canvas_are_clipped() {
        let mut e = editor(ColorMode::Rgba);
        e.select(e.rect_shape((-9, -9), (2, 2)), SelectOp::Replace);
        assert_eq!(e.selection().expect("set").bounds(), (0, 0, 3, 3));
        e.select(Mask::rect(90, 90, 4, 4), SelectOp::Replace);
        assert!(e.selection().is_none());
    }

    #[test]
    fn lasso_paths_far_off_canvas_stay_on_it() {
        let e = editor(ColorMode::Rgba);
        let shape = e.lasso_shape(&[(-5000, -5000), (5000, -5000), (5000, 5000)]);
        assert!(shape.bounds().2 <= 32 && shape.bounds().3 <= 32);
    }

    #[test]
    fn the_wand_finds_joined_cells_and_stops_at_other_colors() {
        for mode in modes() {
            let mut e = editor(mode);
            line(&mut e, (10, 0), (10, 31));
            let left = e.wand_shape(0, 0, true).expect("on canvas");
            assert_eq!(left.count(), 10 * 32, "{mode:?}");
            assert_eq!(e.wand_shape(10, 5, true).expect("on").count(), 32);
        }
    }

    #[test]
    fn the_wand_can_take_every_match_on_the_layer() {
        for mode in modes() {
            let mut e = editor(mode);
            line(&mut e, (2, 2), (5, 2));
            line(&mut e, (20, 20), (22, 20));
            e.color = BLUE;
            line(&mut e, (8, 8), (9, 8));
            assert_eq!(
                e.wand_shape(2, 2, false).expect("on").count(),
                7,
                "{mode:?}"
            );
            assert_eq!(e.wand_shape(2, 2, true).expect("on").count(), 4, "{mode:?}");
        }
    }

    #[test]
    fn the_wand_ignores_points_off_canvas() {
        assert!(editor(ColorMode::Rgba).wand_shape(-1, 40, true).is_none());
    }

    #[test]
    fn painting_stays_inside_the_selection() {
        for mode in modes() {
            let mut e = editor(mode);
            e.select(Mask::rect(5, 5, 5, 5), SelectOp::Replace);
            line(&mut e, (0, 7), (20, 7));
            assert_eq!(e.color_at(7, 7), RED, "{mode:?}");
            assert_eq!(e.color_at(4, 7), 0, "{mode:?}");
            assert_eq!(e.color_at(10, 7), 0, "{mode:?}");
        }
    }

    #[test]
    fn fill_treats_the_selection_edge_as_a_wall() {
        for mode in modes() {
            let mut e = editor(mode);
            e.select(Mask::rect(5, 5, 5, 5), SelectOp::Replace);
            e.begin(Tool::Fill, 7, 7).expect("fill");
            assert_eq!(e.color_at(9, 9), RED, "{mode:?}");
            assert_eq!(e.color_at(4, 4), 0, "{mode:?}");
            e.deselect();
            e.select(Mask::rect(5, 5, 5, 5), SelectOp::Replace);
            let before = e.history.undo_depth();
            e.begin(Tool::Fill, 20, 20).expect("fill outside");
            assert_eq!(e.history.undo_depth(), before, "{mode:?}");
        }
    }

    #[test]
    fn selection_changes_can_be_undone_but_never_mark_the_document_modified() {
        let mut e = editor(ColorMode::Rgba);
        let revision = e.revision;
        e.select_all();
        e.select(Mask::rect(1, 1, 2, 2), SelectOp::Replace);
        e.deselect();
        assert_eq!(e.history.undo_depth(), 3);
        assert!(e.undo());
        assert_eq!(e.selection().map(|m| m.count()), Some(4));
        assert!(e.undo());
        assert_eq!(e.selection().map(|m| m.count()), Some(32 * 32));
        assert!(e.redo());
        assert_eq!(e.selection().map(|m| m.count()), Some(4));
        assert_eq!(e.revision, revision);
    }

    #[test]
    fn a_selection_that_does_not_change_is_no_step() {
        let mut e = editor(ColorMode::Rgba);
        e.deselect();
        e.select_all();
        e.select_all();
        assert_eq!(e.history.undo_depth(), 1);
    }

    #[test]
    fn a_selection_made_to_pick_up_the_layer_is_no_step() {
        let mut e = editor(ColorMode::Rgba);
        line(&mut e, (1, 1), (5, 1));
        let depth = e.history.undo_depth();
        e.select_temporarily(Mask::rect(1, 1, 5, 1));
        e.deselect();
        assert_eq!(e.history.undo_depth(), depth);
        e.select_temporarily(Mask::rect(1, 1, 5, 1));
        e.lift(false);
        e.move_floating(2, 2);
        e.anchor();
        assert_eq!(e.history.undo_depth(), depth + 1);
        e.undo();
        assert!(e.selection().is_none(), "undo goes back before the pick-up");
    }
}
