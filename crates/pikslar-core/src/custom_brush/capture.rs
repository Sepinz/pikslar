//! The editor's brush calls. All view state: none of them is an undo step or marks the file.

use super::brush::{BrushPaint, Handle};
use super::state::BrushChoice;
use crate::document::LimitError;
use crate::editor::Editor;
use crate::Turn;
use std::sync::Arc;

impl Editor {
    /// Chooses the brush. A custom one is also remembered as recent.
    pub fn set_brush(&mut self, choice: BrushChoice) {
        self.brushes.set_active(choice);
    }

    /// Makes a brush from the floating pixels, or else from the active cel inside the selection,
    /// and chooses it. Refused with `NothingToCapture` when nothing is selected or the area is
    /// clear, and `BrushTooLarge` past 256 by 256.
    pub fn capture_brush(&mut self, paint: BrushPaint) -> Result<(), LimitError> {
        // The size is checked from the selection's bounds, before any pixel is copied.
        let (w, h) = self.selection_size().ok_or(LimitError::NothingToCapture)?;
        if w > super::MAX_BRUSH_SIDE || h > super::MAX_BRUSH_SIDE {
            return Err(LimitError::BrushTooLarge);
        }
        let clip = self.copy_selected().ok_or(LimitError::NothingToCapture)?;
        let brush = super::Brush::from_clip("", &clip, paint)?;
        let brush = brush.with_name(&self.brushes.next_name());
        self.brushes
            .set_active(BrushChoice::Custom(Arc::new(brush)));
        Ok(())
    }

    /// Flips or quarter-turns the active custom brush. Returns false if none is active.
    pub fn turn_brush(&mut self, turn: Turn) -> bool {
        let Some(brush) = self.brushes.active_custom() else {
            return false;
        };
        let turned = brush.turned_by(turn);
        self.brushes.replace_active(turned);
        true
    }

    /// Switches the active custom brush between its own colours and its shape.
    pub fn set_brush_paint(&mut self, paint: BrushPaint) -> bool {
        let Some(brush) = self.brushes.active_custom() else {
            return false;
        };
        let changed = brush.with_paint(paint);
        self.brushes.replace_active(changed);
        true
    }

    /// Moves the active custom brush's handle.
    pub fn set_brush_handle(&mut self, handle: Handle) -> bool {
        let Some(brush) = self.brushes.active_custom() else {
            return false;
        };
        let changed = brush.with_handle(handle);
        self.brushes.replace_active(changed);
        true
    }
}

impl Editor {
    /// Takes over the brushes of another editor, as when a document is opened: the brush
    /// outlives the document it was captured from.
    pub fn use_brushes(&mut self, brushes: super::BrushState) {
        self.brushes = brushes;
    }
}
