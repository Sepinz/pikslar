//! Where in the history the file on disk was saved, so that undo and redo can bring the
//! document back to "unchanged". The revision cannot say it: undo and redo advance it.
//!
//! A position counts the steps from the document as it was opened: the state after the step at
//! position `p` is state `p`, and state 0 is the document as opened. Positions do not move when
//! the oldest steps are dropped for the budget or when Rewind reports earlier steps beneath.
//!
//! The saved position is a claim that the document at that position is what the file holds.
//! Anything that could make the claim untrue drops it, and it is only ever dropped, never
//! restored: a change to a step at position `p` drops a claim at `p` or above (the states there
//! were made with the step), a change outside the history drops every claim, and a step dropped
//! from the oldest end drops a claim beneath what is kept. Where unsure, drop it: the document
//! then reads as modified until it is saved again, which costs a question and never work.

use super::History;
use std::sync::atomic::{AtomicU64, Ordering};

/// Hands out each copy's number once, across every editor, so that the end of a save of one
/// document can never be taken for the end of a save of another.
static NEXT_COPY: AtomicU64 = AtomicU64::new(1);

/// A copy of the document taken to be written, to be handed back when the write has ended.
/// It names the place in history the copy was taken at, which is where the file will be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedCopy(u64);

impl SavedCopy {
    /// A copy that claims no place: nothing becomes saved when it is finished.
    pub const NONE: SavedCopy = SavedCopy(0);
}

/// The claims history keeps: where the file is, and where the copy being written is.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Saved {
    at: Option<i128>,
    copy: Option<(u64, i128)>,
    /// How far Rewind moved the positions of the steps in memory up: taken off again here.
    shift: u64,
}

impl Saved {
    /// Raises the shift as Rewind raises the positions it counts, so a position stays put.
    pub(super) fn shifted(&mut self, by: u64) {
        self.shift = self.shift.saturating_add(by);
    }

    fn forget_from(&mut self, from: i128) {
        self.at = self.at.filter(|at| *at < from);
        self.copy = self.copy.filter(|(_, at)| *at < from);
    }
}

impl History {
    /// The position of the state the document is in.
    pub(crate) fn position(&self) -> i128 {
        i128::from(self.marks.below) + self.undo.len() as i128 - i128::from(self.saved.shift)
    }

    /// The document as it is now is what the file holds.
    pub(crate) fn mark_saved(&mut self) {
        self.saved.at = Some(self.position());
    }

    /// Nothing is known to be what the file holds, and nothing the copy being written holds.
    pub(crate) fn forget_saved(&mut self) {
        self.saved.at = None;
        self.saved.copy = None;
    }

    /// Notes the place a copy is taken at, to become the saved one when its write ends.
    /// `claim` is false when the document is not in a state history can name.
    pub(crate) fn begin_copy(&mut self, claim: bool) -> SavedCopy {
        self.saved.copy = None;
        if !claim {
            return SavedCopy::NONE;
        }
        let number = NEXT_COPY.fetch_add(1, Ordering::Relaxed);
        self.saved.copy = Some((number, self.position()));
        SavedCopy(number)
    }

    /// The write of `copy` ended: its place is where the file is, unless it was dropped since,
    /// or a newer copy was taken.
    pub(crate) fn finish_copy(&mut self, copy: SavedCopy) {
        // The file holds the copy now, so whatever place it held before is out of date.
        let claimed = self.saved.copy.filter(|(number, _)| *number == copy.0);
        self.saved.at = claimed.map(|(_, at)| at);
        if claimed.is_some() {
            self.saved.copy = None;
        }
    }

    /// True if the document is at the place the file was saved at.
    pub(crate) fn is_at_saved(&self) -> bool {
        self.saved.at == Some(self.position())
    }

    /// The step at position `from`, or a step above it, was changed, dropped or replaced.
    pub(crate) fn forget_saved_from(&mut self, from: i128) {
        self.saved.forget_from(from);
    }

    /// The newest step, which is at the position the document is in, was changed.
    pub(crate) fn newest_step_changed(&mut self) {
        let at = self.position();
        self.saved.forget_from(at);
    }

    /// The oldest steps were dropped: positions beneath what is kept can not be reached again.
    pub(super) fn forget_saved_below_kept(&mut self) {
        let kept = i128::from(self.marks.below) - i128::from(self.saved.shift);
        self.saved.at = self.saved.at.filter(|at| *at >= kept);
        self.saved.copy = self.saved.copy.filter(|(_, at)| *at >= kept);
    }
}
