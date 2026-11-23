//! The tile editing state an editor holds: the stamp chosen for placing, and the map stroke in
//! progress. Choosing and turning a stamp is view state, like choosing a brush: never an undo
//! step, and never a reason to save.

use super::cell::MapCell;
use super::stamp::{orientation_of, TileStamp};
use super::stroke::MapStroke;
use crate::document::LimitError;
use crate::editor::Editor;
use crate::layer::Turn;
use crate::orient::Orientation;
use crate::overlay::Overlay;
use crate::tileset::{TileId, TilesetId};

/// The chosen stamp and the map stroke in progress.
#[derive(Default)]
pub struct TileState {
    stamp: Option<TileStamp>,
    pub(crate) stroke: Option<Box<MapStroke>>,
    /// The terrain stroke in progress. At most one of it and `stroke` is open.
    pub(crate) terrain: Option<Box<crate::autotile::stroke::TerrainStroke>>,
    /// The last stroke's overlay, kept so the next one reuses its tiles.
    spare: Option<Overlay>,
    /// How many pixels the last stroke on a map in Pixels mode dropped over empty cells.
    pub(crate) off_tiles: usize,
}

impl std::fmt::Debug for TileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileState")
            .field("stamp", &self.stamp)
            .field("stroking", &self.stroke.is_some())
            .field("terrain", &self.terrain.is_some())
            .finish_non_exhaustive()
    }
}

/// Two states are equal when they hold the same stamp and both have, or both lack, a stroke.
impl PartialEq for TileState {
    fn eq(&self, other: &Self) -> bool {
        self.stamp == other.stamp
            && self.stroke.is_some() == other.stroke.is_some()
            && self.terrain.is_some() == other.terrain.is_some()
    }
}

impl Eq for TileState {}

impl TileState {
    /// The block that placing puts down, if one is chosen.
    pub fn stamp(&self) -> Option<&TileStamp> {
        self.stamp.as_ref()
    }

    /// The tile, and how it is turned, when the stamp is a single cell.
    pub fn current(&self) -> Option<(TilesetId, TileId, Orientation)> {
        let stamp = self.stamp.as_ref()?;
        let cell = stamp.single_cell()?;
        Some((stamp.tileset(), cell.tile()?, cell.orientation()))
    }

    /// An empty overlay of this size: the last stroke's if it fits, else a new one.
    pub(crate) fn overlay_of(&mut self, (w, h): (usize, usize)) -> Overlay {
        match self.spare.take() {
            Some(spare) if spare.size() == (w, h) => spare,
            _ => Overlay::new(w, h),
        }
    }

    /// Keeps a finished stroke's overlay, emptied, for the next one.
    pub(crate) fn keep_overlay(&mut self, mut overlay: Overlay) {
        overlay.clear();
        self.spare = Some(overlay);
    }

    pub(crate) fn set_stamp(&mut self, stamp: Option<TileStamp>) {
        self.stamp = stamp;
    }
}

/// A cell a map stroke in progress has placed, not yet in the map: its canvas rectangle (left,
/// top, width, height) and what it will hold, empty where the stroke clears it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingCell {
    pub rect: (i32, i32, i32, i32),
    pub cell: MapCell,
}

impl Editor {
    /// The cells the map stroke in progress has placed so far, for the canvas to show before
    /// the stroke is put into the map. Empty when no map stroke is in progress.
    pub fn pending_cells(&self) -> Vec<PendingCell> {
        let mut cells = Vec::new();
        if let Some(stroke) = self.tiles.stroke.as_deref() {
            let geo = *stroke.geometry();
            stroke.for_each_placed(|c, value| {
                cells.push(PendingCell {
                    rect: geo.rect_of(c),
                    cell: MapCell::from_bits(value),
                });
            });
        }
        cells
    }

    /// How many pixels the last pixel stroke on a tilemap layer dropped because no tile is
    /// placed under them. When it is not 0 the status line says "Place tiles here first".
    pub fn pixels_off_tiles(&self) -> usize {
        self.tiles.off_tiles
    }

    /// Makes one tile, unturned, the stamp. Refused if the tileset has no such tile.
    pub fn set_current_tile(&mut self, tileset: TilesetId, tile: TileId) -> Result<(), LimitError> {
        self.doc
            .tileset(tileset)?
            .tile(tile)
            .ok_or(LimitError::NoSuchItem)?;
        let cell = MapCell::place(tile, Orientation::IDENTITY);
        self.tiles.set_stamp(Some(TileStamp::single(tileset, cell)));
        Ok(())
    }

    /// Makes a block of tiles the stamp, `width` by `height`, row by row. `None` leaves a place
    /// empty, so the block leaves the map as it is there.
    pub fn set_tile_block(
        &mut self,
        tileset: TilesetId,
        width: usize,
        height: usize,
        tiles: &[Option<TileId>],
    ) -> Result<(), LimitError> {
        let set = self.doc.tileset(tileset)?;
        if tiles.iter().flatten().any(|&t| set.tile(t).is_none()) {
            return Err(LimitError::NoSuchItem);
        }
        let place = |t: &Option<TileId>| {
            t.map_or(MapCell::EMPTY, |t| MapCell::place(t, Orientation::IDENTITY))
        };
        let cells = tiles.iter().map(place).collect();
        let stamp = TileStamp::block(tileset, width, height, cells)?;
        self.tiles.set_stamp(Some(stamp));
        Ok(())
    }

    /// Flips or quarter-turns the stamp as a whole. Returns false if there is none. Turning
    /// tiles by a quarter turn needs square tiles.
    pub fn turn_tile_stamp(&mut self, turn: Turn) -> Result<bool, LimitError> {
        let Some(stamp) = self.tiles.stamp() else {
            return Ok(false);
        };
        let turned = stamp.turned(orientation_of(turn));
        // Refused here, so the stamp never holds a turn the map could not keep.
        if let Ok(set) = self.doc.tileset(turned.tileset()) {
            let (w, h) = set.tile_size();
            if w != h && turned.orientation().swaps_axes() {
                return Err(LimitError::SquareTiles);
            }
        }
        self.tiles.set_stamp(Some(turned));
        Ok(true)
    }

    /// Chooses no stamp.
    pub fn clear_tile_stamp(&mut self) {
        self.tiles.set_stamp(None);
    }
}
