//! Replace colour and the tilemap layers that place palette tiles: a locked layer that would
//! show another colour refuses the replacement, and one that would not does not.

use crate::color::rgba;
use crate::command::{PaletteCmd, TileCmd, TileSource};
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::Editor;
use crate::grid::Grid;
use crate::layer::Cel;
use crate::orient::Orientation;
use crate::tilemap::MapCell;
use crate::tileset::TileId;
use std::sync::Arc;

const RED: u32 = rgba(255, 0, 0, 255);
const GREEN: u32 = rgba(0, 255, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);

/// A palette document with swatches red, green and blue and a palette tileset of three tiles,
/// each all one colour, in that order. Layer 1 is a tilemap layer whose map places `places`.
/// Returns the editor and the ids of the three colours.
fn scene(places: &[u32]) -> (Editor, [u32; 3]) {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Palette).expect("valid"));
    let mut ids = [0; 3];
    for (slot, color) in ids.iter_mut().zip([RED, GREEN, BLUE]) {
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
        *slot = e.doc.colors.id_for_existing(color).expect("a swatch");
    }
    let made = e
        .apply(TileCmd::NewTileset {
            name: "T".into(),
            mode: ColorMode::Palette,
            tile_w: 4,
            tile_h: 4,
        })
        .expect("room");
    let set = crate::tileset::TilesetId::new(made.created.expect("an id"));
    for id in ids {
        let image = Arc::new(Grid::from_cells(4, 4, &[id; 16]).into_image());
        let from = TileSource::Image(image);
        e.apply(TileCmd::AddTile { tileset: set, from })
            .expect("fits");
    }
    e.apply(TileCmd::NewLayer {
        at: 1,
        tileset: set,
    })
    .expect("layer");
    let mut cells = [0u32; 4];
    for (slot, &tile) in cells.iter_mut().zip(places) {
        let id = TileId::new(tile).expect("a tile id");
        *slot = MapCell::place(id, Orientation::IDENTITY).bits();
    }
    let map = Cel::new(Grid::from_cells(2, 2, &cells));
    e.doc.put_cel(1, 0, map).expect("fits");
    (e, ids)
}

fn replace(e: &mut Editor, from: u32, to: u32) -> Result<bool, LimitError> {
    let command = PaletteCmd::ReplaceColor {
        from,
        to,
        layer: None,
    };
    e.apply(command).map(|out| out.changed)
}

#[test]
fn replacing_a_colour_is_refused_while_a_locked_map_places_a_tile_showing_it() {
    let (mut e, [red, green, _]) = scene(&[1, 2]);
    e.doc.layers[1].locked = true;
    let (before, depth) = (e.doc().clone(), e.history().undo_depth());
    assert_eq!(replace(&mut e, red, green), Err(LimitError::Locked));
    assert_eq!(e.doc(), &before, "nothing was half replaced");
    assert_eq!(e.history().undo_depth(), depth);
}

#[test]
fn a_group_lock_over_the_map_refuses_it_too() {
    let (mut e, [red, green, _]) = scene(&[1]);
    let layer = e.layer_id(1).expect("layer");
    e.apply(crate::LayerCmd::Group { layer }).expect("grouped");
    let group = e
        .doc
        .layers()
        .iter()
        .position(|l| l.kind == crate::LayerKind::Group)
        .expect("a group");
    e.doc.layers[group].locked = true;
    assert_eq!(replace(&mut e, red, green), Err(LimitError::Locked));
}

#[test]
fn a_locked_map_that_places_none_of_the_changing_tiles_does_not_refuse() {
    // The map places only the green and blue tiles; the red tile is unplaced.
    let (mut e, [red, green, _]) = scene(&[2, 3]);
    e.doc.layers[1].locked = true;
    assert_eq!(replace(&mut e, red, green), Ok(true));
    let set = e.doc.layers()[1].tileset().expect("tileset");
    let first = e.doc.tileset(set).expect("set").tiles()[0].image.get(0, 0);
    assert_eq!(first, green, "the tile image was replaced");
    assert!(e.undo());
    assert_eq!(
        e.doc.tileset(set).expect("set").tiles()[0].image.get(0, 0),
        red
    );
}

#[test]
fn an_unlocked_map_does_not_refuse_and_a_layer_scope_never_reaches_the_tiles() {
    let (mut e, [red, green, _]) = scene(&[1]);
    assert_eq!(replace(&mut e, red, green), Ok(true));
    let (mut e, [red, green, _]) = scene(&[1]);
    e.doc.layers[1].locked = true;
    let layer = e.layer_id(0).expect("layer");
    let command = PaletteCmd::ReplaceColor {
        from: red,
        to: green,
        layer: Some(layer),
    };
    // The paint layer shows no red, so nothing changes, and the locked map is not asked.
    assert_eq!(e.apply(command).map(|o| o.changed), Ok(false));
}
