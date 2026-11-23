//! The terrain menus: New terrain set from the tileset menu, choosing, renaming and removing a
//! set, the Corner4 corner mode, and a slot's menu. Each change is one step.

use super::*;
use crate::terrain_ui::{ID_SLOT, ID_TERRAIN};
use crate::test_app::{centre_of, click_whole, draw_whole, key, map_app, PLAIN};
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

/// Chooses row `n` of the open menu, as a click on it does.
fn choose(a: &mut App, n: usize) {
    let open = a.menu.take().expect("a menu is open");
    a.tile_menu_action(open.kind, n, open.at);
    draw_whole(a);
}

#[test]
fn new_terrain_set_from_the_tileset_menu_with_a_template_is_one_step() {
    let (mut a, set) = map_app();
    let header = a.tile_ui.header;
    right_click(&mut a, (header.x + 4, header.y + 4));
    assert_eq!(
        labels(&a).last().map(String::as_str),
        Some("New terrain set...")
    );
    choose(&mut a, 4);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::TerrainNew));
    assert_eq!(labels(&a).len(), 6);
    let (depth, tiles) = (a.editor.history().undo_depth(), 3);
    choose(&mut a, 5);
    let found = a.editor.doc().tileset(set).expect("set");
    assert_eq!(found.terrains().len(), 1);
    assert_eq!(found.terrains()[0].kind(), TerrainKind::Blob47);
    assert_eq!(found.terrains()[0].name(), "Terrain 1");
    assert_eq!(found.len(), tiles + 47, "a blank tile for every slot");
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    let ui = a.tile_ui.terrain;
    assert!(ui.painting && ui.slots && ui.chosen.is_some());
    assert!(a.showing_slots());
    let first = a.tile_entries(MenuKind::Terrain)[0].0.to_string();
    assert_eq!(first, "Paint Terrain 1 (Blob47)", "no slot missing");
}

#[test]
fn the_terrain_menu_chooses_renames_and_removes_sets() {
    let (mut a, set) = map_app();
    a.new_terrain(0);
    a.new_terrain(1);
    draw_whole(&mut a);
    let at = centre_of(&a, ID_TERRAIN);
    click_whole(&mut a, at, PLAIN);
    let rows = labels(&a);
    assert_eq!(rows[0], "Paint Terrain 1 (Edge4, 16 missing)");
    assert_eq!(rows[1], "Paint Terrain 2 (Corner4, 16 missing)");
    choose(&mut a, 0);
    let first = a.editor.doc().tileset(set).expect("set").terrains()[0].id();
    assert_eq!(a.tile_ui.terrain.chosen, Some((set, first)));
    assert!(a.tile_ui.terrain.painting);
    // Paint tiles, not terrain.
    a.open_terrain_menu((10, 10));
    choose(&mut a, 2);
    assert!(!a.tile_ui.terrain.painting);
    // Rename: typed in the terrain row, Enter keeps it.
    a.open_terrain_menu((10, 10));
    choose(&mut a, 4);
    assert_eq!(
        a.tile_ui.naming.as_ref().and_then(|n| n.terrain),
        Some(first)
    );
    key(&mut a, Key::Backspace, PLAIN);
    a.text_typed('X');
    key(&mut a, Key::Enter, PLAIN);
    let name = a
        .editor
        .doc()
        .terrain(set, first)
        .expect("set")
        .name()
        .to_string();
    assert_eq!(name, "Terrain X");
    // Remove leaves the other set and every tile.
    let tiles = a.editor.doc().tileset(set).expect("set").len();
    a.open_terrain_menu((10, 10));
    choose(&mut a, 5);
    let found = a.editor.doc().tileset(set).expect("set");
    assert_eq!(found.terrains().len(), 1);
    assert_eq!(found.len(), tiles);
}

#[test]
fn corner4_sets_toggle_between_cells_and_corners() {
    let (mut a, _) = map_app();
    a.new_terrain(0);
    a.open_terrain_menu((10, 10));
    let entries = a.tile_entries(MenuKind::Terrain);
    assert!(!entries[5].1, "only for a Corner4 set");
    a.new_terrain(1);
    let entries = a.tile_entries(MenuKind::Terrain);
    assert!(entries[6].1);
    assert_eq!(a.tile_ui.terrain.corners, Corners::Cell);
    choose(&mut a, 6);
    assert_eq!(a.tile_ui.terrain.corners, Corners::Vertex);
    match a.terrain_tool(pikslar_core::Tool::Pencil, Button::Left, PLAIN) {
        Some(pikslar_core::TileTool::Terrain(t)) => assert_eq!(t.corners, Corners::Vertex),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_slot_menu_puts_the_chosen_tile_and_clears_the_slot() {
    let (mut a, set) = map_app();
    a.new_terrain(0);
    draw_whole(&mut a);
    let terrain = a.tile_ui.terrain.chosen.expect("chosen").1;
    let at = centre_of(&a, ID_SLOT + 3);
    right_click(&mut a, at);
    assert_eq!(a.menu.map(|m| m.kind), Some(MenuKind::TerrainSlot));
    let entries = a.tile_entries(MenuKind::TerrainSlot);
    assert!(
        !entries[0].1 && !entries[1].1,
        "no tile chosen, nothing to clear"
    );
    let tile = a.editor.doc().tileset(set).expect("set").tiles()[2].id;
    a.editor.set_current_tile(set, tile).expect("chosen");
    choose(&mut a, 0);
    assert_eq!(
        a.editor.doc().terrain(set, terrain).expect("set").slot(3),
        Some(tile)
    );
    right_click(&mut a, at);
    choose(&mut a, 1);
    assert_eq!(
        a.editor.doc().terrain(set, terrain).expect("set").slot(3),
        None
    );
}
