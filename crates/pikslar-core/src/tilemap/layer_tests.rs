//! Tilemap layers: making them, the tileset they use, and what removing a tile does to maps.

use super::fixture::{cell, corner, doc_with_map, editor_with_map, id, map_cel, tile_image};
use super::{MapCell, TilemapFault};
use crate::command::{TileCmd, TileSource};
use crate::document::{ColorMode, LimitError};
use crate::grid::Grid;
use crate::layer::LayerKind;
use crate::orient::Orientation;
use crate::tileset::{Tileset, TilesetId};
use crate::{Command, FrameCmd, LayerCmd};
use std::sync::Arc;

#[test]
fn a_new_tilemap_layer_is_active_blank_and_takes_the_tilesets_mode() {
    let (mut e, set) = editor_with_map();
    let depth = e.history().undo_depth();
    let out = e
        .apply(TileCmd::NewLayer {
            at: 2,
            tileset: set,
        })
        .expect("room");
    assert!(out.changed);
    assert_eq!(e.history().undo_depth(), depth + 1);
    let layers = e.doc().layers();
    assert_eq!(layers.len(), 3);
    assert_eq!(layers[2].kind, LayerKind::Tilemap(set));
    assert_eq!(layers[2].mode, ColorMode::Rgba);
    assert_eq!(e.layer, 2);
    assert!(layers[2].cels().iter().all(|c| c.is_blank()));
    assert_eq!(layers[2].frame_count(), e.doc().frames.len());
    assert_eq!(e.doc().tileset_users(set), 2);
}

#[test]
fn a_palette_tileset_makes_a_palette_layer() {
    let (mut e, _) = editor_with_map();
    let made = e
        .apply(TileCmd::NewTileset {
            name: "Pal".into(),
            mode: ColorMode::Palette,
            tile_w: 8,
            tile_h: 8,
        })
        .expect("room");
    let set = TilesetId::new(made.created.expect("id"));
    e.apply(TileCmd::NewLayer {
        at: 0,
        tileset: set,
    })
    .expect("room");
    assert_eq!(e.doc().layers()[0].mode, ColorMode::Palette);
    assert_eq!(e.doc().layers()[0].tileset(), Some(set));
}

#[test]
fn a_new_layer_undoes_and_redoes_to_the_same_document() {
    let (mut e, set) = editor_with_map();
    let before = e.doc().clone();
    e.apply(TileCmd::NewLayer {
        at: 1,
        tileset: set,
    })
    .expect("room");
    let after = e.doc().clone();
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
    assert!(e.redo());
    assert_eq!(e.doc(), &after);
}

#[test]
fn a_new_layer_for_a_tileset_that_is_not_there_is_refused_whole() {
    let (mut e, _) = editor_with_map();
    let before = e.doc().clone();
    let depth = e.history().undo_depth();
    let missing = TilesetId::new(99);
    let err = e.apply(TileCmd::NewLayer {
        at: 0,
        tileset: missing,
    });
    assert_eq!(err, Err(LimitError::NoSuchItem));
    assert_eq!(e.doc(), &before);
    assert_eq!(e.history().undo_depth(), depth);
}

#[test]
fn a_tileset_is_kept_when_its_last_layer_goes_and_refused_while_one_uses_it() {
    let (mut e, set) = editor_with_map();
    let layer = e.layer_id(1).expect("layer");
    assert_eq!(
        e.apply(TileCmd::RemoveTileset { tileset: set }),
        Err(LimitError::TilesetInUse)
    );
    e.apply(LayerCmd::Remove { layer }).expect("removes");
    assert_eq!(e.doc().tileset_users(set), 0);
    assert!(e.doc().tileset(set).is_ok(), "an unused tileset is listed");
    e.apply(TileCmd::RemoveTileset { tileset: set })
        .expect("unused");
    assert!(e.doc().tileset(set).is_err());
}

#[test]
fn the_tilemap_rules_hold_for_a_sound_document() {
    let (doc, _) = doc_with_map();
    assert_eq!(doc.check_tilemaps(), Ok(()));
}

#[test]
fn check_tilemaps_names_each_broken_rule() {
    let (doc, _) = doc_with_map();
    let mut d = doc.clone();
    d.layers_mut()[1].kind = LayerKind::Tilemap(TilesetId::new(77));
    assert_eq!(
        d.check_tilemaps(),
        Err(TilemapFault::NoTileset { layer: 1 })
    );

    let mut d = doc.clone();
    d.layers_mut()[1].mode = ColorMode::Palette;
    assert_eq!(d.check_tilemaps(), Err(TilemapFault::Mode { layer: 1 }));

    let mut d = doc.clone();
    d.put_cel(1, 2, map_cel([cell(9), 0, 0, 0])).expect("fits");
    assert_eq!(
        d.check_tilemaps(),
        Err(TilemapFault::UnknownTile { layer: 1, frame: 2 })
    );

    let mut d = doc.clone();
    let flagged = MapCell::from_bits(0x8000_0000).bits();
    d.put_cel(1, 2, map_cel([flagged, 0, 0, 0])).expect("fits");
    assert_eq!(
        d.check_tilemaps(),
        Err(TilemapFault::FlagsOnEmpty { layer: 1, frame: 2 })
    );
}

#[test]
fn only_the_two_flips_are_allowed_on_oblong_tiles() {
    let (mut d, _) = doc_with_map();
    let oblong = d
        .add_tileset("Bricks", ColorMode::Rgba, (4, 2))
        .expect("room");
    let brick = Grid::from_cells(4, 2, &[0xff00_00ff; 8]).into_image();
    d.add_tile(oblong, Arc::new(brick)).expect("fits");
    d.add_tilemap_layer(2, oblong).expect("room");
    let turned = MapCell::place(id(1), Orientation::rotate90()).bits();
    d.put_cel(2, 0, map_cel([turned, 0, 0, 0])).expect("fits");
    assert_eq!(
        d.check_tilemaps(),
        Err(TilemapFault::DiagonalOnOblong { layer: 2, frame: 0 })
    );
    let flipped = MapCell::place(id(1), Orientation::flip_horizontal()).bits();
    d.put_cel(2, 0, map_cel([flipped, 0, 0, 0])).expect("fits");
    assert_eq!(d.check_tilemaps(), Ok(()));
}

#[test]
fn removing_a_tile_empties_every_cell_that_placed_it_in_one_step() {
    let (mut e, set) = editor_with_map();
    // A second tilemap layer on the same tileset, and a turned placement of tile 1.
    e.apply(TileCmd::NewLayer {
        at: 2,
        tileset: set,
    })
    .expect("room");
    let turned = MapCell::place(id(1), Orientation::rotate270()).bits();
    e.doc
        .put_cel(2, 2, map_cel([turned, cell(2), 0, 0]))
        .expect("fits");
    let before = e.doc().clone();
    assert_eq!(e.doc().cells_using(set, id(1)), 4);
    let depth = e.history().undo_depth();
    e.apply(TileCmd::RemoveTile {
        tileset: set,
        tile: id(1),
    })
    .expect("there");
    assert_eq!(e.history().undo_depth(), depth + 1);
    let layers = e.doc().layers();
    assert_eq!(corner(layers[1].cel(0)), [0, cell(2), cell(2), 0]);
    assert_eq!(corner(layers[1].cel(1)), [cell(2), cell(2), 0, 0]);
    assert_eq!(corner(layers[2].cel(2)), [0, cell(2), 0, 0]);
    assert_eq!(e.doc().cells_using(set, id(1)), 0);
    assert_eq!(e.doc().check_tilemaps(), Ok(()));
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
    assert_eq!(e.doc().check_tilemaps(), Ok(()));
}

#[test]
fn removing_a_tile_keeps_a_hold_and_a_link_sharing_one_drawing() {
    let (mut e, set) = editor_with_map();
    let layer = e.layer_id(1).expect("layer");
    e.apply(FrameCmd::HoldAcross {
        layer,
        source: 0,
        from: 0,
        to: 2,
    })
    .expect("holds");
    e.apply(LayerCmd::Duplicate { layer }).expect("copies");
    assert!(e.doc().is_held(1, 0) && e.doc().is_held(2, 0));
    e.apply(TileCmd::RemoveTile {
        tileset: set,
        tile: id(1),
    })
    .expect("there");
    let layers = e.doc().layers();
    assert!(layers[1].cel(0).same_image(layers[1].cel(2)));
    assert!(layers[1].cel(0).same_image(layers[2].cel(0)));
    assert_eq!(corner(layers[2].cel(1)), [0, cell(2), cell(2), 0]);
    assert_eq!(e.doc().check_links(), Ok(()));
}

#[test]
fn removing_a_tile_nothing_places_touches_no_drawing() {
    let (mut e, set) = editor_with_map();
    e.apply(TileCmd::AddTile {
        tileset: set,
        from: TileSource::Blank,
    })
    .expect("fits");
    let images = |e: &crate::Editor| -> Vec<usize> {
        let cels = e.doc().layers()[1].cels();
        cels.iter().map(|c| c.image_id()).collect()
    };
    let before = images(&e);
    e.apply(TileCmd::RemoveTile {
        tileset: set,
        tile: id(3),
    })
    .expect("there");
    assert_eq!(before, images(&e));
}

#[test]
fn making_a_tileset_independent_copies_it_and_points_the_layer_at_the_copy() {
    let (mut e, set) = editor_with_map();
    let layer = e.layer_id(1).expect("layer");
    e.apply(LayerCmd::Duplicate { layer }).expect("copies");
    let before = e.doc().clone();
    let depth = e.history().undo_depth();
    let made = e.apply(TileCmd::CopyTilesetFor { layer }).expect("room");
    assert_eq!(e.history().undo_depth(), depth + 1);
    let copy = TilesetId::new(made.created.expect("an id"));
    assert_ne!(copy, set);
    let doc = e.doc();
    assert_eq!(doc.layers()[1].tileset(), Some(copy));
    assert_eq!(
        doc.layers()[2].tileset(),
        Some(set),
        "the other layer keeps its own"
    );
    let (old, new) = (
        doc.tileset(set).expect("old"),
        doc.tileset(copy).expect("new"),
    );
    let ids = |s: &Tileset| -> Vec<u32> { s.tiles().iter().map(|t| t.id.get()).collect() };
    assert_eq!(new.tile_size(), old.tile_size());
    assert_eq!(ids(new), ids(old));
    assert!(
        new.tiles()[0].image == old.tiles()[0].image,
        "images are shared until edited"
    );
    assert_eq!(new.next_tile_id(), old.next_tile_id());
    // The layer was linked to its duplicate, which a link across tilesets could not stay.
    assert_eq!(doc.check_links(), Ok(()));
    assert_eq!(doc.check_tilemaps(), Ok(()));
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
    assert!(e.redo());
    assert_eq!(e.doc().layers()[1].tileset(), Some(copy));
}

#[test]
fn editing_a_tile_of_the_copy_leaves_the_original_alone() {
    let (mut e, set) = editor_with_map();
    let layer = e.layer_id(1).expect("layer");
    let made = e.apply(TileCmd::CopyTilesetFor { layer }).expect("room");
    let copy = TilesetId::new(made.created.expect("an id"));
    e.apply(TileCmd::SetTileImage {
        tileset: copy,
        tile: id(1),
        image: tile_image(0xff12_3456),
    })
    .expect("fits");
    let old = e
        .doc()
        .tileset(set)
        .expect("old")
        .tile(id(1))
        .expect("tile");
    let new = e
        .doc()
        .tileset(copy)
        .expect("new")
        .tile(id(1))
        .expect("tile");
    assert_ne!(old.image, new.image);
}

#[test]
fn copying_the_tileset_of_a_paint_layer_is_refused() {
    let (mut e, _) = editor_with_map();
    let paint = e.layer_id(0).expect("layer");
    assert_eq!(
        e.apply(TileCmd::CopyTilesetFor { layer: paint }),
        Err(LimitError::NotTilemap)
    );
}

#[test]
fn a_run_of_tilemap_steps_undoes_and_redoes_state_by_state() {
    let (mut e, set) = editor_with_map();
    let layer = e.layer_id(1).expect("layer");
    let mut states = vec![e.doc().clone()];
    let steps: Vec<Command> = vec![
        TileCmd::NewLayer {
            at: 2,
            tileset: set,
        }
        .into(),
        TileCmd::CopyTilesetFor { layer }.into(),
        TileCmd::RemoveTile {
            tileset: set,
            tile: id(2),
        }
        .into(),
        LayerCmd::SetOpacity {
            layer,
            opacity: 100,
        }
        .into(),
    ];
    for step in steps {
        e.apply(step).expect("runs");
        states.push(e.doc().clone());
    }
    for want in states.iter().rev().skip(1) {
        assert!(e.undo());
        assert_eq!(e.doc(), want);
    }
    for want in states.iter().skip(1) {
        assert!(e.redo());
        assert_eq!(e.doc(), want);
    }
}
