//! What the interface reads to show tiles: the cells a map stroke has placed and not yet put
//! into the map, a cell's pixels as placed, and a new tilemap layer with its own tileset.

use super::fixture::{editor_with_map, id, TILE};
use super::{MapCell, PendingCell};
use crate::command::{TileCmd, TileTool};
use crate::document::{ColorMode, LimitError};
use crate::grid::Grid;
use crate::orient::Orientation;
use std::sync::Arc;

fn mid(cx: i32, cy: i32) -> (i32, i32) {
    (cx * TILE as i32 + 1, cy * TILE as i32 + 1)
}

fn sorted(mut cells: Vec<PendingCell>) -> Vec<PendingCell> {
    cells.sort_by_key(|c| (c.rect.1, c.rect.0));
    cells
}

#[test]
fn a_stroke_in_progress_lists_the_cells_it_placed_until_it_ends() {
    let (mut e, set) = editor_with_map();
    assert!(e.pending_cells().is_empty(), "no stroke");
    e.set_current_tile(set, id(2)).expect("a tile");
    let (x, y) = mid(0, 3);
    e.apply(TileCmd::Begin {
        tool: TileTool::Place,
        x,
        y,
    })
    .expect("begins");
    let (x, y) = mid(2, 3);
    e.apply(TileCmd::To { x, y }).expect("drags");
    let placed = MapCell::place(id(2), Orientation::IDENTITY);
    let want: Vec<PendingCell> = (0..3)
        .map(|cx| PendingCell {
            rect: (cx * 4, 12, 4, 4),
            cell: placed,
        })
        .collect();
    assert_eq!(sorted(e.pending_cells()), want);
    assert_eq!(
        e.tiles().current(),
        Some((set, id(2), Orientation::IDENTITY))
    );
    e.apply(TileCmd::Commit).expect("ends");
    assert!(e.pending_cells().is_empty(), "put into the map");
}

#[test]
fn an_eraser_lists_the_cells_it_clears_as_empty() {
    let (mut e, _) = editor_with_map();
    let (x, y) = mid(1, 0);
    e.apply(TileCmd::Begin {
        tool: TileTool::Erase,
        x,
        y,
    })
    .expect("begins");
    let pending = e.pending_cells();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].rect, (4, 0, 4, 4));
    assert!(pending[0].cell.is_empty());
    e.apply(TileCmd::Cancel).expect("cancels");
    assert!(e.pending_cells().is_empty());
}

#[test]
fn a_cells_pixels_read_through_its_orientation() {
    let (mut e, set) = editor_with_map();
    // A tile whose pixel value is its own position, x + 4 * y + 1.
    let cells: Vec<u32> = (0..16).map(|i| 0xff00_0000 | (i + 1)).collect();
    let image = Arc::new(Grid::from_cells(4, 4, &cells).into_image());
    let made = e
        .apply(TileCmd::AddTile {
            tileset: set,
            from: crate::command::TileSource::Image(image),
        })
        .expect("adds");
    let tile = crate::tileset::TileId::new(made.created.expect("id")).expect("an id");
    let doc = e.doc();
    let plain = MapCell::place(tile, Orientation::IDENTITY);
    assert_eq!(doc.cell_pixel(set, plain, 1, 0), 0xff00_0002);
    let flipped = MapCell::place(tile, Orientation::flip_horizontal());
    assert_eq!(doc.cell_pixel(set, flipped, 0, 0), 0xff00_0004);
    let turned = MapCell::place(tile, Orientation::rotate90());
    for (x, y) in [(0, 0), (3, 0), (1, 2)] {
        let (sx, sy) = Orientation::rotate90().inverse().apply(x, y, 4, 4);
        let want = 0xff00_0000 | (sx + 4 * sy + 1) as u32;
        assert_eq!(doc.cell_pixel(set, turned, x, y), want, "{x} {y}");
    }
    assert_eq!(doc.cell_pixel(set, MapCell::EMPTY, 0, 0), 0);
    assert_eq!(doc.cell_pixel(set, plain, 4, 0), 0, "outside the tile");
}

#[test]
fn a_new_tilemap_layer_with_its_own_tileset_is_one_step() {
    let (mut e, _) = editor_with_map();
    let (before, depth) = (e.doc().clone(), e.history().undo_depth());
    let made = e
        .apply(TileCmd::NewMapLayer {
            at: 2,
            name: "Floor".into(),
            mode: ColorMode::Rgba,
            tile_w: 8,
            tile_h: 8,
        })
        .expect("room");
    let set = crate::tileset::TilesetId::new(made.created.expect("the tileset's id"));
    assert_eq!(e.history().undo_depth(), depth + 1);
    assert_eq!(e.layer, 2);
    assert_eq!(e.doc().layers()[2].tileset(), Some(set));
    let tiles = e.doc().tileset(set).expect("made");
    assert_eq!((tiles.name(), tiles.tile_size()), ("Floor", (8, 8)));
    let after = e.doc().clone();
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
    assert!(e.redo());
    assert_eq!(e.doc(), &after);
    let wrong = TileCmd::NewMapLayer {
        at: 0,
        name: String::new(),
        mode: ColorMode::Rgba,
        tile_w: 0,
        tile_h: 8,
    };
    assert_eq!(e.apply(wrong), Err(LimitError::TileSize));
    assert_eq!(e.doc(), &after, "refused whole");
}
