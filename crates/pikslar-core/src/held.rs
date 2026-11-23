//! What a document holds in memory, each shared image and tile counted once: a measure a
//! host can take before and after work it does not trust, to bound how much the work grows
//! the document. It walks every cel and tile, so it is priced by [`Document::held_cost`].

use crate::document::Document;
use crate::layer::Cel;
use crate::tile::Tile;
use std::mem::size_of;
use std::sync::Arc;

/// What an image's shared allocation holds besides its table: the image and its two counts.
const IMAGE_HEADER: usize = 2 * size_of::<usize>() + size_of::<crate::image::Image>();

impl Document {
    /// Bytes the document's cels, their images and tiles, and its color table hold, with an
    /// image or tile shared by several cels counted once. Each image counts the shared
    /// allocation it lives in, with its two counts, so a run that gives many cels images of
    /// their own is measured for them. Blank tiles hold nothing.
    pub fn held_bytes(&self) -> usize {
        let mut images: Vec<&Cel> = self.layers.iter().flat_map(|l| l.cels()).collect();
        let cel_entries = images.len().saturating_mul(size_of::<Cel>());
        images.sort_unstable_by_key(|c| c.image_id());
        images.dedup_by_key(|c| c.image_id());
        let mut tiles: Vec<&Arc<Tile>> = images.iter().flat_map(|c| c.image().tiles()).collect();
        tiles.sort_unstable_by_key(|t| Arc::as_ptr(t));
        tiles.dedup_by_key(|t| Arc::as_ptr(t));
        let tile_bytes: usize = tiles.iter().map(|t| t.heap_size()).sum();
        let tables: usize = images.iter().map(|c| c.table_bytes()).sum();
        let headers = images.len().saturating_mul(IMAGE_HEADER);
        let colors = self.colors.len().saturating_mul(2 * size_of::<u32>());
        cel_entries
            .saturating_add(tables)
            .saturating_add(headers)
            .saturating_add(tile_bytes)
            .saturating_add(colors)
    }

    /// At most what [`Self::held_bytes`] costs to measure now, in cels and tiles visited: as
    /// though every cel had an image of its own covering the canvas.
    pub fn held_cost(&self) -> usize {
        let cels = self.layers.len().saturating_mul(self.frames.len());
        let tiles = (self.width / crate::tile::TILE + 1) * (self.height / crate::tile::TILE + 1);
        cels.saturating_mul(tiles.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use crate::document::{ColorMode, Document};
    use crate::grid::Grid;
    use crate::image::Image;
    use crate::layer::Cel;
    use std::mem::size_of;

    fn noisy(doc: &mut Document, layer: usize, frame: usize, seed: u32) {
        let (w, h) = (doc.width, doc.height);
        let px: Vec<u32> = (0..w * h)
            .map(|i| 0xff00_0000 | (i as u32).wrapping_mul(2_654_435_761) ^ seed)
            .collect();
        doc.set_cel(layer, frame, Grid::from_cells(w, h, &px))
            .expect("canvas size");
    }

    #[test]
    fn each_image_counts_its_shared_allocation() {
        let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("a canvas");
        let before = doc.held_bytes();
        doc.add_layer(1, ColorMode::Rgba).expect("room");
        let header = 2 * size_of::<usize>() + size_of::<Image>();
        assert_eq!(doc.held_bytes() - before, size_of::<Cel>() + header);
    }

    #[test]
    fn new_pixels_are_counted_and_shared_ones_once() {
        let mut doc = Document::new(64, 64, ColorMode::Rgba).expect("a canvas");
        let blank = doc.held_bytes();
        noisy(&mut doc, 0, 0, 1);
        let one = doc.held_bytes();
        // 16 raw tiles of 1 KiB each, at least.
        assert!(one >= blank + 16 * 1024, "{blank} then {one}");
        doc.add_frame(1, true).expect("room");
        let copied = doc.held_bytes();
        assert!(
            copied < one + 1024,
            "a copied frame shares its tiles: {one} then {copied}"
        );
        noisy(&mut doc, 0, 1, 2);
        assert!(doc.held_bytes() >= copied + 16 * 1024);
        assert!(doc.held_cost() >= 2);
    }
}
