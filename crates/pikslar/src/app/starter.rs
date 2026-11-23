//! The sprite and palette the app starts with, and what a blank document is given to paint with.

use pikslar_core::{rgba, ColorMode, Document, Editor};

/// Sixteen starting colors: neutrals, then warm, green, cool and violet ramps.
pub(super) const STARTER_PALETTE: [u32; 16] = [
    rgba(15, 14, 20, 255),
    rgba(58, 53, 68, 255),
    rgba(123, 117, 137, 255),
    rgba(201, 194, 204, 255),
    rgba(245, 239, 230, 255),
    rgba(122, 46, 58, 255),
    rgba(200, 70, 62, 255),
    rgba(240, 138, 75, 255),
    rgba(247, 201, 107, 255),
    rgba(63, 107, 79, 255),
    rgba(127, 174, 90, 255),
    rgba(47, 92, 138, 255),
    rgba(79, 163, 199, 255),
    rgba(167, 224, 224, 255),
    rgba(107, 63, 124, 255),
    rgba(199, 123, 176, 255),
];

/// Builds the sprite shown at startup.
pub(crate) fn starter_document(size: usize) -> Document {
    let fallback = size.clamp(1, pikslar_core::MAX_DIMENSION);
    let mut doc = match Document::new(fallback, fallback, ColorMode::Palette) {
        Ok(doc) => doc,
        Err(_) => {
            return Document::new(1, 1, ColorMode::Palette).unwrap_or_else(|_| unreachable!())
        }
    };
    add_starter_palette(&mut doc);
    doc
}

/// Gives a document with no swatches, such as an RGBA import, something to paint with.
pub(super) fn add_starter_palette(doc: &mut Document) {
    if !doc.colors.swatches().is_empty() {
        return;
    }
    let colors = std::sync::Arc::make_mut(&mut doc.colors);
    for color in STARTER_PALETTE {
        // A table with no swatches always has room for sixteen colors.
        let _ = colors.add_swatch(color);
    }
}

pub(super) fn first_swatch_color(editor: &Editor) -> u32 {
    let colors = &editor.doc().colors;
    colors
        .swatches()
        .first()
        .map_or(editor.color, |&id| colors.color(id))
}
