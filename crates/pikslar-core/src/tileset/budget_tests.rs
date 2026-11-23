//! The limit on tile pixels: counted as a file stores them, and held by every change that could
//! add any, so that what is saved opens again.

use super::*;
use crate::autotile::rules::RuleKind;
use crate::command::{TileCmd, TileSource};
use crate::document::ColorMode;
use crate::editor::{Editor, Tool};
use crate::grid::Grid;
use crate::layer::Cel;
use crate::mask::Mask;
use crate::tilemap::MapCell;
use crate::tileset::{TileId, MAX_TILES};
use crate::{DrawCmd, SelectOp, SelectionCmd, TerrainCmd};

const SIDE: usize = 256;

/// A picture whose every pixel differs, so no storage tile is solid or packed small.
fn noise() -> Arc<Image> {
    let px: Vec<u32> = (0..SIDE * SIDE)
        .map(|i| 0xff00_0000 | (i as u32).wrapping_mul(2_654_435_761) >> 8)
        .collect();
    Arc::new(Grid::from_cells(SIDE, SIDE, &px).into_image())
}

/// A picture of one colour: 256 solid storage tiles, small in a file but each its own when read.
fn solid() -> Arc<Image> {
    Arc::new(Grid::from_cells(SIDE, SIDE, &[0xff10_2030; SIDE * SIDE]).into_image())
}

/// A document with one 256 by 256 tileset, and a tilemap layer of it at 1.
fn doc() -> (Document, TilesetId) {
    let mut doc = Document::new(SIDE, SIDE, ColorMode::Rgba).expect("size");
    let set = doc
        .add_tileset("Big", ColorMode::Rgba, (SIDE, SIDE))
        .expect("room");
    doc.add_tilemap_layer(1, set).expect("room");
    (doc, set)
}

/// Fills a tileset of `doc` with solid and then blank tiles until less than a blank tile's worth
/// of room is left.
fn fill(doc: &mut Document, set: TilesetId) {
    let (big, blank) = (solid(), Arc::new(Image::blank()));
    let room = |d: &Document| MAX_TILE_BYTES - d.stored_tile_bytes();
    while room(doc) >= stored_tile_bytes(&big) {
        doc.add_tile(set, big.clone()).expect("room left");
    }
    while room(doc) >= stored_tile_bytes(&blank) {
        doc.add_tile(set, blank.clone()).expect("room left");
    }
    assert!(doc.tileset(set).expect("set").len() < MAX_TILES);
}

/// The total counted afresh from every tile, to check the one kept as tiles change.
fn counted(doc: &Document) -> usize {
    doc.tilesets
        .iter()
        .flat_map(|set| set.tiles())
        .map(|entry| stored_tile_bytes(&entry.image))
        .sum()
}

fn refused(e: &mut Editor, command: impl Into<crate::Command>) {
    let before = e.doc().clone();
    let depth = e.history().undo_depth();
    assert_eq!(e.apply(command.into()), Err(LimitError::TileMemory));
    assert_eq!(e.doc(), &before, "nothing changed");
    assert_eq!(e.history().undo_depth(), depth, "no step");
}

#[test]
fn a_tile_counts_as_a_file_reads_it_whatever_it_shares() {
    let image = noise();
    let one = stored_tile_bytes(&image);
    let storage = image.drawn_tiles().count();
    assert_eq!(
        one,
        TILE_RECORD_BYTES + image.heap_size() + storage * STORAGE_TILE_EXTRA
    );
    let (mut d, set) = doc();
    d.add_tile(set, image.clone()).expect("fits");
    d.add_tile(set, image).expect("fits");
    assert_eq!(
        d.stored_tile_bytes(),
        2 * one,
        "a shared image counts twice"
    );
}

#[test]
fn duplicating_a_large_tile_stops_before_the_file_limit() {
    // One 256 by 256 tile and up to 2,000 copies: about 256 KB in memory, far past the limit in a file.
    let mut e = {
        let (mut d, set) = doc();
        d.add_tile(set, noise()).expect("fits");
        Editor::new(d)
    };
    let set = TilesetId::new(1);
    let tile = TileId::new(1).expect("id");
    let mut copies = 0;
    for _ in 0..2_000 {
        let before = e.doc().clone();
        match e.apply(TileCmd::DuplicateTile { tileset: set, tile }) {
            Ok(_) => copies += 1,
            Err(error) => {
                assert_eq!(error, LimitError::TileMemory);
                assert_eq!(e.doc(), &before, "a refusal changes nothing");
                break;
            }
        }
    }
    assert!(copies > 100 && copies < 2_000, "{copies} copies");
    assert!(e.doc().stored_tile_bytes() <= MAX_TILE_BYTES);
    assert_eq!(e.doc().stored_tile_bytes(), counted(e.doc()));
    assert!(LimitError::TileMemory.to_string().contains("416 MiB"));
}

#[test]
fn adding_a_tile_past_the_limit_is_refused_and_changes_nothing() {
    let (mut d, set) = doc();
    fill(&mut d, set);
    let mut e = Editor::new(d);
    for from in [TileSource::Blank, TileSource::Image(solid())] {
        refused(&mut e, TileCmd::AddTile { tileset: set, from });
    }
    let tile = TileId::new(1).expect("id");
    refused(&mut e, TileCmd::DuplicateTile { tileset: set, tile });
    // Taking a tile out makes room again.
    let last = e.doc().tileset(set).expect("set").tiles()[0].id;
    e.apply(TileCmd::RemoveTile {
        tileset: set,
        tile: last,
    })
    .expect("removes");
    e.apply(TileCmd::AddTile {
        tileset: set,
        from: TileSource::Blank,
    })
    .expect("room again");
    assert_eq!(e.doc().stored_tile_bytes(), counted(e.doc()));
}

#[test]
fn copying_a_tileset_or_a_template_past_the_limit_is_refused() {
    let (mut d, set) = doc();
    fill(&mut d, set);
    let mut e = Editor::new(d);
    let layer = e.layer_id(1).expect("layer");
    refused(&mut e, TileCmd::CopyTilesetFor { layer });
    let template = TerrainCmd::New {
        tileset: set,
        kind: RuleKind::Edge4,
        name: "Water".into(),
        template: true,
    };
    refused(&mut e, template);
    let mut d = e.doc().clone();
    assert_eq!(
        d.add_terrain(set, RuleKind::Edge4, "Water", true),
        Err(LimitError::TileMemory)
    );
    assert_eq!(&d, e.doc(), "refused before any tile is added");
}

#[test]
fn a_bigger_tile_image_past_the_limit_is_refused_and_a_smaller_one_is_not() {
    let (mut d, set) = doc();
    fill(&mut d, set);
    let mut e = Editor::new(d);
    let tiles = e.doc().tileset(set).expect("set").tiles();
    let (big, small) = (tiles[0].id, tiles[tiles.len() - 1].id);
    let noise = noise();
    refused(
        &mut e,
        TileCmd::SetTileImage {
            tileset: set,
            tile: small,
            image: noise.clone(),
        },
    );
    // A blank image in place of a solid one frees room, and is kept.
    let blank = Arc::new(Image::blank());
    let out = e.apply(TileCmd::SetTileImage {
        tileset: set,
        tile: big,
        image: blank,
    });
    assert!(out.expect("smaller").changed);
    assert_eq!(e.doc().stored_tile_bytes(), counted(e.doc()));
    assert!(e.undo());
    assert_eq!(e.doc().stored_tile_bytes(), counted(e.doc()));
}

#[test]
fn a_stroke_through_tiles_past_the_limit_is_refused() {
    let (mut d, set) = doc();
    fill(&mut d, set);
    let first = d.tileset(set).expect("set").tiles()[0].id;
    let cell = MapCell::place(first, crate::orient::Orientation::IDENTITY).bits();
    let map = Grid::from_cells(1, 1, &[cell]).into_image();
    d.put_cel(1, 0, Cel::from_image(map)).expect("fits");
    let mut e = Editor::new(d);
    e.layer = 1;
    e.color = 0xff99_0000;
    // A line across the solid tile makes each storage tile it crosses two colours.
    let line = DrawCmd::Stroke {
        tool: Tool::Line,
        path: vec![(0, 3), (SIDE as i32 - 1, 3)],
    };
    refused(&mut e, line);
}

#[test]
fn tiles_cut_from_a_selection_past_the_limit_are_refused() {
    let (mut d, set) = doc();
    fill(&mut d, set);
    let picture = Image::clone(&noise());
    d.put_cel(0, 0, Cel::from_image(picture)).expect("fits");
    let mut e = Editor::new(d);
    e.layer = 0;
    e.apply(SelectionCmd::Select {
        shape: Box::new(Mask::rect(0, 0, SIDE as i32, SIDE as i32)),
        op: SelectOp::Replace,
    })
    .expect("selects");
    let cut = TileCmd::AddTile {
        tileset: set,
        from: TileSource::Selection,
    };
    refused(&mut e, cut);
}

#[test]
fn the_kept_total_follows_every_change_and_its_undo() {
    let (d, set) = doc();
    let mut e = Editor::new(d);
    let image = noise();
    let add = |e: &mut Editor, from| e.apply(TileCmd::AddTile { tileset: set, from });
    add(&mut e, TileSource::Image(image.clone())).expect("fits");
    add(&mut e, TileSource::Blank).expect("fits");
    let tile = TileId::new(2).expect("id");
    e.apply(TileCmd::DuplicateTile { tileset: set, tile })
        .expect("fits");
    e.apply(TileCmd::SetTileImage {
        tileset: set,
        tile,
        image: solid(),
    })
    .expect("fits");
    let layer = e.layer_id(1).expect("layer");
    e.apply(TileCmd::CopyTilesetFor { layer }).expect("fits");
    e.apply(TileCmd::RemoveTile {
        tileset: set,
        tile: TileId::new(1).expect("id"),
    })
    .expect("removes");
    let mut states = 0;
    while e.undo() {
        assert_eq!(e.doc().stored_tile_bytes(), counted(e.doc()));
        states += 1;
    }
    assert_eq!(e.doc().stored_tile_bytes(), 0);
    while e.redo() {
        assert_eq!(e.doc().stored_tile_bytes(), counted(e.doc()));
    }
    assert_eq!(states, 6);
}

#[test]
fn a_storage_tile_is_charged_its_shared_allocation() {
    // `Arc<Tile>` is the tile and two counts; `heap_size` counts the overhead of a solid tile.
    let held = 2 * std::mem::size_of::<usize>() + std::mem::size_of::<crate::tile::Tile>();
    assert_eq!(
        crate::tile::Tile::Solid(1).heap_size() + STORAGE_TILE_EXTRA,
        held
    );
}

#[test]
fn the_largest_document_the_old_rule_allowed_fits_the_present_one() {
    let records = crate::tileset::MAX_TILESETS * MAX_TILES;
    // Every record a blank tile: the most records, and so the most the new charge adds.
    assert!(records * OLD_RECORD_BYTES <= 256 << 20);
    assert!(records * stored_tile_bytes(&Image::blank()) <= MAX_TILE_BYTES);
    const { assert!(OLD_RULE_WORST_CASE <= MAX_TILE_BYTES) };
    // And the limit is no looser than it must be, to the MiB.
    const { assert!(OLD_RULE_WORST_CASE > MAX_TILE_BYTES - (1 << 20)) };
}
