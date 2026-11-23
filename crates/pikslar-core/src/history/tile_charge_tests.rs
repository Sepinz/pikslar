//! What history says tile steps keep, compared with the tile images they do keep.

use crate::command::{TileCmd, TileSource};
use crate::document::{ColorMode, Document};
use crate::editor::Editor;
use crate::grid::Grid;
use crate::image::Image;
use crate::layer::kept_bytes;
use crate::tileset::{TileId, TilesetId};
use std::sync::Arc;

const SIDE: usize = 64;

/// A tile of many different colours, so it takes real memory.
fn noisy(seed: u32) -> Arc<Image> {
    let cells: Vec<u32> = (0..(SIDE * SIDE) as u32)
        .map(|n| n.wrapping_mul(2_654_435_761).wrapping_add(seed) | 1)
        .collect();
    Arc::new(Grid::from_cells(SIDE, SIDE, &cells).into_image())
}

fn editor_with_tile(image: &Arc<Image>) -> (Editor, TilesetId, TileId) {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("size"));
    let made = e
        .apply(TileCmd::NewTileset {
            name: "T".into(),
            mode: ColorMode::Rgba,
            tile_w: SIDE as u16,
            tile_h: SIDE as u16,
        })
        .expect("room");
    let set = TilesetId::new(made.created.expect("an id"));
    let added = e
        .apply(TileCmd::AddTile {
            tileset: set,
            from: TileSource::Image(image.clone()),
        })
        .expect("fits");
    let tile = TileId::new(added.created.expect("an id")).expect("a tile id");
    (e, set, tile)
}

#[test]
fn a_removed_tile_is_charged_for_its_pixels_while_removed_and_not_once_back() {
    let image = noisy(1);
    let (mut e, set, tile) = editor_with_tile(&image);
    let drawing = kept_bytes(&image);
    assert!(drawing > 8_000, "the tile takes real memory: {drawing}");
    let before = e.history().memory_used();
    // The test's own reference goes, so that only the document keeps the image.
    drop(image);
    e.apply(TileCmd::RemoveTile { tileset: set, tile })
        .expect("removed");
    let removed = e.history().memory_used() - before;
    assert!(
        removed >= drawing,
        "removed: charged {removed} for {drawing}"
    );
    assert!(removed < drawing * 3 / 2, "removed: charged {removed}");
    assert!(e.undo());
    let back = e.history().memory_used() - before;
    assert!(back < drawing / 4, "back: charged {back} for {drawing}");
    assert!(e.redo());
    let again = e.history().memory_used() - before;
    assert!(again >= drawing, "again: charged {again} for {drawing}");
}

#[test]
fn an_edited_tile_is_charged_for_the_image_it_replaced_and_then_for_the_new_one() {
    let image = noisy(1);
    let (mut e, set, tile) = editor_with_tile(&image);
    let drawing = kept_bytes(&image);
    drop(image);
    let before = e.history().memory_used();
    let new = noisy(2);
    let new_bytes = kept_bytes(&new);
    e.apply(TileCmd::SetTileImage {
        tileset: set,
        tile,
        image: new,
    })
    .expect("edited");
    let edited = e.history().memory_used() - before;
    assert!(edited >= drawing && edited < drawing * 3 / 2, "{edited}");
    // Undone, the step keeps the new image instead.
    assert!(e.undo());
    let undone = e.history().memory_used() - before;
    assert!(
        undone >= new_bytes && undone < new_bytes * 3 / 2,
        "undone: {undone}"
    );
}

#[test]
fn an_image_two_tiles_share_is_charged_once_and_only_when_the_last_is_removed() {
    let image = noisy(1);
    let (mut e, set, tile) = editor_with_tile(&image);
    let drawing = kept_bytes(&image);
    drop(image);
    let copy = e
        .apply(TileCmd::DuplicateTile { tileset: set, tile })
        .expect("there")
        .created
        .and_then(TileId::new)
        .expect("a tile id");
    let before = e.history().memory_used();
    e.apply(TileCmd::RemoveTile { tileset: set, tile })
        .expect("removed");
    let first = e.history().memory_used() - before;
    assert!(first < drawing / 4, "the copy still shows it: {first}");
    e.apply(TileCmd::RemoveTile {
        tileset: set,
        tile: copy,
    })
    .expect("removed");
    let second = e.history().memory_used() - before;
    assert!(
        second >= drawing && second < drawing * 3 / 2,
        "charged once: {second}"
    );
}

#[test]
fn a_step_that_leaves_the_tilesets_alone_keeps_no_hold_on_them() {
    let (mut e, _, _) = editor_with_tile(&noisy(1));
    e.apply(crate::FrameCmd::Add { at: 1, copy: false })
        .expect("room");
    assert_eq!(
        Arc::strong_count(&e.doc().tilesets),
        1,
        "so editing a tile copies no table"
    );
}

#[test]
fn many_tile_edits_stay_within_the_budget() {
    let budget = 120_000;
    let image = noisy(0);
    let (mut e, set, tile) = {
        let (mut e, set, tile) = editor_with_tile(&image);
        let doc = e.doc().clone();
        e = Editor::with_undo_budget(doc, budget);
        (e, set, tile)
    };
    for n in 1..300 {
        e.apply(TileCmd::SetTileImage {
            tileset: set,
            tile,
            image: noisy(n),
        })
        .expect("edited");
        assert!(e.history().steps_used() <= budget + 40_000, "edit {n}");
    }
    assert!(e.history().undo_depth() < 100, "old steps were dropped");
    assert!(e.history().undo_depth() > 3, "and the newest kept");
}

#[test]
fn a_tile_step_brings_back_the_colour_table_from_before_it() {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Palette).expect("size"));
    let made = e
        .apply(TileCmd::NewTileset {
            name: "T".into(),
            mode: ColorMode::Palette,
            tile_w: 4,
            tile_h: 4,
        })
        .expect("room");
    let set = TilesetId::new(made.created.expect("an id"));
    let added = e
        .apply(TileCmd::AddTile {
            tileset: set,
            from: TileSource::Blank,
        })
        .expect("fits");
    let tile = TileId::new(added.created.expect("an id")).expect("a tile id");
    // As an edit that adds a colour does: the table grows, and the step keeps the old one.
    let table = e.doc().colors.clone();
    let id = Arc::make_mut(&mut e.doc.colors)
        .add_swatch(0xff10_2030)
        .expect("room");
    let mut list = vec![(
        tile,
        Arc::new(Grid::from_cells(4, 4, &[id; 16]).into_image()),
    )];
    e.doc.swap_tile_images(set, &mut list);
    let before = e.history().memory_used();
    e.record(crate::Edit::tile_images(
        set,
        list.into(),
        Some(table.clone()),
    ));
    assert!(
        e.history().memory_used() - before >= table.kept_bytes(),
        "the step is charged for the table it keeps"
    );
    assert!(e.undo());
    assert_eq!(e.doc().colors.colors(), table.colors());
    assert!(e
        .doc()
        .tileset(set)
        .expect("there")
        .tile(tile)
        .expect("t")
        .image
        .is_blank());
    assert!(e.redo());
    assert_eq!(e.doc().colors.len(), table.len() + 1);
    let shown = &e
        .doc()
        .tileset(set)
        .expect("there")
        .tile(tile)
        .expect("t")
        .image;
    assert_eq!(shown.get(0, 0), id);
}
