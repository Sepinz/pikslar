//! Tag and palette commands, and ending or abandoning a value in progress.

use super::{Outcome, StepCmd, TagCmd};
use crate::animation::{MAX_TAG_NAME, MAX_TAG_REPEAT};
use crate::document::LimitError;
use crate::editor::Editor;
use crate::steps::Dragged;

impl Editor {
    pub(super) fn apply_tag(&mut self, command: TagCmd) -> Result<Outcome, LimitError> {
        match command {
            TagCmd::Add { name, from, to } => {
                // Floating pixels go down first, so cancelling the name leaves them applied.
                self.doc.check_new_tag(from, to)?;
                self.anchor();
                let revision = self.revision;
                let tag = self.transact_joined(|d| d.add_tag(&name, from, to))?;
                self.mark_joined(Dragged::TagName(tag), revision);
                Ok(Outcome {
                    changed: true,
                    created: Some(tag as u32),
                })
            }
            TagCmd::SetName { tag, name } => self.set_tag_name(tag, &name),
            TagCmd::Remove { tag } => self.remove_tag(tag),
            TagCmd::SetRange { tag, from, to } => {
                self.transact(|d| d.set_tag_range(tag, from, to))?;
                Ok(Outcome::CHANGED)
            }
            TagCmd::CycleDirection { tag } => {
                self.transact(|d| {
                    let found = d.tags.get_mut(tag).ok_or(LimitError::NoSuchItem)?;
                    found.direction = found.direction.next();
                    Ok(())
                })?;
                Ok(Outcome::CHANGED)
            }
            TagCmd::SetRepeat { tag, repeat } => {
                let repeat = repeat.min(MAX_TAG_REPEAT);
                let found = self.doc.tags.get(tag).ok_or(LimitError::NoSuchItem)?;
                if found.repeat == repeat {
                    return Ok(Outcome::UNCHANGED);
                }
                self.transact(|d| {
                    d.tags[tag].repeat = repeat;
                    Ok(())
                })?;
                Ok(Outcome::CHANGED)
            }
        }
    }

    /// Typing joins one step, so a name typed in full is undone in one go.
    fn set_tag_name(&mut self, tag: usize, name: &str) -> Result<Outcome, LimitError> {
        let found = self.doc.tags.get(tag).ok_or(LimitError::NoSuchItem)?;
        let name: String = name.chars().take(MAX_TAG_NAME).collect();
        if found.name == name {
            return Ok(Outcome::UNCHANGED);
        }
        self.join_step(Dragged::TagName(tag), (self.layer, self.frame));
        self.doc.tags[tag].name = name;
        self.amended();
        Ok(Outcome::CHANGED)
    }

    fn remove_tag(&mut self, tag: usize) -> Result<Outcome, LimitError> {
        if tag >= self.doc.tags.len() {
            return Err(LimitError::NoSuchItem);
        }
        if self.joins(Dragged::TagName(tag)) {
            self.doc.tags.remove(tag);
            self.end_join();
            self.amended();
        } else {
            self.transact(|d| {
                d.tags.remove(tag);
                Ok(())
            })?;
        }
        Ok(Outcome::CHANGED)
    }

    pub(super) fn apply_step(&mut self, command: StepCmd) -> Result<Outcome, LimitError> {
        match command {
            StepCmd::EndDrag => self.finish_join(),
            StepCmd::AbortJoin => self.abort_join(),
            StepCmd::Undo => return Ok(super::did(self.undo())),
            StepCmd::Redo => return Ok(super::did(self.redo())),
            StepCmd::ScrubTo { depth } => {
                let before = self.history.undo_depth();
                self.scrub_to(depth);
                return Ok(super::did(self.history.undo_depth() != before));
            }
        }
        Ok(Outcome::UNCHANGED)
    }
}
