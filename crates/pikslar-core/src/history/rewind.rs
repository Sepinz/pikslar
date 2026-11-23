//! What Rewind needs from history (`docs/rewind.md`): where the undo list stands among every
//! step the document ever had, which of those steps changed since they were last written, and a
//! way to put a step read back from disk under the oldest one in memory.
//!
//! Positions count steps from the start of a document's history. The step at position `p`
//! takes the document from state `p - 1` to state `p`. None of this runs while undoing or
//! redoing, which leave every step's position as it is.

use super::{Edit, History};

/// The two counters history keeps for Rewind.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Marks {
    /// How many steps lie beneath the oldest one in memory: dropped for the budget, or kept
    /// only on disk.
    pub(crate) below: u64,
    /// The lowest position whose step was added, removed or changed since the last write.
    low: u64,
}

impl Default for Marks {
    fn default() -> Self {
        Marks {
            below: 0,
            low: u64::MAX,
        }
    }
}

impl Marks {
    pub(crate) fn rewrote(&mut self, at: u64) {
        self.low = self.low.min(at);
    }
}

/// Where history stands for Rewind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RewindMarks {
    /// The position of the state beneath the oldest step in memory.
    pub below: u64,
    /// The position of the state the document is in.
    pub depth: u64,
    /// Every step up to this position is as it was at the last [`History::clear_rewrites`].
    pub unchanged_up_to: u64,
}

impl History {
    /// Notes that the newest step was added, changed or is about to go.
    pub(super) fn mark_top(&mut self) {
        let at = self.depth();
        self.marks.rewrote(at);
    }

    /// The position of the state the document is in. Positions can come from disk, so they
    /// never overflow: past `u64::MAX` they stay there.
    fn depth(&self) -> u64 {
        self.marks.below.saturating_add(self.undo.len() as u64)
    }

    pub fn rewind_marks(&self) -> RewindMarks {
        RewindMarks {
            below: self.marks.below,
            depth: self.depth(),
            unchanged_up_to: self.marks.low.saturating_sub(1),
        }
    }

    /// Moves every position up by `by`: the document turned out to have that many steps
    /// before this session's.
    pub(crate) fn rebase(&mut self, by: u64) {
        match self.marks.below.checked_add(by) {
            Some(_) => self.saved.shifted(by),
            None => self.forget_saved(),
        }
        self.marks.below = self.marks.below.saturating_add(by);
        if self.marks.low != u64::MAX {
            self.marks.low = self.marks.low.saturating_add(by);
        }
    }

    /// Every step is as written now.
    pub(crate) fn clear_rewrites(&mut self) {
        self.marks.low = u64::MAX;
    }

    /// The steps from position `at` up were not written after all.
    pub(crate) fn not_written_from(&mut self, at: u64) {
        self.marks.rewrote(at);
    }

    /// The step undo plays next.
    pub(crate) fn newest_undo(&self) -> Option<&Edit> {
        self.undo.last().map(|(edit, _)| edit)
    }

    /// Puts a step from beneath the oldest one in memory into the undo list, leaving redo as
    /// it is. Only while nothing is left to undo, so the step goes on top and undo plays it.
    /// It is charged as any step is. False, changing nothing, if there is no room below.
    pub(crate) fn push_older(&mut self, mut edit: Edit) -> bool {
        if !self.undo.is_empty() || self.marks.below == 0 {
            return false;
        }
        edit.take_on(&mut self.held);
        let size = edit.size();
        self.used += size;
        self.undo.push((edit, size));
        self.marks.below -= 1;
        true
    }
}

impl History {
    /// The room the redo list has, to give back what a walk grew it by.
    pub(crate) fn redo_room(&self) -> usize {
        self.redo.capacity()
    }

    /// Gives back redo room past `room`, so a walk leaves history taking what it took.
    pub(crate) fn give_back_redo_room(&mut self, room: usize) {
        self.redo.shrink_to(room);
    }
}
