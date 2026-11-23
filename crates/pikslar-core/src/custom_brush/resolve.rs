//! The indexed-mode rule, the same one paste uses (`Editor::clip_cells` in `floating.rs`).

use crate::color::ColorTable;
use crate::document::LimitError;
use std::sync::Arc;

/// Cell values for a palette layer, in source order. A pixel keeps the id it was captured with
/// while that id still shows its colour; otherwise it takes the colour's id, which is added to
/// the table if new. Returns the grown table only when something was added. `table` is not
/// changed, so a refusal leaves everything as it was.
pub(super) fn resolve_cells(
    colors: &[u32],
    ids: Option<&[u32]>,
    table: &Arc<ColorTable>,
) -> Result<(Vec<u32>, Option<Arc<ColorTable>>), LimitError> {
    let mut grown = table.clone();
    let mut cells = Vec::with_capacity(colors.len());
    for (i, &color) in colors.iter().enumerate() {
        if color == 0 {
            cells.push(0);
            continue;
        }
        let kept = ids.map(|ids| ids[i]).filter(|&id| grown.color(id) == color);
        let cell = match kept.or_else(|| grown.id_for_existing(color)) {
            Some(id) => id,
            None => Arc::make_mut(&mut grown).id_for(color)?,
        };
        cells.push(cell);
    }
    let changed = !Arc::ptr_eq(table, &grown);
    Ok((cells, changed.then_some(grown)))
}
