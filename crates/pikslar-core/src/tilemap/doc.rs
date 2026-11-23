//! What a document does with its tilemap layers. Commands run these inside a step, so a refusal
//! leaves the document as it was.

use super::cell::MapCell;
use super::check::find_cell;
use crate::document::{Document, LimitError};
use crate::image::Image;
use crate::layer::{Cel, Layer, LayerKind};
use crate::tileset::{TileId, TilesetId};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// How many cells of this map drawing place the tile.
fn cells_placing(image: &Image, tile: TileId) -> usize {
    let mut count = 0;
    find_cell(image, |bits| {
        count += usize::from(MapCell::from_bits(bits).tile() == Some(tile));
        false
    });
    count
}

impl Document {
    /// Inserts a tilemap layer of `tileset`, every frame empty, at `index` from the bottom. Its
    /// colour mode is the tileset's.
    pub fn add_tilemap_layer(
        &mut self,
        index: usize,
        tileset: TilesetId,
    ) -> Result<(), LimitError> {
        let mode = self.tileset(tileset)?.mode();
        let name = format!("Tilemap {}", self.layers.len() + 1);
        let mut layer = Layer::new(name, mode);
        layer.kind = LayerKind::Tilemap(tileset);
        self.insert_blank_layer(index, layer)
    }

    /// Makes the tilemap layer at `index` use a copy of its tileset: the same tile ids and
    /// images, shared until edited, under the next tileset id. Cels of the layer leave any link
    /// that reaches another layer, since a link needs one tileset. Returns the copy's id.
    pub fn copy_tileset_for(&mut self, index: usize) -> Result<TilesetId, LimitError> {
        let layer = self.layers.get(index).ok_or(LimitError::NoSuchItem)?;
        let old = layer.tileset().ok_or(LimitError::NotTilemap)?;
        let source = self.tileset(old)?;
        // A file stores the copy's tiles on their own, though they share images here.
        self.check_tile_room(0, source.stored_bytes())?;
        let name = format!("{} copy", source.name());
        let copy = Arc::make_mut(&mut self.tilesets).copy(old, &name)?;
        self.unlink_from_other_layers(index);
        self.layers[index].kind = LayerKind::Tilemap(copy);
        Ok(copy)
    }

    /// How many cells place this tile, over every layer and frame. A drawing that frames, holds
    /// or links share is counted once.
    pub fn cells_using(&self, tileset: TilesetId, tile: TileId) -> usize {
        let mut seen = HashSet::new();
        let maps = self.layers.iter().filter(|l| l.tileset() == Some(tileset));
        let cels = maps.flat_map(|l| l.cels().iter());
        let distinct = cels.filter(|cel| seen.insert(cel.image_id()));
        distinct.map(|cel| cells_placing(cel.image(), tile)).sum()
    }

    /// Makes every cell that places this tile empty, on every layer and frame, so a removed tile
    /// leaves nothing that names it. Drawings that were shared stay shared. Returns how many
    /// cells were emptied.
    pub(crate) fn empty_cells_of(&mut self, tileset: TilesetId, tile: TileId) -> usize {
        let mut places: Vec<(usize, usize, Cel)> = Vec::new();
        for (index, layer) in self.layers.iter().enumerate() {
            if layer.tileset() == Some(tileset) {
                let cels = layer.cels().iter().enumerate();
                places.extend(cels.map(|(frame, cel)| (index, frame, cel.clone())));
            }
        }
        // Keyed by the drawing's address, which the clones above keep from being reused.
        let mut made: HashMap<usize, Option<Arc<Image>>> = HashMap::new();
        let mut emptied = 0;
        for (index, frame, cel) in places {
            let new = made.entry(cel.image_id()).or_insert_with(|| {
                let count = cells_placing(cel.image(), tile);
                emptied += count;
                (count > 0).then(|| Arc::new(without_tile(cel.image(), tile)))
            });
            if let Some(image) = new {
                // A hold's frames and a link's cels all get the one new drawing, so they still
                // show one.
                let put = self.layers[index].put_cel(frame, cel.with_image(image.clone()));
                debug_assert!(put.is_ok(), "the frame was listed from this layer");
            }
        }
        emptied
    }
}

/// The map drawing with every cell that places the tile emptied.
fn without_tile(image: &Image, tile: TileId) -> Image {
    image.map_cells(|bits| {
        if MapCell::from_bits(bits).tile() == Some(tile) {
            0
        } else {
            bits
        }
    })
}
