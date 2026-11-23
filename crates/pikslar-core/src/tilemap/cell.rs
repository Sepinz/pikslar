//! One cell of a tilemap: which tile stands there, and how it is turned.
//!
//! A map cel is an ordinary [`crate::layer::Cel`] whose image holds cell values. The layout is
//! Tiled's: bits 0 to 27 name the tile, bit 28 is the *pinned* flag (reserved in M10, used by the
//! automatic tilemaps of M11), and bits 29, 30 and 31 are the diagonal, vertical and horizontal
//! flips, where Tiled keeps them, so an export copies the flags unchanged.
//!
//! **How the cell grid sits on the cel's image.** Cell (`cx`, `cy`) is the image pixel
//! (`cx`, `cy`). The cel's offset stays in canvas pixels and is where cell (0, 0) begins, so the
//! cell is the canvas rectangle from (`offset.x + cx * tile_w`, `offset.y + cy * tile_h`) of the
//! tileset's tile size. Cells may lie at negative coordinates or past the canvas, as pixels of a
//! paint cel may. Moving a cel, or resizing the canvas, moves the grid with it.

use crate::orient::Orientation;
use crate::tileset::{TileId, MAX_TILE_ID};

/// The pinned flag: set by the automatic tilemaps of M11 on a cell the artist placed by hand.
pub const PINNED: u32 = 1 << 28;

/// One cell value. Tile id 0 is empty, whatever the flag bits say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MapCell(u32);

impl MapCell {
    /// No tile.
    pub const EMPTY: MapCell = MapCell(0);

    /// The cell a stored value stands for.
    pub const fn from_bits(bits: u32) -> MapCell {
        MapCell(bits)
    }

    /// The value as stored.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// The tile placed with this turn.
    pub const fn place(tile: TileId, orientation: Orientation) -> MapCell {
        MapCell(tile.get() | orientation.to_tiled_bits())
    }

    /// The tile that stands here, if any.
    pub const fn tile(self) -> Option<TileId> {
        TileId::new(self.0 & MAX_TILE_ID)
    }

    /// True when no tile stands here, though flag bits may be set.
    pub const fn is_empty(self) -> bool {
        self.0 & MAX_TILE_ID == 0
    }

    /// How the tile is turned.
    pub const fn orientation(self) -> Orientation {
        Orientation::from_tiled_bits(self.0)
    }

    pub const fn is_pinned(self) -> bool {
        self.0 & PINNED != 0
    }

    /// The same cell, pinned or not.
    pub const fn with_pinned(self, pinned: bool) -> MapCell {
        if pinned {
            MapCell(self.0 | PINNED)
        } else {
            MapCell(self.0 & !PINNED)
        }
    }

    /// True for the one form a document keeps: an empty cell is exactly 0.
    pub const fn is_canonical(self) -> bool {
        self.0 == 0 || !self.is_empty()
    }

    /// An empty cell with flag bits set becomes 0; any other cell is as it was.
    pub const fn canonical(self) -> MapCell {
        if self.is_empty() {
            MapCell::EMPTY
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orient::{FLIPPED_DIAGONALLY, FLIPPED_HORIZONTALLY, FLIPPED_VERTICALLY};

    fn tile(n: u32) -> TileId {
        TileId::new(n).expect("a tile id")
    }

    #[test]
    fn flags_sit_where_tiled_puts_them() {
        let h = MapCell::place(tile(5), Orientation::flip_horizontal());
        let v = MapCell::place(tile(5), Orientation::flip_vertical());
        let d = MapCell::place(tile(5), Orientation::transpose());
        assert_eq!(h.bits(), 5 | FLIPPED_HORIZONTALLY);
        assert_eq!(v.bits(), 5 | FLIPPED_VERTICALLY);
        assert_eq!(d.bits(), 5 | FLIPPED_DIAGONALLY);
    }

    #[test]
    fn every_tile_and_orientation_reads_back() {
        for id in [1, 2, 777, MAX_TILE_ID] {
            for o in Orientation::ALL {
                let cell = MapCell::place(tile(id), o);
                assert_eq!(cell.tile(), TileId::new(id));
                assert_eq!(cell.orientation(), o);
                assert!(!cell.is_empty() && !cell.is_pinned() && cell.is_canonical());
            }
        }
    }

    #[test]
    fn the_pinned_bit_is_bit_28_and_leaves_the_rest_alone() {
        let cell = MapCell::place(tile(9), Orientation::rotate90());
        let pinned = cell.with_pinned(true);
        assert_eq!(pinned.bits(), cell.bits() | 1 << 28);
        assert!(pinned.is_pinned());
        assert_eq!(pinned.tile(), cell.tile());
        assert_eq!(pinned.orientation(), cell.orientation());
        assert_eq!(pinned.with_pinned(false), cell);
    }

    #[test]
    fn zero_is_empty_and_flags_do_not_make_a_tile() {
        assert!(MapCell::EMPTY.is_empty() && MapCell::EMPTY.tile().is_none());
        let flagged = MapCell::from_bits(FLIPPED_HORIZONTALLY | PINNED);
        assert!(flagged.is_empty() && flagged.tile().is_none());
        assert!(!flagged.is_canonical());
        assert_eq!(flagged.canonical(), MapCell::EMPTY);
        let real = MapCell::place(tile(3), Orientation::rotate180());
        assert_eq!(real.canonical(), real);
    }
}
