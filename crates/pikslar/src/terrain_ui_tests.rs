//! The terrain row and slot view: shown once the tileset has a terrain set, a slot is filled by
//! choosing a tile and clicking it, and with a terrain chosen the tools paint it on the canvas,
//! with the cells the stroke changes drawn before it commits.

use super::*;
use crate::app::ToolChoice;
use crate::panel_tiles::ID_TILE;
use crate::test_app::TILE_COLORS;
use crate::test_app::{centre_of, click_whole, draw_whole, map_app, said, screen, ALT, PLAIN};
use pikslar_core::tileset::TileId;

fn has(a: &App, id: u32) -> bool {
    a.ui.widgets().iter().any(|&(_, seen)| seen == id)
}

fn tiles(a: &App, set: TilesetId) -> Vec<TileId> {
    let found = a.editor.doc().tileset(set).expect("set");
    found.tiles().iter().map(|t| t.id).collect()
}

/// The colour the tilemap layer shows at a canvas pixel.
fn shown(a: &App, x: i32, y: i32) -> u32 {
    let doc = a.editor.doc();
    doc.layer_pixel(&doc.layers()[a.editor.layer], a.editor.frame, x, y)
}

fn drag(a: &mut App, from: (i32, i32), to: (i32, i32), button: Button, mods: Mods) {
    let (fx, fy) = screen(a, from.0, from.1);
    let (tx, ty) = screen(a, to.0, to.1);
    a.pointer_down(fx, fy, button, mods);
    a.ui.track(tx, ty);
    a.canvas_move(tx, ty);
    a.pointer_up(tx, ty, button);
    draw_whole(a);
}

/// An Edge4 set whose isolated slot shows tile 0, "east" tile 1 and "west" tile 2, chosen to
/// paint with.
fn edge_app() -> (App, TilesetId, u16) {
    let (mut a, set) = map_app();
    a.new_terrain(0);
    let terrain = a.tile_ui.terrain.chosen.expect("made and chosen").1;
    let ids = tiles(&a, set);
    for (slot, tile) in [(0u8, ids[0]), (2, ids[1]), (8, ids[2])] {
        a.editor
            .apply(TerrainCmd::Assign {
                tileset: set,
                set: terrain,
                slot,
                tile,
            })
            .expect("assigned");
    }
    a.tile_ui.terrain.slots = false;
    draw_whole(&mut a);
    (a, set, terrain)
}

#[test]
fn a_slot_stands_for_its_mask() {
    assert_eq!(slot_mask(TerrainKind::Edge4, 5), N | S);
    assert_eq!(slot_mask(TerrainKind::Edge4, 10), E | W);
    assert_eq!(slot_mask(TerrainKind::Corner4, 15), NE | SE | SW | NW);
    assert_eq!(slot_mask(TerrainKind::Corner4, 2), SE);
    assert_eq!(slot_mask(TerrainKind::Blob47, 0), 0);
    assert_eq!(slot_mask(TerrainKind::Blob47, 46), 255);
    assert_eq!(slots_per_row(TerrainKind::Edge4, 7), 4, "the 4 by 4 table");
    assert_eq!(slots_per_row(TerrainKind::Blob47, 7), 7);
    assert_eq!(kind_name(TerrainKind::Corner4), "Corner4");
}

#[test]
fn the_terrain_row_is_shown_once_the_tileset_has_a_terrain_set() {
    let (mut a, _) = map_app();
    assert!(!has(&a, ID_TERRAIN) && !has(&a, ID_TERRAIN + 1));
    let before = a.tiles_head();
    a.new_terrain(2);
    draw_whole(&mut a);
    assert!(has(&a, ID_TERRAIN) && has(&a, ID_TERRAIN + 1));
    assert_eq!(a.tiles_head(), before + TERRAIN_ROW);
    a.undo();
    draw_whole(&mut a);
    assert!(!has(&a, ID_TERRAIN), "gone with the set");
    assert!(a.terrain_tool(Tool::Pencil, Button::Left, PLAIN).is_none());
}

#[test]
fn the_slot_view_shows_every_slot_and_a_click_puts_the_chosen_tile_there() {
    let (mut a, set, terrain) = edge_app();
    let at = centre_of(&a, ID_TERRAIN + 1);
    click_whole(&mut a, at, PLAIN);
    draw_whole(&mut a);
    assert!(a.showing_slots());
    let rows = a.tile_ui.terrain.grid.map_or(0, |g| g.rows);
    let shown = (rows * 4) as u32;
    assert!(rows >= 1, "a row of slots at least");
    assert!((0..shown).all(|s| has(&a, ID_SLOT + s)), "four to a row");
    assert!(!has(&a, ID_TILE), "in place of the tiles");
    if rows < 4 {
        let list = a.tile_ui.list;
        a.panel_wheel(-4, list.x + 2, list.y + 2);
        draw_whole(&mut a);
        assert!(has(&a, ID_SLOT + 15), "the last row scrolls into view");
        a.panel_wheel(4, list.x + 2, list.y + 2);
        draw_whole(&mut a);
    }
    let at = centre_of(&a, ID_SLOT + 5);
    click_whole(&mut a, at, PLAIN);
    assert!(said(&a).starts_with("Choose a tile first"), "{}", said(&a));
    let tile = tiles(&a, set)[1];
    a.editor.set_current_tile(set, tile).expect("chosen");
    let depth = a.editor.history().undo_depth();
    click_whole(&mut a, at, PLAIN);
    let found = a.editor.doc().terrain(set, terrain).expect("set");
    assert_eq!(found.slot(5), Some(tile));
    assert_eq!(a.editor.history().undo_depth(), depth + 1, "one step");
    assert_eq!(a.slot_at(at.0, at.1), Some(5));
    let at = centre_of(&a, ID_TERRAIN + 1);
    click_whole(&mut a, at, PLAIN);
    draw_whole(&mut a);
    assert!(!a.showing_slots() && has(&a, ID_TILE), "the tiles again");
}

#[test]
fn with_a_terrain_chosen_the_pencil_paints_it_and_the_eraser_clears_it() {
    let (mut a, set, _) = edge_app();
    let depth = a.editor.history().undo_depth();
    drag(&mut a, (1, 1), (5, 1), Button::Left, PLAIN);
    assert_eq!(shown(&a, 1, 1), TILE_COLORS[1], "joined to the east");
    assert_eq!(shown(&a, 5, 1), TILE_COLORS[2], "joined to the west");
    assert_eq!(a.editor.history().undo_depth(), depth + 1, "one step");
    a.tool = ToolChoice::Paint(Tool::Eraser);
    drag(&mut a, (5, 1), (5, 1), Button::Left, PLAIN);
    assert_eq!(shown(&a, 5, 1), 0, "cleared");
    assert_eq!(shown(&a, 1, 1), TILE_COLORS[0], "alone now");
    a.tool = ToolChoice::Paint(Tool::Pencil);
    drag(&mut a, (5, 1), (5, 1), Button::Left, PLAIN);
    drag(&mut a, (5, 1), (5, 1), Button::Right, PLAIN);
    assert_eq!(shown(&a, 5, 1), 0, "the right button clears");
    let _ = set;
}

#[test]
fn the_tools_take_the_terrain_shapes_and_alt_retiles() {
    let (a, set, terrain) = edge_app();
    let shape = |tool, button, mods| match a.terrain_tool(tool, button, mods) {
        Some(TileTool::Terrain(t)) => {
            assert_eq!((t.tileset, t.set), (set, terrain));
            Some((t.shape, t.clear))
        }
        _ => None,
    };
    let left = Button::Left;
    assert_eq!(
        shape(Tool::Pencil, left, PLAIN),
        Some((TerrainShape::Brush, false))
    );
    assert_eq!(
        shape(Tool::Line, left, PLAIN),
        Some((TerrainShape::Brush, false))
    );
    assert_eq!(
        shape(Tool::Eraser, left, PLAIN),
        Some((TerrainShape::Brush, true))
    );
    assert_eq!(
        shape(Tool::Rect, left, PLAIN),
        Some((TerrainShape::Rect, false))
    );
    assert_eq!(
        shape(Tool::Fill, left, PLAIN),
        Some((TerrainShape::Fill, false))
    );
    assert_eq!(
        shape(Tool::Pencil, left, ALT),
        Some((TerrainShape::Retile, false))
    );
    assert_eq!(
        shape(Tool::Fill, Button::Right, PLAIN),
        Some((TerrainShape::Brush, true))
    );
    assert_eq!(
        shape(Tool::Picker, left, PLAIN),
        None,
        "the picker picks tiles"
    );
}

#[test]
fn alt_click_re_picks_a_hand_placed_cell() {
    let (mut a, set, _) = edge_app();
    drag(&mut a, (1, 1), (5, 1), Button::Left, PLAIN);
    let ids = tiles(&a, set);
    a.editor.set_current_tile(set, ids[0]).expect("chosen");
    a.editor
        .apply(pikslar_core::TileCmd::Stroke {
            tool: TileTool::Place,
            path: vec![(1, 1)],
        })
        .expect("placed by hand");
    assert_eq!(shown(&a, 1, 1), TILE_COLORS[0], "pinned as placed");
    a.tile_ui.terrain.painting = true;
    drag(&mut a, (1, 1), (1, 1), Button::Left, ALT);
    assert_eq!(
        shown(&a, 1, 1),
        TILE_COLORS[1],
        "re-picked from its neighbours"
    );
}

#[test]
fn choosing_or_picking_a_tile_stops_painting_terrain() {
    let (mut a, set, _) = edge_app();
    assert!(a.tile_ui.terrain.painting);
    a.choose_tile(set, 0, false);
    assert!(!a.tile_ui.terrain.painting);
    assert!(a.terrain_tool(Tool::Pencil, Button::Left, PLAIN).is_none());
    a.tile_ui.terrain.painting = true;
    a.tool = ToolChoice::Paint(Tool::Picker);
    drag(&mut a, (1, 1), (1, 1), Button::Left, PLAIN);
    assert!(!a.tile_ui.terrain.painting);
}

#[test]
fn a_terrain_stroke_in_progress_shows_the_cells_it_changes() {
    let (mut a, _, _) = edge_app();
    let (fx, fy) = screen(&a, 1, 1);
    let (tx, ty) = screen(&a, 5, 1);
    a.pointer_down(fx, fy, Button::Left, PLAIN);
    a.ui.track(tx, ty);
    a.canvas_move(tx, ty);
    assert!(a.tile_ui.map_stroke);
    draw_whole(&mut a);
    let cells = a.pending_now();
    let rects: Vec<_> = cells.iter().map(|c| c.rect).collect();
    assert_eq!(
        rects,
        vec![(0, 0, 4, 4), (4, 0, 4, 4)],
        "both painted cells"
    );
    assert!(cells.iter().all(|c| !c.cell.is_empty()));
    a.pointer_up(tx, ty, Button::Left);
    draw_whole(&mut a);
    assert!(a.pending_now().is_empty(), "nothing after the commit");
}
