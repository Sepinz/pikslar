//! Loading colours into the palette: one step, validated on a copy of the table before the
//! document is touched.

use super::Outcome;
use crate::document::LimitError;
use crate::editor::Editor;
use crate::palette_ops::load::{apply, LoadHow};
use std::sync::Arc;

impl Editor {
    /// No step if the palette would come out as it is. A refusal changes nothing, and does not
    /// even set aside an open drag or the redo history.
    pub(super) fn load_palette(
        &mut self,
        colors: &[u32],
        how: LoadHow,
    ) -> Result<Outcome, LimitError> {
        let mut table = (*self.doc.colors).clone();
        apply(&mut table, colors, how)?;
        if table == *self.doc.colors {
            return Ok(Outcome::UNCHANGED);
        }
        self.transact(|d| {
            d.colors = Arc::new(table);
            Ok(())
        })?;
        Ok(Outcome::CHANGED)
    }
}

#[cfg(test)]
#[path = "palette_load_tests.rs"]
mod tests;
