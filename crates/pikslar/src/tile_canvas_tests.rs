//! A tilemap layer on the canvas: Tiles mode places, clears, picks and turns tiles with the
//! usual tool keys; Pixels mode draws on the tiles and says when pixels found no tile; the grid
//! and a stroke's cells are shown.

use super::*;
use crate::app::ToolChoice;
use crate::panel_tiles::TileMode;
use crate::test_app::{draw_whole, key, map_app, said, screen, ALT, PLAIN, SHIFT, TILE_COLORS};
use pikslar_core::orient::Orientation;
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{Mask, SelectOp, SelectionCmd};
use pikslar_platform::Key;
use pikslar_ui::theme;

fn first_tile(a: &App, set: TilesetId) -> TileId {
    a.editor.doc().tileset(set).expect("set").tiles()[0].id
}

/// The colour a tilemap layer shows at a canvas pixel.
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

#[test]
fn every_paint_tool_has_a_tile_tool() {
    assert_eq!(map_tool(Tool::Pencil), TileTool::Place);
    assert_eq!(map_tool(Tool::Eraser), TileTool::Erase);
    assert_eq!(map_tool(Tool::Fill), TileTool::Fill);
    assert_eq!(map_tool(Tool::Picker), TileTool::Pick);
    assert_eq!(map_tool(Tool::Line), TileTool::Line);
    assert_eq!(map_tool(Tool::Rect), TileTool::Rect);
}

#[test]
fn the_pencil_places_tiles_along_a_drag_as_one_step() {
    let (mut a, set) = map_app();
    a.editor
        .set_current_tile(set, first_tile(&a, set))
        .expect("a tile");
    let depth = a.editor.history().undo_depth();
    drag(&mut a, (1, 1), (9, 1), Button::Left, PLAIN);
    for x in [1, 5, 9] {
        assert_eq!(shown(&a, x, 1), TILE_COLORS[0], "{x}");
    }
    assert_eq!(shown(&a, 1, 5), 0, "the next row is empty");
    assert_eq!(a.editor.history().undo_depth(), depth + 1);
    assert!(!a.tile_ui.map_stroke);
}

#[test]
fn a_right_press_clears_and_alt_picks_the_tile_and_its_turn() {
    let (mut a, set) = map_app();
    let tile = first_tile(&a, set);
    a.editor.set_current_tile(set, tile).expect("a tile");
    a.editor
        .turn_tile_stamp(Turn::FlipHorizontal)
        .expect("turns");
    drag(&mut a, (1, 1), (5, 1), Button::Left, PLAIN);
    a.editor.clear_tile_stamp();
    drag(&mut a, (5, 1), (5, 1), Button::Left, ALT);
    let picked = a.editor.tiles().current();
    assert_eq!(picked, Some((set, tile, Orientation::flip_horizontal())));
    drag(&mut a, (1, 1), (1, 1), Button::Right, PLAIN);
    assert_eq!(shown(&a, 1, 1), 0, "cleared");
    assert_eq!(shown(&a, 5, 1), TILE_COLORS[0], "kept");
}

#[test]
fn the_turn_keys_turn_the_stamp_and_not_the_layer() {
    let (mut a, set) = map_app();
    let tile = first_tile(&a, set);
    a.editor.set_current_tile(set, tile).expect("a tile");
    let doc = a.editor.doc().clone();
    key(&mut a, Key::Char(b'H'), SHIFT);
    let turned = a.editor.tiles().current();
    assert_eq!(turned, Some((set, tile, Orientation::flip_horizontal())));
    assert_eq!(a.editor.doc(), &doc, "nothing on the map changed");
}

#[test]
fn pixels_over_empty_cells_say_to_place_tiles_first() {
    let (mut a, set) = map_app();
    a.set_tile_mode(TileMode::Pixels);
    a.editor.color = 0xff12_3456;
    drag(&mut a, (1, 1), (2, 1), Button::Left, PLAIN);
    assert_eq!(said(&a), format!("Error: {PLACE_TILES_FIRST}"));
    // Over a placed tile the pixels go into the tile.
    a.set_tile_mode(TileMode::Tiles);
    a.editor
        .set_current_tile(set, first_tile(&a, set))
        .expect("a tile");
    drag(&mut a, (9, 9), (9, 9), Button::Left, PLAIN);
    a.set_tile_mode(TileMode::Pixels);
    a.message = None;
    drag(&mut a, (9, 9), (9, 9), Button::Left, PLAIN);
    assert_eq!(shown(&a, 9, 9), 0xff12_3456);
    assert_eq!(said(&a), "");
}

#[test]
fn delete_in_tiles_mode_empties_the_selected_cells() {
    let (mut a, set) = map_app();
    a.editor
        .set_current_tile(set, first_tile(&a, set))
        .expect("a tile");
    drag(&mut a, (1, 1), (9, 1), Button::Left, PLAIN);
    a.editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(0, 0, 8, 8)),
            op: SelectOp::Replace,
        })
        .expect("select");
    key(&mut a, Key::Delete, PLAIN);
    assert_eq!(shown(&a, 1, 1), 0);
    assert_eq!(shown(&a, 9, 1), TILE_COLORS[0], "outside the selection");
}

/// The screen pixels of the canvas after drawing the whole interface.
fn pixels(a: &mut App) -> Vec<u32> {
    let (w, h) = crate::test_app::WINDOW;
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = Surface { px: &mut px, w, h };
    a.full_canvas = true;
    a.draw_interface(
        &mut surface,
        pikslar_platform::Window::detached(),
        &mut Vec::new(),
    );
    a.draw_canvas(&mut surface, &mut Vec::new());
    px
}

#[test]
fn the_canvas_shows_the_grid_and_the_cells_of_a_stroke_in_progress() {
    let (mut a, set) = map_app();
    a.tool = ToolChoice::Paint(Tool::Pencil);
    let px = pixels(&mut a);
    let w = crate::test_app::WINDOW.0;
    let at = |px: &[u32], (x, y): (i32, i32)| px[(y * w + x) as usize];
    // The left edge of cell column 1 is the screen column of canvas x 4.
    let (gx, gy) = screen(&a, 4, 1);
    let cell = a.view.num / a.view.den;
    assert_eq!(at(&px, (gx - cell / 2, gy)), theme::muted(), "a grid line");
    a.editor
        .set_current_tile(set, first_tile(&a, set))
        .expect("a tile");
    let (x, y) = screen(&a, 1, 1);
    a.pointer_down(x, y, Button::Left, PLAIN);
    assert!(a.tile_ui.map_stroke);
    let px = pixels(&mut a);
    let red = crate::color_ui::to_screen(TILE_COLORS[0]);
    assert_eq!(
        at(&px, screen(&a, 2, 2)),
        red,
        "the placed cell shows before letting go"
    );
    assert_eq!(shown(&a, 2, 2), 0, "and is not in the map yet");
    a.pointer_up(x, y, Button::Left);
    assert_eq!(shown(&a, 2, 2), TILE_COLORS[0]);
    a.editor.layer = 0;
    let px = pixels(&mut a);
    assert_ne!(
        at(&px, (gx - cell / 2, gy)),
        theme::muted(),
        "no grid over a paint layer"
    );
}
