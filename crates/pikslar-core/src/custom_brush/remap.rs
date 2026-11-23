//! Session brushes follow a palette remap. A brush keeps the ids of the layer it was captured
//! from only as hints (the indexed rule paints a hint only while it still shows the brush's
//! colour), so moving them keeps the brush painting by id instead of by colour lookup.

use super::{BrushChoice, BrushState};
use crate::custom_brush::Brush;
use crate::palette_ops::remap::IdMap;
use std::sync::Arc;

impl BrushState {
    /// Moves the palette ids of the active brush and the recent ones through `map`. A brush
    /// that is both active and recent stays one brush, and one that holds no ids is untouched.
    /// An id the remap removed becomes 0, which never shows a brush's colour, so it is ignored.
    pub(crate) fn remap_ids(&mut self, map: &IdMap) {
        let mut done: Vec<(Arc<Brush>, Arc<Brush>)> = Vec::new();
        let mut moved = |brush: &Arc<Brush>| -> Arc<Brush> {
            if let Some((_, new)) = done.iter().find(|(old, _)| Arc::ptr_eq(old, brush)) {
                return new.clone();
            }
            let new = with_ids_moved(brush, map);
            done.push((brush.clone(), new.clone()));
            new
        };
        if let BrushChoice::Custom(brush) = &self.active {
            self.active = BrushChoice::Custom(moved(brush));
        }
        for brush in &mut self.recent {
            *brush = moved(brush);
        }
    }
}

fn with_ids_moved(brush: &Arc<Brush>, map: &IdMap) -> Arc<Brush> {
    let Some(ids) = brush.ids.as_deref() else {
        return brush.clone();
    };
    let moved: Vec<u32> = ids.iter().map(|&id| map.get(id)).collect();
    if moved == ids {
        return brush.clone();
    }
    Arc::new(Brush {
        ids: Some(moved.into()),
        ..(**brush).clone()
    })
}
