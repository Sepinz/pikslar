//! The rules every document keeps about its tilemap layers, checked in debug builds after every
//! command, as `check_links` does for links.
//!
//! - T1: a tilemap layer names a tileset the document has.
//! - T2: the layer's colour mode is its tileset's.
//! - T3: every drawn cell of a map cel names a tile its tileset has.
//! - T4: an empty cell is exactly 0, with no flag bits.
//! - T5: a diagonal flip is set only where the tiles are square.
//! - T6: every terrain set has one slot for each of its kind's, a distinct id, and fills its
//!   slots only with tiles its tileset has.
//!
//! The rule that links join tilemap layers of one tileset is L3 of `check_links`.

use super::cell::MapCell;
use crate::document::Document;
use crate::image::Image;
use crate::layer::LayerKind;
use crate::tile::{RawTile, TILE_AREA};
use crate::tileset::Tileset;
use std::collections::HashSet;

/// A broken rule of the tilemap layers, naming the layer by its place from the bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TilemapFault {
    /// T1.
    NoTileset { layer: usize },
    /// T2.
    Mode { layer: usize },
    /// T3.
    UnknownTile { layer: usize, frame: usize },
    /// T4.
    FlagsOnEmpty { layer: usize, frame: usize },
    /// T5.
    DiagonalOnOblong { layer: usize, frame: usize },
    /// T6, naming the tileset by its id.
    Terrain { tileset: u32 },
}

impl std::fmt::Display for TilemapFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TilemapFault::NoTileset { layer } => write!(f, "T1: layer {layer} has no tileset"),
            TilemapFault::Mode { layer } => {
                write!(
                    f,
                    "T2: layer {layer} differs in colour mode from its tileset"
                )
            }
            TilemapFault::UnknownTile { layer, frame } => {
                write!(
                    f,
                    "T3: layer {layer}, frame {frame} places a tile that is gone"
                )
            }
            TilemapFault::FlagsOnEmpty { layer, frame } => {
                write!(
                    f,
                    "T4: layer {layer}, frame {frame} has flags on an empty cell"
                )
            }
            TilemapFault::DiagonalOnOblong { layer, frame } => {
                write!(f, "T5: layer {layer}, frame {frame} turns oblong tiles")
            }
            TilemapFault::Terrain { tileset } => {
                write!(f, "T6: a terrain set of tileset {tileset} is unsound")
            }
        }
    }
}

/// The first stored value of the image that is not 0 and that `visit` answers `true` for, tried
/// tile by tile.
pub(crate) fn find_cell(image: &Image, mut visit: impl FnMut(u32) -> bool) -> Option<u32> {
    let (x0, y0, wide, high) = image.table();
    let mut raw: RawTile = [0; TILE_AREA];
    for ty in y0..y0 + high as i32 {
        for tx in x0..x0 + wide as i32 {
            let Some(tile) = image.tile(tx, ty) else {
                continue;
            };
            tile.decode(&mut raw);
            if let Some(&found) = raw.iter().find(|&&v| v != 0 && visit(v)) {
                return Some(found);
            }
        }
    }
    None
}

/// The first rule of T3 to T5 the map image breaks, as a fault for the given place.
fn image_fault(set: &Tileset, image: &Image, at: (usize, usize)) -> Option<TilemapFault> {
    let (layer, frame) = at;
    let square = set.tile_size().0 == set.tile_size().1;
    let mut fault = None;
    find_cell(image, |bits| {
        let cell = MapCell::from_bits(bits);
        fault = if !cell.is_canonical() {
            Some(TilemapFault::FlagsOnEmpty { layer, frame })
        } else if cell.tile().is_some_and(|t| set.position(t).is_none()) {
            Some(TilemapFault::UnknownTile { layer, frame })
        } else if !square && cell.orientation().swaps_axes() {
            Some(TilemapFault::DiagonalOnOblong { layer, frame })
        } else {
            None
        };
        fault.is_some()
    })?;
    fault
}

/// True if the tileset's terrain sets keep T6.
fn terrains_sound(set: &Tileset) -> bool {
    let sets = set.terrains();
    let mut ids = HashSet::new();
    sets.iter().all(|t| {
        let mut tiles = t.slots().iter().flatten();
        ids.insert(u32::from(t.id()))
            && t.slots().len() == t.kind().slot_count()
            && tiles.all(|&tile| set.position(tile).is_some())
    })
}

impl Document {
    /// Checks the rules every document keeps about its tilemap layers and tilesets, T1 to T6
    /// above, and names the first one broken. One pass over each distinct map drawing and each
    /// terrain set; nothing for a document without tilesets.
    pub fn check_tilemaps(&self) -> Result<(), TilemapFault> {
        if let Some(set) = self.tilesets.iter().find(|set| !terrains_sound(set)) {
            return Err(TilemapFault::Terrain {
                tileset: set.id().get(),
            });
        }
        for (layer, l) in self.layers.iter().enumerate() {
            let LayerKind::Tilemap(id) = l.kind else {
                continue;
            };
            let set = self.tilesets.get(id);
            let set = set.ok_or(TilemapFault::NoTileset { layer })?;
            if set.mode() != l.mode {
                return Err(TilemapFault::Mode { layer });
            }
            let mut seen = HashSet::new();
            for (frame, cel) in l.cels().iter().enumerate() {
                if !seen.insert(cel.image_id()) {
                    continue;
                }
                if let Some(fault) = image_fault(set, cel.image(), (layer, frame)) {
                    return Err(fault);
                }
            }
        }
        Ok(())
    }
}
