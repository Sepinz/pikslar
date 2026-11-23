//! The position tween as a command: one step that gives the frames between two keys of a hold
//! offsets between the keys'. No pixel and no duration changes.
//!
//! Every frame of the run stays in its hold and keeps sharing its drawing; only its offset
//! changes. So afterwards a hold's frames may sit in different places: moving the hold moves
//! each of them by the same amount, and turning it is refused until they are lined up again,
//! as for any hold whose frames sit apart.

use super::LayerId;
use crate::document::{Document, LimitError};
use crate::editor::Editor;
use crate::tween::{tween_offsets, Ease};

/// The frames whose offset a tween changes, with where each goes.
type Moves = Vec<(usize, (i32, i32))>;

impl Editor {
    /// Tweens a run of one hold's frames as one step. Refusals change nothing.
    pub(super) fn tween_cels(
        &mut self,
        layer: LayerId,
        from: usize,
        to: usize,
        ease: Ease,
    ) -> Result<bool, LimitError> {
        let index = self.index_of(layer)?;
        if tween_moves(&self.doc, index, from, to, ease)?.is_empty() {
            return Ok(false);
        }
        let revision = self.revision;
        self.transact(|doc| {
            // Worked out again inside, after any floating pixels went down.
            for (frame, (x, y)) in tween_moves(doc, index, from, to, ease)? {
                let layer = &mut doc.layers[index];
                let moved = layer.cel(frame).moved_to(x, y);
                layer.put_cel(frame, moved.ok_or(LimitError::CanvasReach)?)?;
            }
            Ok(())
        })?;
        Ok(self.revision != revision)
    }
}

/// Where the frames of the run that must move go, or why the run cannot be tweened.
fn tween_moves(
    doc: &Document,
    index: usize,
    from: usize,
    to: usize,
    ease: Ease,
) -> Result<Moves, LimitError> {
    let cels = doc.layers.get(index).ok_or(LimitError::NoSuchItem)?;
    if from >= to || to - from < 2 || to >= cels.frame_count() {
        return Err(LimitError::TweenTooShort);
    }
    let hold = cels.link(from);
    if hold == 0 || (from..=to).any(|f| cels.link(f) != hold) {
        return Err(LimitError::TweenUnheld);
    }
    let place = |f: usize| {
        let (x, y) = cels.cel(f).position();
        (i32::from(x), i32::from(y))
    };
    let durations: Vec<u32> = (from..=to).map(|f| doc.frame_units(f)).collect();
    let offsets = tween_offsets(&durations, place(from), place(to), ease);
    let moves: Moves = (from..=to)
        .zip(offsets)
        .filter(|&(f, at)| place(f) != at)
        .collect();
    if moves
        .iter()
        .any(|&(_, (x, y))| !crate::layer::Cel::in_range(x, y))
    {
        return Err(LimitError::CanvasReach);
    }
    Ok(moves)
}
