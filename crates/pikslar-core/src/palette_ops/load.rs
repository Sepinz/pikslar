//! Loading a list of colours (from a palette file, or anywhere) into the palette.
//!
//! None of the three ways moves a pixel id, and none changes what an existing pixel looks like
//! except Load as colours, which does so on purpose: it is the palette swap.

use super::rewrite::used_ids;
use crate::color::{rgba, ColorTable};
use crate::document::{Document, LimitError};
use std::collections::HashSet;

/// What loading does with the colours it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadHow {
    /// The colours join the end of the swatches. A colour the table already has keeps its id,
    /// and one that is already a swatch stays where it is.
    Append,
    /// The swatches become exactly these colours, in this order, each once. Nothing is removed
    /// from the table, so a pixel whose colour is no longer a swatch keeps its id and its look:
    /// replacing the palette never changes a picture. Group members that are no longer swatches
    /// leave their groups, and a group left empty goes.
    Replace,
    /// Swatch `i` takes colour `i`: every id stays, so the art changes appearance, as a palette
    /// swap does. The list must be as long as the swatch list.
    AsColors,
}

/// Imported colours have no alpha: whatever came in is made opaque.
fn opaque(color: u32) -> u32 {
    let [r, g, b, _] = color.to_le_bytes();
    rgba(r, g, b, 255)
}

/// `table` after loading `colors`, or why it cannot be. Works on a copy the caller owns, so a
/// refusal halfway leaves nothing changed.
pub(crate) fn apply(
    table: &mut ColorTable,
    colors: &[u32],
    how: LoadHow,
) -> Result<(), LimitError> {
    if colors.is_empty() {
        return Err(LimitError::PaletteNoColors);
    }
    match how {
        LoadHow::Append => {
            for &color in colors {
                table.add_swatch(opaque(color))?;
            }
        }
        LoadHow::Replace => {
            let mut seen = HashSet::new();
            let mut swatches = Vec::with_capacity(colors.len());
            for &color in colors {
                let id = table.id_for(opaque(color))?;
                if seen.insert(id) {
                    swatches.push(id);
                }
            }
            table.set_swatches(swatches);
        }
        LoadHow::AsColors => {
            if colors.len() != table.swatches().len() {
                return Err(LimitError::PaletteSize);
            }
            for (id, &color) in table.swatches().to_vec().into_iter().zip(colors) {
                table.set_color(id, opaque(color));
            }
        }
    }
    Ok(())
}

/// How many colours some pixel shows that are not swatches. After Replace this is how many
/// colours the picture still uses that the palette no longer lists.
pub fn unlisted_used(doc: &Document) -> usize {
    let used = used_ids(doc);
    let swatches: HashSet<u32> = doc.colors.swatches().iter().copied().collect();
    (1..used.len() as u32)
        .filter(|&id| used[id as usize] && !swatches.contains(&id))
        .count()
}
