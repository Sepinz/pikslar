//! Terrain strokes (M11): map strokes that paint a terrain set's membership, the tiles following
//! from the set's rules. One undo step for the whole stroke, an `Edit::Tiles` on the map cel like
//! every map stroke, written through to the cels that hold or link the drawing. The points are in
//! `autotile::stroke` and the rules in `autotile::paint`; this is the part that touches the editor.
//!
//! **Alt-click** is `TileCmd::Begin` with [`TerrainShape::Retile`], not a command of its own: it
//! is a press of the terrain tool like a fill, so it takes the same lock and link rules from the
//! guard, the same undo step and the same refusals, and needs no case of its own anywhere.

use super::tile_stroke::mark_tiles;
use super::Editor;
use crate::autotile::paint::{At, Rule};
use crate::autotile::stroke::TerrainStroke;
use crate::command::{TerrainShape, TerrainTool};
use crate::document::LimitError;
use crate::grid::Cell;
use crate::image::{Image, TileKey};
use crate::tile::{RawTile, Tile, TILE, TILE_AREA};
use crate::tilemap::MapCell;
use std::sync::Arc;

/// What the terrain stroke in progress would change if committed now: the map cel, and its
/// cells in row order with the value each would hold. The canvas draws it as a stroke goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainPreview {
    pub layer: usize,
    pub frame: usize,
    pub cells: Vec<((i32, i32), MapCell)>,
}

impl Editor {
    /// Presses the terrain tool on the active tilemap layer. A fill and a retile are finished at
    /// once. Refused for a set of another tileset than the layer's, or one that is not there.
    pub(crate) fn begin_terrain(
        &mut self,
        tool: TerrainTool,
        x: i32,
        y: i32,
    ) -> Result<(), LimitError> {
        let tileset = self.active_map()?;
        if tool.tileset != tileset {
            return Err(LimitError::NoTileChosen);
        }
        let rule = Rule::new(self.doc.terrain(tileset, tool.set)?);
        let geo = self.map_geometry()?;
        self.anchor();
        self.cancel();
        let place = (self.layer, self.frame);
        let selection = self.selection.clone();
        let mut stroke = Box::new(TerrainStroke::new(
            place,
            tool,
            (geo, rule),
            selection,
            (x, y),
        ));
        let size = (self.doc.width, self.doc.height);
        let (damage, overlay) = (&mut self.damage, &self.overlay);
        let mut mark = |r| mark_tiles(damage, overlay, size, r);
        match tool.shape {
            TerrainShape::Fill => {
                let image = self.doc.layers[self.layer].cel(self.frame).image();
                stroke.fill(image, &mut mark);
            }
            TerrainShape::Retile => mark(stroke.start_rect()),
            TerrainShape::Brush | TerrainShape::Rect => stroke.press(&mut mark),
        }
        self.tiles.terrain = Some(stroke);
        if matches!(tool.shape, TerrainShape::Fill | TerrainShape::Retile) {
            self.commit_tile_stroke();
        }
        Ok(())
    }

    /// Continues the terrain stroke to a canvas position.
    pub(crate) fn drag_terrain(&mut self, x: i32, y: i32) {
        let Some(mut stroke) = self.tiles.terrain.take() else {
            return;
        };
        let size = (self.doc.width, self.doc.height);
        let (damage, overlay) = (&mut self.damage, &self.overlay);
        stroke.drag((x, y), &mut |r| mark_tiles(damage, overlay, size, r));
        self.tiles.terrain = Some(stroke);
    }

    /// Throws the terrain stroke in progress away.
    pub(crate) fn cancel_terrain_stroke(&mut self) {
        let Some(mut stroke) = self.tiles.terrain.take() else {
            return;
        };
        let size = (self.doc.width, self.doc.height);
        let (damage, overlay) = (&mut self.damage, &self.overlay);
        stroke.discard(&mut |r| mark_tiles(damage, overlay, size, r));
    }

    /// Writes the terrain stroke into its cel as one undo step, if it changes anything.
    pub(crate) fn commit_terrain_stroke(&mut self) {
        let Some(stroke) = self.tiles.terrain.take() else {
            return;
        };
        let place = (stroke.layer, stroke.frame);
        let Some(cel) = self
            .doc
            .layers
            .get(place.0)
            .and_then(|l| l.cels().get(place.1))
        else {
            return;
        };
        let image = cel.image();
        let changes = tile_changes(image, &stroke.changes(image));
        self.record_map_changes(place, stroke.geometry(), changes);
    }

    /// What the terrain stroke in progress would change, `None` without one. Worked out from the
    /// stroke's points each time it is asked, exactly as the commit does.
    pub fn terrain_preview(&self) -> Option<TerrainPreview> {
        let stroke = self.tiles.terrain.as_ref()?;
        let layer = self.doc.layers.get(stroke.layer)?;
        let image = layer.cels().get(stroke.frame)?.image();
        let cells = stroke.changes(image);
        Some(TerrainPreview {
            layer: stroke.layer,
            frame: stroke.frame,
            cells: cells
                .into_iter()
                .map(|(c, v)| (c, MapCell::from_bits(v)))
                .collect(),
        })
    }
}

/// The tiles of an image that changed cells fall in, as they will be, leaving out a tile the
/// cells leave as it is.
fn tile_changes(image: &Image, cells: &[(At, u32)]) -> Vec<(TileKey, Cell)> {
    let side = TILE as i32;
    let key_of = |(x, y): At| (y.div_euclid(side), x.div_euclid(side));
    let mut order: Vec<usize> = (0..cells.len()).collect();
    crate::sorting::sort_by_u64(&mut order, |&i| {
        let (ty, tx) = key_of(cells[i].0);
        crate::sorting::pair_key(ty, tx)
    });
    let mut out = Vec::new();
    let mut raw: RawTile = [0; TILE_AREA];
    let mut at = 0;
    while at < order.len() {
        let (ty, tx) = key_of(cells[order[at]].0);
        image.decode(tx, ty, &mut raw);
        let before = raw;
        while at < order.len() && key_of(cells[order[at]].0) == (ty, tx) {
            let ((x, y), value) = cells[order[at]];
            let index = ((y - ty * side) * side + (x - tx * side)) as usize;
            if let Some(slot) = raw.get_mut(index) {
                *slot = value;
            }
            at += 1;
        }
        if raw != before {
            // Cells a stroke reaches meet the canvas, so their tiles fit 16 bits.
            let key = TileKey {
                x: tx as i16,
                y: ty as i16,
            };
            out.push((key, Tile::pack(&raw).map(Arc::new)));
        }
    }
    out
}
