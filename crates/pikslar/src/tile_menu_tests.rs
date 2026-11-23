//! The tile, tileset and New tilemap layer menus: each choice is one step, Remove says first how
//! many cells it empties, and a tileset's name is typed in the Tiles header.

use super::*;
use crate::panel_tiles::ID_TILE;
use crate::test_app::{centre_of, draw_whole, key, map_app, said, PLAIN};
use pikslar_platform::{Button, Key};

fn right_click(a: &mut App, at: (i32, i32)) {
    a.pointer_down(at.0, at.1, Button::Right, PLAIN);
    draw_whole(a);
}

fn labels(a: &App) -> Vec<String> {
    let kind = a.menu.expect("a menu is open").kind;
    a.tile_entries(kind)
        .into_iter()
        .map(|(l, _)| l.into_owned())
        .collect()
}

fn ids(a: &App, set: TilesetId) -> Vec<TileId> {
    let found = a.editor.doc().tileset(set).expect("set");
    found.tiles().iter().map(|t| t.id).collect()
}

/// Places tile `n` of the set in the top left cell and the one right of it.
fn place_two(a: &mut App, set: TilesetId, n: usize) {
    let tile = ids(a, set)[n];
    a.editor.set_current_tile(set, tile).expect("chosen");
    a.editor
        .apply(TileCmd::Stroke {
            tool: pikslar_core::TileTool::Place,
            path: vec![(1, 1), (5, 1)],
        })
        .expect("placed");
}

#[test]
fn remove_says_how_many_cells_it_empties_before_and_after() {
    let (mut a, set) = map_app();
    place_two(&mut a, set, 1);
    let at = centre_of(&a, ID_TILE + 1);
    right_click(&mut a, at);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Tile));
    assert_eq!(labels(&a)[3], "Remove tile, emptying 2 cell(s)");
    let depth = a.editor.history().undo_depth();
    a.tile_menu_action(MenuKind::Tile, 3, (0, 0));
    assert_eq!(said(&a), "Removed the tile and emptied 2 cell(s)");
    assert_eq!(ids(&a, set).len(), 2);
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    assert_eq!(
        a.editor.tiles().current(),
        None,
        "the stamp named the tile removed"
    );
}

#[test]
fn new_and_duplicate_make_a_tile_and_choose_it() {
    let (mut a, set) = map_app();
    let at = centre_of(&a, ID_TILE);
    right_click(&mut a, at);
    a.close_menu();
    a.tile_menu_action(MenuKind::Tile, 2, (0, 0));
    let now = ids(&a, set);
    assert_eq!(now.len(), 4);
    let copy = now[1];
    assert_eq!(
        a.editor.tiles().current().map(|c| c.1),
        Some(copy),
        "next to the original"
    );
    a.tile_ui.menu_tile = None;
    a.tile_menu_action(MenuKind::Tile, 0, (0, 0));
    assert_eq!(ids(&a, set).len(), 5);
    let entries = a.tile_entries(MenuKind::Tile);
    assert!(
        !entries[2].1 && !entries[3].1,
        "no tile to duplicate or remove"
    );
    assert!(!entries[1].1, "nothing selected to cut tiles from");
}

#[test]
fn the_new_layer_button_offers_a_tileset_or_a_new_tile_size() {
    let (mut a, set) = map_app();
    a.editor.layer = 0;
    draw_whole(&mut a);
    let plus = a.tile_ui.new_layer;
    right_click(&mut a, (plus.x + plus.w / 2, plus.y + plus.h / 2));
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::NewTilemap));
    assert_eq!(labels(&a)[0], "Tilemap of Floor 4x4");
    assert_eq!(labels(&a)[2], "Tilemap, new tileset 16x16");
    let depth = a.editor.history().undo_depth();
    a.tile_menu_action(MenuKind::NewTilemap, 0, (0, 0));
    assert_eq!(
        a.active_tileset(),
        Some(set),
        "a second layer of the same tileset"
    );
    assert_eq!(a.editor.doc().tileset_users(set), 2);
    a.tile_menu_action(MenuKind::NewTilemap, 3, (0, 0));
    let made = a.active_tileset().expect("a tilemap layer");
    assert_ne!(made, set);
    let size = a.editor.doc().tileset(made).expect("made").tile_size();
    assert_eq!(size, (32, 32));
    assert_eq!(a.editor.history().undo_depth(), depth + 2, "a step each");
}

#[test]
fn make_independent_and_remove_unused_are_offered_only_when_they_apply() {
    let (mut a, set) = map_app();
    let entries = a.tile_entries(MenuKind::Tileset);
    assert!(!entries[row::INDEPENDENT].1 && !entries[row::UNUSED].1);
    a.tile_menu_action(MenuKind::NewTilemap, 0, (0, 0));
    assert!(a.tile_entries(MenuKind::Tileset)[row::INDEPENDENT].1);
    a.tile_menu_action(MenuKind::Tileset, row::INDEPENDENT, (0, 0));
    let own = a.active_tileset().expect("a map");
    assert_ne!(own, set, "this layer has its own copy");
    // A tileset no layer uses can be removed by name.
    a.run(TileCmd::NewTileset {
        name: "Spare".into(),
        mode: pikslar_core::ColorMode::Rgba,
        tile_w: 8,
        tile_h: 8,
    });
    assert_eq!(a.tile_entries(MenuKind::TileUnused)[0].0, "Remove Spare");
    let count = a.editor.doc().tilesets.len();
    a.tile_menu_action(MenuKind::TileUnused, 0, (0, 0));
    assert_eq!(a.editor.doc().tilesets.len(), count - 1);
}

#[test]
fn a_tileset_is_renamed_in_the_header_as_one_step() {
    let (mut a, set) = map_app();
    let header = a.tile_ui.header;
    right_click(&mut a, (header.x + 4, header.y + 4));
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Tileset));
    a.close_menu();
    let depth = a.editor.history().undo_depth();
    a.tile_menu_action(MenuKind::Tileset, row::RENAME, (0, 0));
    assert!(a.tile_ui.naming.is_some());
    for _ in 0..5 {
        key(&mut a, Key::Backspace, PLAIN);
    }
    for c in *b"WALL" {
        key(&mut a, Key::Char(c), PLAIN);
    }
    assert_eq!(
        a.editor.history().undo_depth(),
        depth,
        "nothing kept while typing"
    );
    key(&mut a, Key::Enter, PLAIN);
    assert_eq!(a.editor.doc().tileset(set).expect("set").name(), "wall");
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    a.tile_menu_action(MenuKind::Tileset, row::RENAME, (0, 0));
    key(&mut a, Key::Escape, PLAIN);
    assert!(a.tile_ui.naming.is_none());
    assert_eq!(a.editor.doc().tileset(set).expect("set").name(), "wall");
}

#[test]
fn a_right_click_on_the_tileset_name_opens_the_tileset_menu_not_the_hidden_palettes() {
    use crate::frame_menu::MenuKind;
    let mut a = crate::test_app::app(1);
    a.resized(1400, 900, 1);
    // The Palette is drawn first, as it is before a map layer is made.
    crate::test_app::draw_whole_at(&mut a, 1400, 900);
    crate::layout_tests::add_map(&mut a, 2);
    crate::test_app::draw_whole_at(&mut a, 1400, 900);
    crate::test_app::draw_whole_at(&mut a, 1400, 900);
    let header = a.tile_ui.header;
    assert!(header.w > 0);
    a.pointer_down(
        header.x + 10,
        header.y + header.h / 2,
        pikslar_platform::Button::Right,
        crate::test_app::PLAIN,
    );
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::Tileset));
}

#[test]
fn a_tab_that_hides_the_tiles_panel_leaves_no_tileset_header_to_right_click() {
    let mut a = crate::test_app::app(1);
    a.resized(1400, 900, 1);
    crate::layout_tests::add_map(&mut a, 2);
    crate::test_app::draw_whole_at(&mut a, 1400, 900);
    assert!(a.tile_ui.header.w > 0);
    a.tiles_shown = false;
    crate::test_app::draw_whole_at(&mut a, 1400, 900);
    crate::test_app::draw_whole_at(&mut a, 1400, 900);
    assert!(a.active_tileset().is_some());
    assert_eq!(a.tile_ui.header.w, 0, "Palette shown, Tiles hidden");
}
