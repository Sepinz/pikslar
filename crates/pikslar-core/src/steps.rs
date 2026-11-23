//! Edits made a little at a time, which count as one step of history however long they take.

use crate::animation::{MAX_DURATION_MS, MIN_DURATION_MS};
use crate::command::LayerId;
use crate::document::LimitError;
use crate::editor::Editor;
use crate::history::{Edit, Place};

/// What a run of small changes is changing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dragged {
    Timing(usize, usize),
    Opacity(LayerId),
    Recolor(u32),
    TagName(usize),
    GroupName(usize),
    TilesetName(crate::tileset::TilesetId),
    TerrainName(crate::tileset::TilesetId, u16),
    /// A cel being moved, named by its layer and the hold or frame it shows, and whether only
    /// that cel moves.
    Cel(LayerId, usize, bool),
}

impl Editor {
    /// True if a change to this value continues the step already recorded for it.
    pub(crate) fn joins(&self, what: Dragged) -> bool {
        self.drag_mark
            .is_some_and(|(value, pushes, _)| value == what && pushes == self.history.pushes())
    }

    /// Records the step for this value, unless the last change to it already did.
    pub(crate) fn join_step(&mut self, what: Dragged, place: Place) {
        if self.joins(what) {
            return;
        }
        let revision = self.revision;
        self.close_open_step();
        self.cancel();
        let mut snapshot = crate::history::Snapshot::capture(&self.doc, place);
        self.doc.layers.start_log();
        drop(self.absorb_floating(&mut snapshot));
        snapshot.take_log(&mut self.doc);
        // Finished when the value is, since the change is still to come.
        self.record_joined(Edit::Snapshot(Box::new(snapshot)));
        self.mark_joined(what, revision);
    }

    /// Later changes to this value join the step just recorded.
    pub(crate) fn mark_joined(&mut self, what: Dragged, revision: u64) {
        self.drag_mark = Some((what, self.history.pushes(), revision));
    }

    /// The next change of a dragged value starts a new step: the pointer was released.
    /// Goes back to the number from before a step that came to nothing, so the file counts as
    /// unchanged. Anything using the number to notice change must also watch the rewinds.
    fn rewind_revision(&mut self, revision: u64) {
        self.revision = revision;
        self.revision_rewinds += 1;
    }

    pub(crate) fn end_join(&mut self) {
        self.close_open_step();
    }

    /// Ends the value in progress, keeping only what its step altered. A finished step is never
    /// joined again, so call this only while the document is as the value left it.
    pub(crate) fn close_open_step(&mut self) {
        self.history.finish_last(&self.doc);
        self.history.drop_aside();
        self.drag_mark = None;
    }

    /// Runs something that may change more than the value in progress, with the value's step
    /// finished while the document is as the value left it. If it changes nothing and does not
    /// end the value, the step opens again, so the value can still go on in it.
    pub(crate) fn with_open_step_finished<R>(&mut self, run: impl FnOnce(&mut Self) -> R) -> R {
        let before = (self.history.pushes(), self.revision);
        let mark = self.drag_mark;
        let finished = self.history.finish_last(&self.doc);
        let result = run(self);
        // A mark `run` set is for the step it recorded, and stays.
        if finished && self.drag_mark.is_some() && self.drag_mark == mark {
            match before == (self.history.pushes(), self.revision) {
                true => self.history.reopen_last(&self.doc),
                // A finished step is never joined again.
                false => self.drag_mark = None,
            }
        }
        result
    }

    /// The value is finished. If it ended where it started, there is nothing to undo.
    pub(crate) fn finish_join(&mut self) {
        if let Some((_, pushes, revision)) = self.drag_mark.take() {
            let selection = self.selection.as_ref();
            if pushes == self.history.pushes() && self.history.pop_if_same(&self.doc, selection) {
                self.rewind_revision(revision);
            }
        }
        self.close_open_step();
    }

    /// Takes back the step of the value in progress, leaving nothing to redo.
    pub(crate) fn abort_join(&mut self) {
        let Some((_, pushes, revision)) = self.drag_mark.take() else {
            return;
        };
        if pushes != self.history.pushes() {
            return;
        }
        let mut place = (self.layer, self.frame);
        if self
            .history
            .discard_last(&mut self.doc, &mut self.selection, &mut place)
        {
            self.after_history(true, true, place, None);
            self.rewind_revision(revision);
        }
    }

    /// Sets the timing of the frames from `from` onward, one value for each.
    pub(crate) fn retime(&mut self, from: usize, durations: &[u32]) -> Result<bool, LimitError> {
        let count = durations
            .len()
            .min(self.doc.frames.len().saturating_sub(from));
        if count == 0 {
            return Ok(false);
        }
        let clamped = durations[..count].iter().map(|ms| {
            self.doc
                .snapped((*ms).clamp(MIN_DURATION_MS, MAX_DURATION_MS))
        });
        let wanted: Vec<u32> = clamped.collect();
        let now = self.doc.frames[from..from + count].iter();
        if now.map(|f| f.duration_ms).eq(wanted.iter().copied()) {
            return Ok(false);
        }
        // Timing can change while another frame is playing, so go back to the one changed.
        self.join_step(Dragged::Timing(from, from + count - 1), (self.layer, from));
        // Each frame keeps its key: only its timing changes.
        let frames = self.doc.frames[from..from + count].iter_mut();
        frames
            .zip(wanted)
            .for_each(|(frame, ms)| frame.duration_ms = ms);
        self.amended();
        Ok(true)
    }

    /// Sets a layer's opacity.
    pub(crate) fn set_opacity(&mut self, layer: LayerId, opacity: u8) -> Result<bool, LimitError> {
        let index = self.index_of(layer)?;
        if self.doc.layers[index].opacity == opacity {
            return Ok(false);
        }
        self.join_step(Dragged::Opacity(layer), (self.layer, self.frame));
        self.doc.layers[index].opacity = opacity;
        self.damage_all();
        self.amended();
        Ok(true)
    }
}
