//! Reference layers: a picture to draw from, shown on the screen and in nothing made from the
//! document. One drawing is held across every frame, so moving it moves it everywhere.

use crate::document::{ColorMode, Document, LimitError, MAX_LAYERS};
use crate::image::Image;
use crate::layer::{Cel, Layer, LayerKind};
use crate::tile::TILE_AREA;
use std::sync::Arc;

/// The widest and tallest a reference image may be.
pub const MAX_REFERENCE_SIDE: usize = 4096;
/// What every reference image together may take decoded: one image of the largest size.
pub const MAX_REFERENCE_BYTES: usize = MAX_REFERENCE_SIDE * MAX_REFERENCE_SIDE * 4;

/// What an image's tiles take when decoded, whether drawn on or not.
fn decoded_bytes(image: &Image) -> usize {
    let (_, _, wide, high) = image.table();
    wide * high * TILE_AREA * 4
}

impl Document {
    /// What the reference images take decoded, each counted once however many cels share it.
    pub fn reference_bytes(&self) -> usize {
        let mut seen: Vec<usize> = Vec::new();
        let references = self
            .layers
            .iter()
            .filter(|l| l.kind == LayerKind::Reference);
        let mut total = 0;
        for cel in references.flat_map(Layer::cels) {
            if !seen.contains(&cel.image_id()) {
                seen.push(cel.image_id());
                total += decoded_bytes(cel.image());
            }
        }
        total
    }

    /// Refuses a reference image of this size before anything is decoded or changed.
    pub fn check_reference(&self, width: usize, height: usize) -> Result<(), LimitError> {
        let side = 1..=MAX_REFERENCE_SIDE;
        if !side.contains(&width) || !side.contains(&height) {
            return Err(LimitError::ReferenceTooLarge);
        }
        // Counted as whole tiles, as it will be stored.
        let tiles = |n: usize| n.div_ceil(crate::tile::TILE);
        let bytes = tiles(width) * tiles(height) * TILE_AREA * 4;
        match self.reference_bytes().saturating_add(bytes) <= MAX_REFERENCE_BYTES {
            true => Ok(()),
            false => Err(LimitError::ReferenceTooLarge),
        }
    }

    /// Puts a reference layer showing `image` at the canvas origin above layer `below`.
    pub(crate) fn place_reference(
        &mut self,
        below: usize,
        image: Arc<Image>,
        name: String,
    ) -> Result<usize, LimitError> {
        if self.layers.len() >= MAX_LAYERS {
            return Err(LimitError::TooManyLayers);
        }
        let (_, _, wide, high) = image.table();
        let side = |tiles: usize| tiles * crate::tile::TILE;
        self.check_reference(side(wide).max(1), side(high).max(1))?;
        let cel = Cel::blank().with_image(image);
        let mut layer = Layer::new(name, ColorMode::Rgba).with_cels(vec![cel; self.frames.len()]);
        layer.kind = LayerKind::Reference;
        layer.set_persisted_id(self.new_persisted_id());
        let at = below.saturating_add(1).min(self.layers.len());
        layer.depth = self.depth_for(at);
        self.layers.insert(at, layer);
        self.hold_references()?;
        Ok(at)
    }

    /// Makes every reference layer show its one drawing on every frame, held together. Run
    /// after anything that adds frames, which gives other layers blank or copied cels.
    pub fn hold_references(&mut self) -> Result<(), LimitError> {
        self.hold_references_counted().map(|_| ())
    }

    /// As `hold_references`, saying how many frames showed something else and were changed.
    pub fn hold_references_counted(&mut self) -> Result<usize, LimitError> {
        let (frames, mut changed) = (self.frames.len(), 0);
        for index in 0..self.layers.len() {
            let layer = &self.layers[index];
            if layer.kind != LayerKind::Reference {
                continue;
            }
            let cels = layer.cels();
            let drawing = cels
                .iter()
                .find(|c| !c.is_blank())
                .unwrap_or(&cels[0])
                .clone();
            let differ: Vec<usize> = (0..frames)
                .filter(|&f| !cels[f].looks_the_same(&drawing))
                .collect();
            changed += differ.len();
            for frame in differ {
                self.layers[index].put_cel(frame, drawing.clone())?;
            }
            let first = self.layers[index].link(0);
            if frames > 1 && (first == 0 || self.layers[index].links().any(|l| l != first)) {
                self.set_holds(index, &vec![1; frames])?;
            }
        }
        Ok(changed)
    }
}
