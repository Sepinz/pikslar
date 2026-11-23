//! Which tiles a tilemap layer places, for the commands that change what tiles look like.

use super::cell::MapCell;
use super::check::find_cell;
use crate::document::Document;
use std::collections::HashSet;

impl Document {
    /// True if some cell of the layer, in any frame, places one of these tiles. A drawing that
    /// frames hold or link share is read once. False for a layer that is not a tilemap layer.
    pub(crate) fn layer_places_any(&self, layer: usize, tiles: &HashSet<u32>) -> bool {
        let Some(l) = self.layers.get(layer).filter(|l| l.is_tilemap()) else {
            return false;
        };
        let mut seen = HashSet::new();
        l.cels()
            .iter()
            .filter(|cel| seen.insert(cel.image_id()))
            .any(|cel| {
                let places = |bits| {
                    MapCell::from_bits(bits)
                        .tile()
                        .is_some_and(|t| tiles.contains(&t.get()))
                };
                find_cell(cel.image(), places).is_some()
            })
    }
}
