//! Map strokes: pointer input turned into cells placed on the active tilemap layer, one undo
//! step for the whole stroke. The cells are in `tilemap::stroke`; this is the part that touches
//! the editor: damage, history, and the rules of which layer may be drawn on.

use super::{Damage, Editor};
use crate::command::TileTool;
use crate::document::LimitError;
use crate::history::{tile_bytes, Edit};
use crate::tilemap::stroke::{Geometry, MapStroke, Rect};
use crate::tilemap::{MapCell, Prepared, TileStamp};

impl Editor {
    /// The tileset of the active layer, which must be a tilemap layer with the frame shown.
    pub(crate) fn active_map(&self) -> Result<crate::tileset::TilesetId, LimitError> {
        let layer = self
            .doc
            .layers
            .get(self.layer)
            .ok_or(LimitError::NoSuchItem)?;
        if self.frame >= layer.frame_count() {
            return Err(LimitError::NoSuchItem);
        }
        layer.tileset().ok_or(LimitError::NotTilemap)
    }

    /// Where the active map's cells are on the canvas.
    pub(crate) fn map_geometry(&self) -> Result<Geometry, LimitError> {
        let set = self.doc.tileset(self.active_map()?)?;
        let cel = self.doc.layers[self.layer].cel(self.frame);
        let canvas = (self.doc.width, self.doc.height);
        Ok(Geometry::new(set.tile_size(), cel.position(), canvas))
    }

    /// Marks the canvas tiles a rectangle touches for redrawing.
    fn mark_rect(&mut self, (x, y, w, h): Rect) {
        self.mark_area(x, y, w, h);
    }

    /// The stamp as it will be placed by a stroke of this tool.
    fn stamp_for(
        &self,
        tool: TileTool,
        tileset: crate::tileset::TilesetId,
    ) -> Result<Prepared, LimitError> {
        if matches!(tool, TileTool::Erase | TileTool::Pick) {
            return Ok(Prepared::eraser());
        }
        let stamp = self.tiles.stamp().ok_or(LimitError::NoTileChosen)?;
        if stamp.tileset() != tileset {
            return Err(LimitError::NoTileChosen);
        }
        let set = self.doc.tileset(tileset)?;
        let mut prepared = stamp.prepare(set)?;
        pin_terrain_tiles(&mut prepared, set);
        Ok(prepared)
    }

    /// Presses a map tool on the canvas. A fill is finished at once, and a pick changes the
    /// stamp and nothing else.
    pub(crate) fn begin_tiles(&mut self, tool: TileTool, x: i32, y: i32) -> Result<(), LimitError> {
        if let TileTool::Terrain(terrain) = tool {
            return self.begin_terrain(terrain, x, y);
        }
        let tileset = self.active_map()?;
        let geo = self.map_geometry()?;
        let prepared = self.stamp_for(tool, tileset)?;
        self.anchor();
        self.cancel();
        let start = geo.cell_of(x, y);
        if tool == TileTool::Pick {
            self.pick_cell(tileset, start);
            return Ok(());
        }
        let place = (self.layer, self.frame);
        let cells = self.tiles.overlay_of(geo.overlay_size());
        let mut stroke = Box::new(MapStroke::new(place, tool, geo, prepared, (start, cells)));
        let selection = self.selection.clone();
        let size = (self.doc.width, self.doc.height);
        let (damage, overlay) = (&mut self.damage, &self.overlay);
        let mut mark = |r: Rect| mark_tiles(damage, overlay, size, r);
        if tool == TileTool::Fill {
            let image = self.doc.layers[self.layer].cel(self.frame).image();
            stroke.fill(image, selection.as_deref(), &mut mark);
        } else {
            stroke.press(selection.as_deref(), &mut mark);
        }
        self.tiles.stroke = Some(stroke);
        if tool == TileTool::Fill {
            self.commit_tile_stroke();
        }
        Ok(())
    }

    /// Makes the tile under the press, and how it is turned, the stamp. An empty cell changes
    /// nothing, as the colour picker leaves the colour on a clear pixel.
    fn pick_cell(&mut self, tileset: crate::tileset::TilesetId, cell: (i32, i32)) {
        let image = self.doc.layers[self.layer].cel(self.frame).image();
        let picked = MapCell::from_bits(image.get(cell.0, cell.1));
        if picked.tile().is_some() {
            self.tiles
                .set_stamp(Some(TileStamp::single(tileset, picked)));
        }
    }

    /// Continues the map stroke to a canvas position.
    pub(crate) fn drag_tiles(&mut self, x: i32, y: i32) {
        let Some(mut stroke) = self.tiles.stroke.take() else {
            self.drag_terrain(x, y);
            return;
        };
        let selection = self.selection.clone();
        let damage = &mut self.damage;
        let (width, height) = (self.doc.width, self.doc.height);
        let overlay = &self.overlay;
        let mut mark =
            |(x, y, w, h): Rect| mark_tiles(damage, overlay, (width, height), (x, y, w, h));
        stroke.drag((x, y), selection.as_deref(), &mut mark);
        self.tiles.stroke = Some(stroke);
    }

    /// Throws the map stroke in progress away.
    pub(crate) fn cancel_tile_stroke(&mut self) {
        self.cancel_terrain_stroke();
        let Some(mut stroke) = self.tiles.stroke.take() else {
            return;
        };
        let damage = &mut self.damage;
        let (width, height) = (self.doc.width, self.doc.height);
        let overlay = &self.overlay;
        stroke.discard(&mut |r| mark_tiles(damage, overlay, (width, height), r));
        self.tiles.keep_overlay(stroke.into_overlay());
    }

    /// Writes the map stroke into its cel as one undo step, if it changed anything. The
    /// drawing is shared with every cel that holds or links it, so they show the change too.
    pub(crate) fn commit_tile_stroke(&mut self) {
        self.commit_terrain_stroke();
        let Some(stroke) = self.tiles.stroke.take() else {
            return;
        };
        let (layer, frame) = (stroke.layer, stroke.frame);
        let Some(cel) = self.doc.layers.get(layer).and_then(|l| l.cels().get(frame)) else {
            self.tiles.keep_overlay(stroke.into_overlay());
            return;
        };
        let changes = stroke.changes(cel.image());
        let geometry = *stroke.geometry();
        self.tiles.keep_overlay(stroke.into_overlay());
        self.record_map_changes((layer, frame), &geometry, changes);
    }

    /// Writes changed tiles of a map cel's image as one undo step, nothing if there are none,
    /// marking the canvas they cover. The drawing is shared with every cel that holds or links
    /// it, so they show the change too.
    pub(super) fn record_map_changes(
        &mut self,
        (layer, frame): (usize, usize),
        geometry: &Geometry,
        mut changes: Vec<(crate::image::TileKey, crate::grid::Cell)>,
    ) {
        if changes.is_empty() {
            return;
        }
        for (key, _) in &changes {
            let rect = geometry.rect_of_image_tile(i32::from(key.x), i32::from(key.y));
            self.mark_rect(rect);
        }
        let written = tile_bytes(&changes);
        let mut split = None;
        if self.doc.swap_tiles(layer, frame, &mut changes, &mut split) {
            // Other layers show the change too.
            self.damage = Damage::All;
        }
        let swing = (written as i64 - tile_bytes(&changes) as i64)
            .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        let table = self.doc.layers[layer].cel(frame).table_bytes();
        self.widest_table = self.widest_table.max(table);
        self.record(Edit::Tiles {
            // Within a document's limits, which history checks fit 16 bits.
            layer: layer as u16,
            frame: frame as u16,
            left: 0,
            swing,
            changes: changes.into_boxed_slice(),
            selection: None,
            colors: None,
            split,
        });
        self.debug_check_tilemaps("a map stroke");
    }

    /// Clears every cell of the active map whose centre the selection holds, as one step.
    /// Nothing happens without a selection.
    pub(crate) fn delete_tiles(&mut self) -> Result<(), LimitError> {
        let geo = self.map_geometry()?;
        self.anchor();
        self.cancel();
        let Some(selection) = self.selection.clone() else {
            return Ok(());
        };
        let place = (self.layer, self.frame);
        let cells = self.tiles.overlay_of(geo.overlay_size());
        let eraser = Prepared::eraser();
        let mut stroke = Box::new(MapStroke::new(
            place,
            TileTool::Erase,
            geo,
            eraser,
            ((0, 0), cells),
        ));
        let image = self.doc.layers[self.layer].cel(self.frame).image();
        stroke.clear_selected(image, &selection, &mut |_| {});
        self.tiles.stroke = Some(stroke);
        self.commit_tile_stroke();
        Ok(())
    }
}

/// Pins every value of a prepared stamp whose tile fills a slot of one of the tileset's terrain
/// sets: a terrain tile placed by hand stays as placed. Done once when a stroke begins, so placing
/// costs nothing more per event, and with no terrain sets nothing at all.
fn pin_terrain_tiles(prepared: &mut Prepared, set: &crate::tileset::Tileset) {
    if set.terrains().is_empty() {
        return;
    }
    let slots = set
        .terrains()
        .iter()
        .flat_map(|t| t.slots().iter().flatten());
    let mut tiles: Vec<u32> = slots.map(|tile| tile.get()).collect();
    tiles.sort_unstable();
    for value in &mut prepared.values {
        let tile = MapCell::from_bits(*value).tile();
        if tile.is_some_and(|t| tiles.binary_search(&t.get()).is_ok()) {
            *value |= crate::tilemap::PINNED;
        }
    }
}

/// Marks the canvas tiles a rectangle touches, without borrowing the whole editor.
pub(super) fn mark_tiles(
    damage: &mut Damage,
    overlay: &crate::overlay::Overlay,
    (width, height): (usize, usize),
    (x, y, w, h): Rect,
) {
    use crate::tile::TILE_SHIFT;
    let (x1, y1) = (
        x.saturating_add(w).min(width as i32),
        y.saturating_add(h).min(height as i32),
    );
    let (x0, y0) = (x.max(0), y.max(0));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    for ty in (y0 as usize >> TILE_SHIFT)..=((y1 - 1) as usize >> TILE_SHIFT) {
        for tx in (x0 as usize >> TILE_SHIFT)..=((x1 - 1) as usize >> TILE_SHIFT) {
            let index = overlay.tile_index(tx << TILE_SHIFT, ty << TILE_SHIFT);
            damage.add(index as u32);
        }
    }
}
