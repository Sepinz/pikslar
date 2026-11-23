//! Working on several frames at once in the strip: duplicating and moving the selected frames,
//! by command or by dragging them, and dragging a tag's edges to give it other frames.

use crate::app::App;
use pikslar_core::{FrameCmd, TagCmd, TagId};
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::Interaction;

/// How close to a tag's end, in interface units, a press takes the edge instead of the tag.
const EDGE_REACH: i32 = 5;

/// Selected frames being dragged along the strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunDrag {
    pub from: usize,
    pub to: usize,
    /// The frame pressed, and the frame under the pointer now.
    pub grab: usize,
    pub over: usize,
}

impl RunDrag {
    /// Where the run's first frame would land, kept inside the strip; `None` if it stays.
    pub fn landing(&self, frames: usize) -> Option<usize> {
        let n = self.to - self.from + 1;
        let at = self.from as i64 + self.over as i64 - self.grab as i64;
        let at = at.clamp(0, frames.saturating_sub(n) as i64) as usize;
        (at != self.from).then_some(at)
    }
}

/// One end of a tag being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TagEdge {
    /// The tag, by id: the list may change under the drag, by undo or a key.
    pub tag: TagId,
    /// True for the tag's first frame, false for its last.
    pub start: bool,
    /// The frame under the pointer now.
    pub over: usize,
    /// The pointer has been over another frame since the press, so letting go is not a click.
    pub moved: bool,
}

impl TagEdge {
    /// The tag's frames if the edge were let go now. An edge never passes the other one.
    pub fn range(&self, (from, to): (usize, usize)) -> (usize, usize) {
        if self.start {
            (self.over.min(to), to)
        } else {
            (from, self.over.max(from))
        }
    }
}

impl App {
    /// Copies the selected frames straight after them and selects the copies.
    pub(crate) fn duplicate_run(&mut self, from: usize, to: usize) {
        if self.frame_op(FrameCmd::Duplicate { from, to }) {
            let n = to - from + 1;
            self.timeline.select(to + 1, to + n);
        }
    }

    /// Moves the selected frames so the first is at `at`, keeping them selected.
    pub(crate) fn move_run(&mut self, from: usize, to: usize, at: usize) {
        if self.frame_op(FrameCmd::MoveRun { from, to, at }) {
            self.timeline.select(at, at + to - from);
        }
    }

    /// A press on a selected frame, without Shift, starts dragging the selection.
    pub(crate) fn start_run_drag(&mut self, under: usize) -> bool {
        let Some((from, to)) = self.selected_frames() else {
            return false;
        };
        if self.ui.shift || !(from..=to).contains(&under) {
            return false;
        }
        self.timeline.run_drag = Some(RunDrag {
            from,
            to,
            grab: under,
            over: under,
        });
        true
    }

    /// Carries a dragged selection; on letting go, moves it, or if it never moved, goes to the
    /// frame pressed as a click would. True while the strip is busy with one, so it does not
    /// scrub.
    pub(crate) fn carry_run(&mut self, state: Interaction, under: usize) -> bool {
        let Some(drag) = self.timeline.run_drag.as_mut() else {
            return false;
        };
        if state.held && self.ui.down {
            if drag.over != under {
                drag.over = under;
                self.ui_dirty = true;
            }
            return true;
        }
        let drag = *drag;
        self.timeline.run_drag = None;
        self.ui_dirty = true;
        // The selection the drag began with may have changed under it, by undo or a key.
        if !self.ui.released || self.selected_frames() != Some((drag.from, drag.to)) {
            return true;
        }
        match drag.landing(self.editor.doc().frames.len()) {
            Some(at) => self.defer(move |a| a.move_run(drag.from, drag.to, at)),
            None if drag.over == drag.grab => {
                self.timeline.unselect();
                self.defer(move |a| a.set_frame(drag.grab));
            }
            None => {}
        }
        true
    }

    /// Where a dragged selection would land, outlined over the strip.
    pub(crate) fn draw_run_landing(&self, sf: &mut Surface<'_>, area: Rect, first: usize) {
        let Some(drag) = self.timeline.run_drag else {
            return;
        };
        let Some(at) = drag.landing(self.editor.doc().frames.len()) else {
            return;
        };
        outline_frames(
            sf,
            area,
            first,
            (at, at + drag.to - drag.from),
            self.ui.scale,
        );
    }

    /// A press near a tag's end, where that end is in view, takes the edge. `shown_to` is one
    /// past the last frame in view.
    pub(crate) fn start_tag_edge(
        &mut self,
        r: Rect,
        index: usize,
        under: usize,
        shown_to: usize,
    ) -> bool {
        let reach = EDGE_REACH * self.ui.scale;
        let x = self.ui.mouse.0;
        let Some(tag) = self.editor.doc().tags.get(index) else {
            return false;
        };
        let first = self.timeline.first();
        let start = (x - r.x).abs() <= reach && tag.from >= first;
        let end = !start && (r.right() - x).abs() <= reach && tag.to < shown_to;
        if !start && !end {
            return false;
        }
        self.timeline.tag_edge = Some(TagEdge {
            tag: tag.id,
            start,
            over: under,
            moved: false,
        });
        true
    }

    /// Follows a dragged tag edge; on letting go, gives the tag its new frames. Returns true on
    /// the pass an edge that moved is let go, so the press does not also count as a click.
    pub(crate) fn carry_tag_edge(&mut self, under: usize) -> bool {
        let Some(edge) = self.timeline.tag_edge.as_mut() else {
            return false;
        };
        if self.ui.down {
            if edge.over != under {
                edge.over = under;
                edge.moved = true;
                self.ui_dirty = true;
            }
            return false;
        }
        let edge = *edge;
        self.timeline.tag_edge = None;
        self.ui_dirty = true;
        if !edge.moved {
            return false;
        }
        let tags = &self.editor.doc().tags;
        let Some((index, tag)) = tags.iter().enumerate().find(|(_, t)| t.id == edge.tag) else {
            return true;
        };
        let (from, to) = edge.range((tag.from, tag.to));
        if self.ui.released && (from, to) != (tag.from, tag.to) {
            let tag = index;
            self.defer(move |a| {
                a.finish_naming();
                a.run(TagCmd::SetRange { tag, from, to });
            });
        }
        true
    }

    /// The frames a dragged tag edge would give the tag, outlined over the tag row.
    pub(crate) fn draw_tag_edge(&self, sf: &mut Surface<'_>, area: Rect, first: usize) {
        let Some(edge) = self.timeline.tag_edge else {
            return;
        };
        let Some(tag) = self.editor.doc().tags.iter().find(|t| t.id == edge.tag) else {
            return;
        };
        let range = edge.range((tag.from, tag.to));
        outline_frames(sf, area, first, range, self.ui.scale);
    }
}

/// A frame around the columns of some frames, as far as they are in view.
fn outline_frames(
    sf: &mut Surface<'_>,
    area: Rect,
    first: usize,
    (from, to): (usize, usize),
    s: i32,
) {
    let cell = crate::panel_timeline::CELL * s;
    let left = from as i32 - first as i32;
    let whole = Rect::new(
        area.x + left * cell,
        area.y,
        (to - from + 1) as i32 * cell - 2 * s,
        area.h,
    );
    let r = whole.intersect(area);
    if r.is_empty() {
        return;
    }
    for edge in [
        Rect::new(r.x, r.y, r.w, s),
        Rect::new(r.x, r.bottom() - s, r.w, s),
        Rect::new(r.x, r.y, s, r.h),
        Rect::new(r.right() - s, r.y, s, r.h),
    ] {
        sf.fill(edge, theme::text());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dragged_run_follows_the_pointer_and_stays_in_the_strip() {
        let drag = |grab, over| RunDrag {
            from: 2,
            to: 3,
            grab,
            over,
        };
        assert_eq!(drag(3, 3).landing(8), None, "not moved");
        assert_eq!(drag(3, 5).landing(8), Some(4));
        assert_eq!(drag(2, 0).landing(8), Some(0));
        assert_eq!(
            drag(2, 7).landing(8),
            Some(6),
            "the last two frames at most"
        );
        assert_eq!(drag(3, 0).landing(8), Some(0), "never before the first");
    }

    #[test]
    fn a_tag_edge_never_passes_the_other_edge() {
        let edge = |start, over| TagEdge {
            tag: TagId::fresh(),
            start,
            over,
            moved: true,
        };
        assert_eq!(edge(true, 1).range((3, 5)), (1, 5));
        assert_eq!(edge(true, 7).range((3, 5)), (5, 5));
        assert_eq!(edge(false, 8).range((3, 5)), (3, 8));
        assert_eq!(edge(false, 0).range((3, 5)), (3, 3));
    }
}
