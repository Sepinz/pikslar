//! A block of tiles to place: the current tile, a block chosen in the tileset panel, or a block
//! captured from a map.
//!
//! The cells are kept as they were made, and the stamp's [`Orientation`] turns the whole block:
//! positions move as the turn moves them, and each cell's own turn is composed with the stamp's.
//! Four quarter turns therefore give back the same block, and nothing is rebuilt from a turned
//! copy. A stamp is view state, like a brush: choosing and turning one is never an undo step.

use super::cell::MapCell;
use crate::document::LimitError;
use crate::layer::Turn;
use crate::orient::Orientation;
use crate::tileset::{Tileset, TilesetId};
use std::sync::Arc;

/// The most tiles a stamp is wide or tall.
pub const MAX_STAMP_SIDE: usize = 64;

/// The orientation a flip or quarter turn of a picture stands for.
pub fn orientation_of(turn: Turn) -> Orientation {
    match turn {
        Turn::FlipHorizontal => Orientation::flip_horizontal(),
        Turn::FlipVertical => Orientation::flip_vertical(),
        Turn::Clockwise => Orientation::rotate90(),
        Turn::CounterClockwise => Orientation::rotate270(),
    }
}

/// A block of cells of one tileset, and how the block is turned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileStamp {
    tileset: TilesetId,
    /// The block as made, before the stamp's turn.
    width: usize,
    height: usize,
    cells: Arc<[MapCell]>,
    orientation: Orientation,
}

impl TileStamp {
    /// One cell, which is how a single chosen tile or a picked one is held. The cell keeps its
    /// own turn.
    pub fn single(tileset: TilesetId, cell: MapCell) -> TileStamp {
        TileStamp {
            tileset,
            width: 1,
            height: 1,
            cells: Arc::from([cell.with_pinned(false)]),
            orientation: Orientation::IDENTITY,
        }
    }

    /// A block of `width` by `height` cells, row by row. An empty cell leaves the map as it is
    /// where the block is placed.
    pub fn block(
        tileset: TilesetId,
        width: usize,
        height: usize,
        cells: Vec<MapCell>,
    ) -> Result<TileStamp, LimitError> {
        if width > MAX_STAMP_SIDE || height > MAX_STAMP_SIDE {
            return Err(LimitError::StampTooLarge);
        }
        if width == 0 || height == 0 || cells.len() != width * height {
            return Err(LimitError::NoTileChosen);
        }
        let cells: Arc<[MapCell]> = cells.into_iter().map(|c| c.with_pinned(false)).collect();
        Ok(TileStamp {
            tileset,
            width,
            height,
            cells,
            orientation: Orientation::IDENTITY,
        })
    }

    pub fn tileset(&self) -> TilesetId {
        self.tileset
    }

    /// How the whole block is turned.
    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    /// The block's width and height as it is placed, after the turn.
    pub fn size(&self) -> (usize, usize) {
        self.orientation.turned_size(self.width, self.height)
    }

    /// This stamp turned once more by `by`.
    pub fn turned(&self, by: Orientation) -> TileStamp {
        TileStamp {
            orientation: self.orientation.compose(by),
            ..self.clone()
        }
    }

    /// The cell at (`x`, `y`) of the block as placed, with the block's turn in its own.
    pub fn cell(&self, x: usize, y: usize) -> MapCell {
        let (w, h) = self.size();
        if x >= w || y >= h {
            return MapCell::EMPTY;
        }
        let (sx, sy) = self.orientation.inverse().apply(x, y, w, h);
        let source = self.cells[sy * self.width + sx];
        match source.tile() {
            Some(tile) => MapCell::place(tile, source.orientation().compose(self.orientation)),
            None => MapCell::EMPTY,
        }
    }

    /// The one cell of a single-cell block, as placed.
    pub fn single_cell(&self) -> Option<MapCell> {
        (self.cells.len() == 1).then(|| self.cell(0, 0))
    }

    /// The block made ready for placing in a map of this tileset: values row by row as placed,
    /// with a tile the tileset no longer has left out. Refused if no tile is left to place, or
    /// if a turn would turn tiles that are not square.
    pub(crate) fn prepare(&self, set: &Tileset) -> Result<Prepared, LimitError> {
        let (w, h) = self.size();
        let square = set.tile_size().0 == set.tile_size().1;
        let mut values = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let cell = self.cell(x, y);
                let live = cell.tile().is_some_and(|t| set.position(t).is_some());
                if live && !square && cell.orientation().swaps_axes() {
                    return Err(LimitError::SquareTiles);
                }
                values.push(if live { cell.bits() } else { 0 });
            }
        }
        if values.iter().all(|&v| v == 0) {
            return Err(LimitError::NoTileChosen);
        }
        Ok(Prepared {
            width: w as i32,
            height: h as i32,
            values,
        })
    }
}

/// A stamp resolved for one stroke: the cell values as placed, 0 where nothing is placed. Built
/// once when the stroke begins, so placing it allocates nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prepared {
    pub width: i32,
    pub height: i32,
    pub values: Vec<u32>,
}

impl Prepared {
    /// What an eraser places: one empty cell.
    pub fn eraser() -> Prepared {
        Prepared {
            width: 1,
            height: 1,
            values: vec![0],
        }
    }

    /// The value at (`x`, `y`) of the block, which must lie inside it.
    #[inline]
    pub fn at(&self, x: i32, y: i32) -> u32 {
        self.values[(y * self.width + x) as usize]
    }
}
