//! Whether the document is where it was last saved, told by its place in history (see
//! `history/saved.rs`) because the revision moves on undo and redo too. The revision is for
//! what is derived from the document; this is only for whether the file is out of date.

use super::Editor;
use crate::history::SavedCopy;

impl Editor {
    /// The document as it is now is what the file holds.
    pub fn mark_saved(&mut self) {
        self.history.mark_saved();
    }

    /// Nothing is known to be what the file holds: the document is recovered work, or an
    /// import, which differs from every file until it is saved.
    pub fn forget_saved(&mut self) {
        self.history.forget_saved();
    }

    /// Takes note of the place the document is at, as a copy of it is taken to be written.
    /// While pixels float or a stroke is under way the document is not in a state history
    /// names, so the copy claims no place.
    pub fn begin_saved_copy(&mut self) -> SavedCopy {
        let claim = !self.is_busy();
        self.history.begin_copy(claim)
    }

    /// The copy was written: the file holds the document as it was at its place. Nothing
    /// becomes saved if that place was dropped since, or another copy was taken.
    pub fn finish_saved_copy(&mut self, copy: SavedCopy) {
        self.history.finish_copy(copy);
    }

    /// True if the document is at the place the file holds, so that what is shown is what was
    /// saved even though undo and redo have moved the revision on.
    pub fn is_at_saved(&self) -> bool {
        self.history.is_at_saved()
    }

    /// The step in progress went on changing the document: the place it led to is not the one
    /// that was saved, though the step is the same. Also moves the revision.
    pub(crate) fn amended(&mut self) {
        self.revision += 1;
        self.history.newest_step_changed();
    }

    /// The document changed in a way no step of history holds, so undo and redo cannot take
    /// it back and no place in history is what was saved. Also moves the revision.
    pub(crate) fn touched(&mut self) {
        self.revision += 1;
        self.history.forget_saved();
    }
}
