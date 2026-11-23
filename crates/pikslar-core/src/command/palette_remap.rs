//! The lossless remaps and Replace colour.
//!
//! A remap changes ids and no colour, so every composited frame is the same afterwards. It
//! reaches every palette image whatever its layer's lock, since a layer left on the old ids
//! would show the wrong colours. Replace colour does change what pixels show, so it is refused
//! where painting would be: it changes nothing if any layer it would change is locked (itself or
//! through a group), is a reference, or shares a drawing with a locked layer.

use super::{LayerId, Outcome};
use crate::document::{ColorMode, LimitError};
use crate::editor::Editor;
use crate::palette_ops::remap::{plan, IdMap, RemapKind};
use crate::palette_ops::rewrite::{cels_showing, holds_ids, image_shows, rewrite, used_ids, Scope};
use std::collections::HashSet;
use std::sync::Arc;

impl Editor {
    /// One step. Session brushes follow the new ids afterwards; they are view state, so undo
    /// leaves them, and the indexed rule keeps them painting the right colour.
    pub(super) fn remap_palette(&mut self, kind: RemapKind) -> Result<Outcome, LimitError> {
        let mut applied: Option<IdMap> = None;
        self.transact(|d| {
            let plan = plan(kind, &d.colors, || used_ids(d));
            if plan.table != *d.colors {
                d.colors = Arc::new(plan.table);
            }
            rewrite(d, plan.map.as_slice(), Scope::Everywhere);
            applied = Some(plan.map);
            Ok(())
        })?;
        if let Some(map) = applied {
            self.brushes.remap_ids(&map);
        }
        Ok(Outcome::CHANGED)
    }

    /// One step, or none if no pixel shows `from`. The ids are the colours' own: the colour
    /// behind `to` is not looked at, so `to` may be 0, which clears the pixels.
    pub(super) fn replace_color(
        &mut self,
        from: u32,
        to: u32,
        layer: Option<LayerId>,
    ) -> Result<Outcome, LimitError> {
        let scope = match layer {
            Some(layer) => Scope::Layer(self.index_of(layer)?),
            None => Scope::Everywhere,
        };
        let len = self.doc.colors.len() as u32;
        if from == 0 || from >= len || to >= len {
            return Err(LimitError::NoSuchItem);
        }
        if let Scope::Layer(index) = scope {
            if !holds_ids(&self.doc.layers()[index]) {
                return Err(LimitError::NoSuchItem);
            }
        }
        if from == to {
            return Ok(Outcome::UNCHANGED);
        }
        self.refuse_replacing(from, scope)?;
        let map = IdMap::replacing(len as usize, from, to);
        self.transact(|d| {
            rewrite(d, map.as_slice(), scope);
            Ok(())
        })?;
        Ok(Outcome::CHANGED)
    }

    /// The paint rule for Replace colour. Only layers that show `from` are asked: a locked layer
    /// that has none of that colour loses nothing. Anything it would change that painting could
    /// not is a refusal of the whole command, so a drawing is never half replaced.
    fn refuse_replacing(&self, from: u32, scope: Scope) -> Result<(), LimitError> {
        let shown = cels_showing(&self.doc, from, scope);
        // Everywhere asks every layer that shows it. On one layer, only that layer is asked for
        // its own lock; the layers linked to it are asked below, as a link to a locked layer is
        // when painting.
        let own = |layer: &usize| !matches!(scope, Scope::Layer(own) if own != *layer);
        let layers = shown.iter().map(|&(layer, _)| layer).filter(own);
        if let Some(refusal) = layers.filter_map(|l| self.paint_refusal(l)).next() {
            return Err(refusal);
        }
        let shared = shown.iter().find_map(|&(l, f)| self.shared_refusal(l, f));
        shared.map_or(Ok(()), Err)?;
        match scope {
            Scope::Everywhere => self.refuse_locked_tiles(from),
            Scope::Layer(_) => Ok(()),
        }
    }

    /// Replacing everywhere also changes the images of palette tilesets, so every tilemap layer
    /// that places a tile showing `from` shows something else afterwards. Locked, that layer is
    /// not to change: the replacement is refused. A locked layer that places none of those
    /// tiles loses nothing.
    fn refuse_locked_tiles(&self, from: u32) -> Result<(), LimitError> {
        for (index, layer) in self.doc.layers().iter().enumerate() {
            let Some(set) = layer.tileset().and_then(|id| self.doc.tileset(id).ok()) else {
                continue;
            };
            if set.mode() != ColorMode::Palette || !self.doc.locked_through(index) {
                continue;
            }
            let changing: HashSet<u32> = set
                .tiles()
                .iter()
                .filter(|t| image_shows(&t.image, from))
                .map(|t| t.id.get())
                .collect();
            if !changing.is_empty() && self.doc.layer_places_any(index, &changing) {
                return Err(LimitError::Locked);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "palette_remap_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "palette_remap_tile_tests.rs"]
mod tile_tests;
