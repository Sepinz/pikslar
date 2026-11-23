//! Documents with tilemap layers for the tests of this module.

use super::cell::MapCell;
use crate::document::{ColorMode, Document};
use crate::editor::Editor;
use crate::grid::Grid;
use crate::image::Image;
use crate::layer::Cel;
use crate::orient::Orientation;
use crate::tileset::{TileId, TilesetId};
use std::sync::Arc;

/// Width and height of the fixture's tiles, and of its canvas in cells.
pub(super) const TILE: usize = 4;

pub(super) fn id(n: u32) -> TileId {
    TileId::new(n).expect("a tile id")
}

/// The cell value that places tile `n` unturned.
pub(super) fn cell(n: u32) -> u32 {
    MapCell::place(id(n), Orientation::IDENTITY).bits()
}

/// A tile image: every pixel one colour, within the fixture's tile size.
pub(super) fn tile_image(color: u32) -> Arc<Image> {
    Arc::new(Grid::from_cells(TILE, TILE, &[color; TILE * TILE]).into_image())
}

/// A map cel of 2 by 2 cells, left to right and top to bottom.
pub(super) fn map_cel(cells: [u32; 4]) -> Cel {
    Cel::new(Grid::from_cells(2, 2, &cells))
}

/// The four cells of a map cel's 2 by 2 corner, as stored.
pub(super) fn corner(cel: &Cel) -> [u32; 4] {
    let at = |x, y| cel.image().get(x, y);
    [at(0, 0), at(1, 0), at(0, 1), at(1, 1)]
}

/// A 16 by 16 document of three frames with a paint layer at 0 and a tilemap layer at 1. The
/// tileset has tiles 1 and 2, and the map places `1 2 / 2 1` in frame 0, `2 2 / 0 1` in frame 1
/// and nothing in frame 2.
pub(super) fn doc_with_map() -> (Document, TilesetId) {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    doc.add_frame(1, false).expect("room");
    doc.add_frame(2, false).expect("room");
    let set = doc
        .add_tileset("Walls", ColorMode::Rgba, (TILE, TILE))
        .expect("room");
    doc.add_tile(set, tile_image(0xff00_00ff)).expect("fits");
    doc.add_tile(set, tile_image(0xff00_ff00)).expect("fits");
    doc.add_tilemap_layer(1, set).expect("room");
    let (a, b) = (cell(1), cell(2));
    doc.put_cel(1, 0, map_cel([a, b, b, a])).expect("fits");
    doc.put_cel(1, 1, map_cel([b, b, 0, a])).expect("fits");
    (doc, set)
}

/// [`doc_with_map`] in an editor with the tilemap layer active.
pub(super) fn editor_with_map() -> (Editor, TilesetId) {
    let (doc, set) = doc_with_map();
    let mut editor = Editor::new(doc);
    editor.layer = 1;
    (editor, set)
}
