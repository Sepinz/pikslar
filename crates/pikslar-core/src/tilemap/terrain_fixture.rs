//! Documents with a terrain set for the tests of terrain strokes.

use super::fixture::{id, tile_image};
use super::MapCell;
use crate::autotile::TerrainKind;
use crate::command::{Corners, TerrainShape, TerrainTool, TileCmd, TileTool};
use crate::document::{ColorMode, Document};
use crate::editor::Editor;
use crate::orient::Orientation;
use crate::tileset::{TilesetId, MAX_TILE_ID};

/// Pixels a tile is wide and tall, and cells the canvas is wide and tall.
pub(super) const TILE: i32 = 4;
pub(super) const CELLS: i32 = 12;
/// A tile of the tileset that no terrain set holds.
pub(super) const OTHER: u32 = 50;

/// A 48 by 48 document of three frames: a paint layer at 0 and an empty tilemap layer at 1 of a
/// tileset with tiles 1 to 50. Terrain set 1, of `kind`, has slot `s` filled with tile `s + 1`.
pub(super) fn terrain_doc(kind: TerrainKind) -> (Document, TilesetId) {
    let side = (CELLS * TILE) as usize;
    let mut doc = Document::new(side, side, ColorMode::Rgba).expect("size");
    doc.add_frame(1, false).expect("room");
    doc.add_frame(2, false).expect("room");
    let set = doc
        .add_tileset("Ground", ColorMode::Rgba, (TILE as usize, TILE as usize))
        .expect("room");
    for n in 1..=OTHER {
        doc.add_tile(set, tile_image(0xff00_0000 | n))
            .expect("fits");
    }
    doc.add_tilemap_layer(1, set).expect("room");
    let terrain = doc.add_terrain(set, kind, "Grass", false).expect("room");
    for slot in 0..kind.slot_count() {
        let tile = id(slot as u32 + 1);
        doc.set_terrain_slot(set, terrain, slot, Some(tile))
            .expect("slot");
    }
    (doc, set)
}

/// [`terrain_doc`] in an editor with the tilemap layer active.
pub(super) fn terrain_editor(kind: TerrainKind) -> (Editor, TilesetId) {
    let (doc, set) = terrain_doc(kind);
    let mut editor = Editor::new(doc);
    editor.layer = 1;
    (editor, set)
}

/// The terrain tool of set 1.
pub(super) fn terrain(tileset: TilesetId, shape: TerrainShape, clear: bool) -> TileTool {
    TileTool::Terrain(TerrainTool {
        tileset,
        set: 1,
        shape,
        clear,
        corners: Corners::Cell,
    })
}

/// The same tool painting a Corner4 set's corners.
pub(super) fn by_vertex(tool: TileTool) -> TileTool {
    match tool {
        TileTool::Terrain(t) => TileTool::Terrain(TerrainTool {
            corners: Corners::Vertex,
            ..t
        }),
        other => other,
    }
}

/// The pixel in the middle of a cell.
pub(super) fn mid((cx, cy): (i32, i32)) -> (i32, i32) {
    (cx * TILE + 1, cy * TILE + 1)
}

/// A whole stroke through the middles of cells.
pub(super) fn through(tool: TileTool, cells: &[(i32, i32)]) -> TileCmd {
    TileCmd::Stroke {
        tool,
        path: cells.iter().map(|&c| mid(c)).collect(),
    }
}

/// The cell value at a cell of the active frame's map.
pub(super) fn at(e: &Editor, (cx, cy): (i32, i32)) -> u32 {
    e.doc().layers()[e.layer].cel(e.frame).image().get(cx, cy)
}

/// The slot of set 1 a cell's tile fills (tile `s + 1` is slot `s`), or `None` when empty.
pub(super) fn slot(e: &Editor, c: (i32, i32)) -> Option<u32> {
    let tile = at(e, c) & MAX_TILE_ID;
    tile.checked_sub(1)
}

/// The cell value placing a tile unturned.
pub(super) fn placed(tile: u32) -> u32 {
    MapCell::place(id(tile), Orientation::IDENTITY).bits()
}
