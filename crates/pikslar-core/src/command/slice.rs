//! Slice commands. Each is one step; a slice is set whole, so a name typed into a dialog is one
//! change.

use super::{Outcome, SliceCmd};
use crate::document::LimitError;
use crate::editor::Editor;

impl Editor {
    pub(super) fn apply_slice(&mut self, command: SliceCmd) -> Result<Outcome, LimitError> {
        match command {
            SliceCmd::Add { slice } => {
                let index = self.transact(|d| d.add_slice(*slice))?;
                Ok(Outcome {
                    changed: true,
                    created: Some(index as u32),
                })
            }
            SliceCmd::Set { index, slice } => {
                let slice = slice.checked()?;
                // A slice set as it was changes nothing, and so makes no step.
                self.transact(|d| {
                    let found = d.slices.get_mut(index).ok_or(LimitError::NoSuchItem)?;
                    *found = slice;
                    Ok(())
                })?;
                Ok(Outcome::CHANGED)
            }
            SliceCmd::Remove { index } => {
                if index >= self.doc.slices.len() {
                    return Err(LimitError::NoSuchItem);
                }
                self.transact(|d| {
                    d.slices.remove(index);
                    Ok(())
                })?;
                Ok(Outcome::CHANGED)
            }
        }
    }
}
