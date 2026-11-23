//! Drawing, selection and floating-pixel commands.

use super::{did, DrawCmd, FloatingCmd, Outcome, SelectionCmd};
use crate::document::LimitError;
use crate::editor::Editor;

impl Editor {
    pub(super) fn apply_draw(&mut self, command: DrawCmd) -> Result<Outcome, LimitError> {
        let revision = self.revision;
        match command {
            DrawCmd::Begin { tool, x, y } => self.begin(tool, x, y)?,
            DrawCmd::To { x, y } => self.drag(x, y),
            DrawCmd::Commit => self.finish_stroke()?,
            DrawCmd::Cancel => self.cancel(),
            DrawCmd::Stroke { tool, path } => self.whole_stroke(tool, &path)?,
            DrawCmd::EraseCell { x, y } => self.erase_cell(x, y),
            DrawCmd::DeleteSelected => self.delete_selected(),
        }
        Ok(did(self.revision != revision))
    }

    /// A stroke given all at once, as a script would give it.
    fn whole_stroke(
        &mut self,
        tool: crate::editor::Tool,
        path: &[(i32, i32)],
    ) -> Result<(), LimitError> {
        let Some((&(x, y), rest)) = path.split_first() else {
            return Ok(());
        };
        self.begin(tool, x, y)?;
        for &(x, y) in rest {
            self.drag(x, y);
        }
        self.finish_stroke()
    }

    pub(super) fn apply_selection(&mut self, command: SelectionCmd) -> Result<Outcome, LimitError> {
        let version = self.selection_version;
        match command {
            SelectionCmd::Select { shape, op } => self.select(*shape, op),
            SelectionCmd::Temporary { shape } => self.select_temporarily(*shape),
            SelectionCmd::All => self.select_all(),
            SelectionCmd::None => self.deselect(),
            SelectionCmd::Invert => self.invert_selection(),
        }
        Ok(did(self.selection_version != version))
    }

    pub(super) fn apply_floating(&mut self, command: FloatingCmd) -> Result<Outcome, LimitError> {
        let seen = |e: &Editor| {
            (
                e.revision,
                e.selection_version,
                e.selection_moved,
                e.floating_epoch,
                e.floating_stamp(),
            )
        };
        let before = seen(self);
        match command {
            FloatingCmd::Lift { copy } => return Ok(did(self.lift(copy))),
            FloatingCmd::Move { dx, dy, join } => self.nudge_floating(dx, dy, join),
            FloatingCmd::Scale { x, y, w, h } => self.scale_floating(x, y, w, h),
            FloatingCmd::FlipHorizontal => self.flip_horizontal(),
            FloatingCmd::FlipVertical => self.flip_vertical(),
            FloatingCmd::Rotate { clockwise } => self.rotate(clockwise),
            FloatingCmd::Anchor => self.anchor(),
            FloatingCmd::Revert => self.revert_floating(),
            FloatingCmd::Paste { clip, x, y } => self.paste(&clip, x, y)?,
            FloatingCmd::ReplacePasted { clip } => self.replace_pasted(&clip)?,
            FloatingCmd::GestureBegin => self.gesture_begin(),
            FloatingCmd::GestureEnd => self.gesture_end(),
            FloatingCmd::GestureCancel => self.gesture_cancel(),
        }
        Ok(did(seen(self) != before))
    }
}
