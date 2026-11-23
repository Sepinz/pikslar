//! The numbers that name tilesets and the tiles in them.

/// The most a tile id can be: 28 bits, so a map cell keeps its flags in the bits above.
pub const MAX_TILE_ID: u32 = (1 << 28) - 1;

/// Names a tileset for the life of a document. Given out from a counter and never given twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TilesetId(u32);

impl TilesetId {
    pub const fn new(number: u32) -> Self {
        TilesetId(number)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Names a tile within its tileset: 1 to [`MAX_TILE_ID`]. There is no tile 0, since a map cell
/// of 0 is empty. Given out from the tileset's counter and never given twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId(u32);

impl TileId {
    /// The tile a number names, unless it is 0 or past [`MAX_TILE_ID`].
    pub const fn new(number: u32) -> Option<Self> {
        if number == 0 || number > MAX_TILE_ID {
            None
        } else {
            Some(TileId(number))
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    /// Sorts ids by number, through the sort of plain numbers.
    #[inline(never)]
    pub fn sort(ids: &mut [TileId]) {
        let mut numbers: Vec<u32> = ids.iter().map(|id| id.0).collect();
        numbers.sort_unstable();
        for (id, number) in ids.iter_mut().zip(numbers) {
            *id = TileId(number);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_ids_are_one_to_the_most_a_cell_holds() {
        assert_eq!(TileId::new(0), None);
        assert_eq!(TileId::new(1).map(TileId::get), Some(1));
        assert_eq!(TileId::new(MAX_TILE_ID).map(TileId::get), Some(MAX_TILE_ID));
        assert_eq!(TileId::new(MAX_TILE_ID + 1), None);
    }
}
