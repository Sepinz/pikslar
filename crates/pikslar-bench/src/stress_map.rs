//! The tilemap stress document of `docs/creative-tools-plan.md`, section 9: 256 by 256 cells of
//! 8 by 8 tiles (a 2,048 pixel square canvas), one tileset of 2,000 tiles, three tilemap layers
//! sharing it over one paint layer, 8 frames, two layers held. `pikslar-bench make-stress-map`
//! writes it; the M10 bench lines and the stress-map limits are measured on it.
//!
//! Layer 0 is the paint layer and layer 3 the top map; both hold frame 1's drawing through all
//! 8 frames. Layers 1 and 2 have a map of their own in every frame. In frame 1, layer 1 places
//! [`EDITED`] 4,000 times, in every sixteenth cell, and no other cell of any map places it.
//! Layer 2 turns its tiles every way; layer 3 leaves about a quarter of its cells empty.

use crate::workloads::{sprite_layer, Rng};
use pikslar_core::tileset::TilesetId;
use pikslar_core::{Cel, ColorMode, Document, Grid};
use std::sync::Arc;

pub const CELLS: usize = 256;
pub const TILE: usize = 8;
pub const SIDE: usize = CELLS * TILE;
pub const TILES: u32 = 2_000;
pub const FRAMES: usize = 8;
/// The tile that layer 1's first frame places [`PLACED`] times.
pub const EDITED: u32 = 7;
pub const PLACED: usize = 4_000;
/// Flag bits of a map cell: diagonal, vertical and horizontal flips, as Tiled has them.
const FLIPS: u32 = 0xe000_0000;

/// A tile's pixels: a colour of its own in a diagonal band pattern, with clear pixels so that
/// the layers under it show through.
pub fn tile_pixels(n: u32) -> Vec<u32> {
    (0..TILE * TILE)
        .map(|i| {
            let (x, y) = ((i % TILE) as u32, (i / TILE) as u32);
            if (x + 2 * y + n) % 5 == 0 {
                return 0;
            }
            let shade = (x + y) * 9;
            0xff00_0000 | ((n.wrapping_mul(2_654_435_761) & 0x00ff_ffff) ^ shade)
        })
        .collect()
}

/// xorshift32: the same maps on every machine.
fn next(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

/// A random tile other than [`EDITED`].
fn other_tile(seed: &mut u32) -> u32 {
    match 1 + next(seed) % TILES {
        EDITED => EDITED + 1,
        id => id,
    }
}

/// The cells of `layer`'s map in `frame`, as raw map cell values.
pub fn map_cells(layer: usize, frame: usize) -> Vec<u32> {
    let mut seed = 0x2545_f491 ^ (layer as u32 * 0x9e37) ^ (frame as u32).wrapping_mul(0x7f4a_7c15);
    (0..CELLS * CELLS)
        .map(|i| match (layer, frame) {
            (1, 0) if i % 16 == 0 && i / 16 < PLACED => EDITED,
            (2, _) => other_tile(&mut seed) | (next(&mut seed) & FLIPS),
            (3, _) if next(&mut seed) % 4 == 0 => 0,
            _ => other_tile(&mut seed),
        })
        .collect()
}

fn tileset(doc: &mut Document) -> TilesetId {
    let set = doc
        .add_tileset("Stress", ColorMode::Rgba, (TILE, TILE))
        .expect("room for a tileset");
    for n in 1..=TILES {
        let image = Grid::from_cells(TILE, TILE, &tile_pixels(n)).into_image();
        doc.add_tile(set, Arc::new(image)).expect("a tile fits");
    }
    set
}

pub fn stress_map_document() -> Document {
    // Layer 3 is held, so only its first map is kept.
    map_document(map_cells, &[0, 3])
}

/// A document like the stress map whose map layer `layer` shows `cells(layer, frame)` in each
/// frame, with the layers `held` holding their first frame through all of them.
pub fn map_document(cells: impl Fn(usize, usize) -> Vec<u32>, held: &[usize]) -> Document {
    let mut doc = Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid size");
    let set = tileset(&mut doc);
    for frame in 1..FRAMES {
        doc.add_frame(frame, false).expect("room");
    }
    let paint = sprite_layer(SIDE, SIDE, &mut Rng(5));
    doc.set_cel(0, 0, Grid::from_cells(SIDE, SIDE, &paint))
        .expect("canvas size");
    for layer in 1..=3 {
        doc.add_tilemap_layer(layer, set).expect("room");
        let frames = if held.contains(&layer) { 1 } else { FRAMES };
        for frame in 0..frames {
            let map = Cel::new(Grid::from_cells(CELLS, CELLS, &cells(layer, frame)));
            doc.put_cel(layer, frame, map).expect("a map fits");
        }
    }
    for &layer in held {
        doc.hold_across(layer, 0, 0, FRAMES - 1).expect("a hold");
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::tileset::TileId;
    use pikslar_core::LayerKind;

    fn edited_tile() -> TileId {
        TileId::new(EDITED).expect("a tile id")
    }

    #[test]
    fn the_stress_map_has_the_shape_the_plan_gives() {
        let doc = stress_map_document();
        assert_eq!((doc.width, doc.height), (2048, 2048));
        assert_eq!(doc.frames.len(), FRAMES);
        assert_eq!(doc.tilesets.len(), 1);
        let set = doc.tilesets.iter().next().expect("a tileset");
        assert_eq!((set.len(), set.tile_size()), (2_000, (8, 8)));
        let kinds: Vec<bool> = doc.layers().iter().map(|l| l.is_tilemap()).collect();
        assert_eq!(kinds, [false, true, true, true]);
        assert!(doc.layers()[1..]
            .iter()
            .all(|l| l.kind == LayerKind::Tilemap(set.id())));
        assert_eq!(doc.check_tilemaps(), Ok(()));
        assert_eq!(doc.check_links(), Ok(()));
    }

    #[test]
    fn two_layers_are_held_and_the_edited_tile_is_placed_4000_times() {
        let doc = stress_map_document();
        let held = |l: usize| (1..FRAMES).all(|f| doc.layers()[l].link(f) != 0);
        assert!(held(0) && held(3));
        assert!(!held(1) && !held(2));
        let set = doc.tilesets.iter().next().expect("a tileset").id();
        assert_eq!(doc.cells_using(set, edited_tile()), PLACED);
        let distinct = doc.layers()[2].cel(0) != doc.layers()[2].cel(1);
        assert!(distinct, "layer 2 has a map of its own in each frame");
    }

    #[test]
    fn tiles_differ_and_have_clear_pixels() {
        let (a, b) = (tile_pixels(1), tile_pixels(2));
        assert_ne!(a, b);
        assert!(a.contains(&0) && a.iter().any(|&p| p != 0));
    }
}
