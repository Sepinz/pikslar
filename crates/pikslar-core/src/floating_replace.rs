//! Swapping the pixels of a paste that is still floating, as text typed after a paste does.
//!
//! Pasting again would anchor what is there, one undo step for every key. Swapping keeps the
//! paste open, so the whole of it, the moves between and the anchor at the end are one step.

use super::{clip_shape, Clip, FloatingSteps, Orientation};
use crate::document::LimitError;
use crate::editor::Editor;
use std::sync::Arc;

impl Editor {
    /// Puts `clip` in place of the pasted pixels, its top-left where theirs is now. Does nothing
    /// unless a paste is floating. A clip that cannot be had changes nothing. The colors the
    /// pixels it replaces brought to the palette are forgotten, as cancelling would.
    pub(crate) fn replace_pasted(&mut self, clip: &Clip) -> Result<(), LimitError> {
        let Some(f) = self.floating.as_ref().filter(|f| f.pasted) else {
            return Ok(());
        };
        let Some((x, y, _, _)) = self.selection.as_deref().map(|s| s.bounds()) else {
            return Ok(());
        };
        if !clip.is_well_formed() {
            return Err(LimitError::Dimensions);
        }
        // The table as it was before the paste, unless something has edited it since.
        let base = match &f.paste_colors {
            Some((before, after)) if Arc::ptr_eq(&self.doc.colors, after) => before.clone(),
            _ => self.doc.colors.clone(),
        };
        let (cells, colors) = self.clip_cells_in(clip, base.clone())?;
        let paste_colors = (!Arc::ptr_eq(&base, &colors)).then(|| (base, colors.clone()));
        self.doc.colors = colors;
        let shape = Arc::new(clip_shape(clip, x, y));
        let cells = Arc::new(cells);
        if let Some(f) = &mut self.floating {
            f.source = shape.clone();
            f.pixels = cells.clone();
            f.placed = cells;
            f.orientation = Orientation::default();
            f.steps = FloatingSteps::default();
            f.paste_colors = paste_colors;
            f.changed = true;
            f.stale = true;
        }
        self.selection = Some(shape);
        self.selection_version += 1;
        self.floating_epoch += 1;
        Ok(())
    }
}
