//! Frame and hold commands.

use super::{FrameCmd, Outcome};
use crate::document::LimitError;
use crate::editor::Editor;

impl Editor {
    pub(super) fn apply_frame(&mut self, command: FrameCmd) -> Result<Outcome, LimitError> {
        match command {
            FrameCmd::Add { at, copy } => {
                self.transact(|d| d.add_frame(at, copy))?;
                self.frame = at.min(self.doc.frames.len() - 1);
            }
            FrameCmd::Remove { from, to } => {
                self.transact(|d| d.remove_frames(from, to))?;
                self.frame = from.min(self.doc.frames.len() - 1);
            }
            FrameCmd::Move { from, to } => {
                self.transact(|d| d.move_frame(from, to))?;
                self.frame = to;
            }
            FrameCmd::Retime { from, durations } => {
                return Ok(super::did(self.retime(from, &durations)?));
            }
            FrameCmd::HoldPrevious { layer, frame } => {
                let index = self.index_of(layer)?;
                self.transact(|d| d.hold_previous(index, frame))?;
            }
            FrameCmd::HoldAcross {
                layer,
                source,
                from,
                to,
            } => {
                let index = self.index_of(layer)?;
                self.transact(|d| d.hold_across(index, source, from, to))?;
            }
            FrameCmd::Release { layer, from, to } => {
                let index = self.index_of(layer)?;
                self.transact(|d| (from..=to).try_for_each(|f| d.release_hold(index, f)))?;
            }
            FrameCmd::Duplicate { from, to } => {
                self.transact(|d| d.duplicate_frames(from, to))?;
                // A copied frame goes to its copy; frames after the run move along with it.
                if self.frame >= from {
                    self.frame += to - from + 1;
                }
            }
            FrameCmd::MoveRun { from, to, at } => {
                self.transact(|d| d.move_frames(from, to, at))?;
                self.frame = crate::frame_block::after_block_move(self.frame, (from, to), at);
            }
            // The same rate again changes nothing, and so makes no step.
            FrameCmd::SetRate { fps } => self.transact(|d| d.set_frame_rate(fps))?,
        }
        Ok(Outcome::CHANGED)
    }
}
