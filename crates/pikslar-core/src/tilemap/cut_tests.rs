//! Tiles from a selection: cut to the tileset's size, with exact duplicates found by hash and
//! confirmed by the pixels.

use super::cut::{cut_tiles, fnv, Room};
use crate::color::rgba;
use crate::command::{TileCmd, TileSource};
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::{Editor, Tool};
use crate::grid::Grid;
use crate::image::Image;
use crate::mask::Mask;
use crate::tileset::TilesetId;
use crate::{DrawCmd, SelectOp, SelectionCmd};
use std::sync::Arc;

const RED: u32 = rgba(255, 0, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);

const ROOM: Room = Room {
    tiles: usize::MAX,
    bytes: usize::MAX,
};

/// A `width` by 2 block of 2 by 2 tiles, each its own picture.
fn distinct_tiles(count: usize) -> Vec<u32> {
    let width = count * 2;
    (0..width * 2).map(|i| (i % width / 2 + 1) as u32).collect()
}

#[test]
fn cutting_stops_as_soon_as_the_tile_count_is_passed() {
    let block = distinct_tiles(1_000);
    let hashed = std::cell::Cell::new(0);
    let hash = |p: &[u32]| {
        hashed.set(hashed.get() + 1);
        fnv(p)
    };
    let room = Room {
        tiles: 3,
        bytes: usize::MAX,
    };
    let cut = cut_tiles((&block, 2_000, 2), (2, 2), &[], hash, room);
    assert_eq!(cut.err(), Some(LimitError::TooManyTiles));
    assert!(hashed.get() < 20, "{} tiles looked at", hashed.get());
    let fits = Room {
        tiles: 1_000,
        ..ROOM
    };
    let all = cut_tiles((&block, 2_000, 2), (2, 2), &[], fnv, fits).expect("fits");
    assert_eq!(all.len(), 1_000);
}

#[test]
fn cutting_stops_as_soon_as_the_tile_bytes_are_passed() {
    let block = distinct_tiles(1_000);
    let hashed = std::cell::Cell::new(0);
    let hash = |p: &[u32]| {
        hashed.set(hashed.get() + 1);
        fnv(p)
    };
    let one = crate::tileset::stored_tile_bytes(&Grid::from_cells(2, 2, &[1; 4]).into_image());
    let room = Room {
        tiles: usize::MAX,
        bytes: one * 3,
    };
    let cut = cut_tiles((&block, 2_000, 2), (2, 2), &[], hash, room);
    assert_eq!(cut.err(), Some(LimitError::TileMemory));
    assert!(hashed.get() < 20, "{} tiles looked at", hashed.get());
}

fn picture(pixels: &[u32]) -> Arc<Image> {
    Arc::new(Grid::from_cells(2, 2, pixels).into_image())
}

#[test]
fn equal_tiles_are_cut_once_and_clear_ones_not_at_all() {
    // A 4 by 2 block holds two 2 by 2 tiles, the same picture.
    let block = [1, 2, 1, 2, 3, 4, 3, 4];
    let cut = cut_tiles((&block, 4, 2), (2, 2), &[], fnv, ROOM).expect("room");
    assert_eq!(cut.len(), 1);
    assert_eq!(cut[0].1, [1, 2, 3, 4]);
    let clear = cut_tiles((&[0; 8], 4, 2), (2, 2), &[], fnv, ROOM).expect("room");
    assert!(clear.is_empty());
}

#[test]
fn a_tile_the_tileset_has_is_left_out_and_the_edge_is_padded() {
    let have = vec![picture(&[1, 2, 3, 4])];
    let block = [1, 2, 9, 3, 4, 9];
    // The first tile is the one it has; the second is `9 0 / 9 0`, padded at the edge.
    let cut = cut_tiles((&block, 3, 2), (2, 2), &have, fnv, ROOM).expect("room");
    assert_eq!(cut.len(), 1);
    assert_eq!(cut[0].1, [9, 0, 9, 0]);
}

#[test]
fn a_hash_that_collides_never_joins_two_different_pictures() {
    let same_hash = |_: &[u32]| 7;
    let have = vec![picture(&[1, 1, 1, 1])];
    // Tiles `2222`, `1111` (the one it has) and `3333`, side by side.
    let block = [2, 2, 1, 1, 3, 3, 2, 2, 1, 1, 3, 3];
    let cut = cut_tiles((&block, 6, 2), (2, 2), &have, same_hash, ROOM).expect("room");
    let pixels: Vec<&Vec<u32>> = cut.iter().map(|(_, p)| p).collect();
    assert_eq!(pixels, [&vec![2, 2, 2, 2], &vec![3, 3, 3, 3]]);
    let again = cut_tiles(
        (&[5, 5, 6, 6, 5, 5, 6, 6], 4, 2),
        (2, 2),
        &[],
        same_hash,
        ROOM,
    )
    .expect("room");
    assert_eq!(
        again.len(),
        2,
        "different pictures with one hash are both kept"
    );
}

fn paint_editor(mode: ColorMode) -> (Editor, TilesetId) {
    let mut e = Editor::new(Document::new(16, 16, mode).expect("size"));
    let made = e
        .apply(TileCmd::NewTileset {
            name: "T".into(),
            mode,
            tile_w: 4,
            tile_h: 4,
        })
        .expect("room");
    (e, TilesetId::new(made.created.expect("an id")))
}

fn fill_square(e: &mut Editor, color: u32, (x, y): (i32, i32)) {
    e.color = color;
    for row in 0..4 {
        let path = vec![(x, y + row), (x + 3, y + row)];
        e.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path,
        })
        .expect("stroke");
    }
}

fn select(e: &mut Editor, (x, y, w, h): (i32, i32, i32, i32)) {
    let shape = Box::new(Mask::rect(x, y, w, h));
    e.apply(SelectionCmd::Select {
        shape,
        op: SelectOp::Replace,
    })
    .expect("selects");
}

#[test]
fn tiles_from_a_selection_are_added_in_one_step_without_duplicates() {
    for mode in [ColorMode::Rgba, ColorMode::Palette] {
        let (mut e, set) = paint_editor(mode);
        fill_square(&mut e, RED, (0, 0));
        fill_square(&mut e, RED, (4, 0));
        fill_square(&mut e, BLUE, (8, 0));
        select(&mut e, (0, 0, 12, 4));
        let depth = e.history().undo_depth();
        let cmd = TileCmd::AddTile {
            tileset: set,
            from: TileSource::Selection,
        };
        let out = e.apply(cmd.clone()).expect("cut");
        assert!(out.changed, "{mode:?}");
        assert_eq!(e.history().undo_depth(), depth + 1, "one step");
        let tiles = e.doc().tileset(set).expect("set").tiles();
        assert_eq!(tiles.len(), 2, "{mode:?}: red once, blue once");
        assert_eq!(out.created, Some(tiles[0].id.get()));
        // Again: everything is there already, so nothing happens.
        let again = e.apply(cmd).expect("runs");
        assert!(!again.changed);
        assert_eq!(e.history().undo_depth(), depth + 1);
        assert!(e.undo());
        assert!(e.doc().tileset(set).expect("set").is_empty());
        assert!(e.redo());
        assert_eq!(e.doc().tileset(set).expect("set").len(), 2);
    }
}

#[test]
fn a_selection_that_is_clear_or_of_the_wrong_mode_makes_no_tiles() {
    let (mut e, set) = paint_editor(ColorMode::Rgba);
    let cmd = TileCmd::AddTile {
        tileset: set,
        from: TileSource::Selection,
    };
    assert_eq!(e.apply(cmd.clone()), Err(LimitError::NothingToCapture));
    select(&mut e, (0, 0, 8, 8));
    let out = e.apply(cmd).expect("a clear selection");
    assert!(!out.changed);
    // A palette tileset takes no pixels of an RGBA layer.
    let made = e
        .apply(TileCmd::NewTileset {
            name: "P".into(),
            mode: ColorMode::Palette,
            tile_w: 4,
            tile_h: 4,
        })
        .expect("room");
    let palette = TilesetId::new(made.created.expect("an id"));
    let cmd = TileCmd::AddTile {
        tileset: palette,
        from: TileSource::Selection,
    };
    assert_eq!(e.apply(cmd), Err(LimitError::TilemapMode));
}
