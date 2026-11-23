//! The Tiles section: shown only while a tilemap layer is active, a click chooses a tile, a
//! Shift-click a block, a drag reorders, and J or the buttons switch between Tiles and Pixels.

use super::*;
use crate::test_app::{centre_of, click_whole, draw_whole, key, map_app, said, PLAIN, SHIFT};
use pikslar_core::orient::Orientation;
use pikslar_platform::{Button, Key};

fn has(a: &App, id: u32) -> bool {
    a.ui.widgets().iter().any(|&(_, seen)| seen == id)
}

fn tile_ids(a: &App, set: TilesetId) -> Vec<TileId> {
    let found = a.editor.doc().tileset(set).expect("the tileset");
    found.tiles().iter().map(|t| t.id).collect()
}

#[test]
fn a_block_is_the_rectangle_between_two_places_row_by_row() {
    assert_eq!(block_places(4, 5, 5), (1, 1, vec![5]));
    assert_eq!(block_places(4, 1, 6), (2, 2, vec![1, 2, 5, 6]));
    assert_eq!(
        block_places(4, 6, 1),
        (2, 2, vec![1, 2, 5, 6]),
        "either way round"
    );
    assert_eq!(block_places(3, 2, 3), (3, 2, vec![0, 1, 2, 3, 4, 5]));
}

#[test]
fn a_thumbnail_is_drawn_at_a_whole_number_zoom() {
    assert_eq!(thumb_zoom((4, 4), 30), (7, 1));
    assert_eq!(thumb_zoom((16, 8), 30), (1, 1));
    assert_eq!(thumb_zoom((8, 8), 32), (4, 1));
    assert_eq!(
        thumb_zoom((64, 64), 30),
        (1, 3),
        "every third pixel of a large tile"
    );
}

#[test]
fn a_tiles_pixel_shows_its_colour_or_nothing() {
    let (a, _) = map_app();
    let doc = a.editor.doc();
    assert_eq!(
        shown_color(doc, ColorMode::Rgba, 0xff12_3456),
        Some(0xff12_3456)
    );
    assert_eq!(shown_color(doc, ColorMode::Rgba, 0x0012_3456), None);
    assert_eq!(
        shown_color(doc, ColorMode::Palette, 0),
        None,
        "colour 0 is clear"
    );
}

#[test]
fn the_section_is_shown_only_while_a_tilemap_layer_is_active() {
    let (mut a, _) = map_app();
    for id in [ID_TILES, ID_TILES + 1, ID_TILE, ID_TILE + 1, ID_TILE + 2] {
        assert!(has(&a, id), "{id}");
    }
    assert!(!has(&a, ID_TILE + 3), "three tiles");
    a.editor.layer = 0;
    draw_whole(&mut a);
    assert!(
        !has(&a, ID_TILES) && !has(&a, ID_TILE),
        "a paint layer has no tiles"
    );
}

#[test]
fn a_click_chooses_a_tile_and_shift_click_a_block() {
    let (mut a, set) = map_app();
    let ids = tile_ids(&a, set);
    let depth = a.editor.history().undo_depth();
    let at = centre_of(&a, ID_TILE + 1);
    click_whole(&mut a, at, PLAIN);
    assert_eq!(
        a.editor.tiles().current(),
        Some((set, ids[1], Orientation::IDENTITY))
    );
    let at = centre_of(&a, ID_TILE + 2);
    click_whole(&mut a, at, SHIFT);
    assert_eq!(a.editor.tiles().current(), None, "a block, not one tile");
    let stamp = a.editor.tiles().stamp().expect("a block");
    assert_eq!(stamp.size(), (2, 1));
    assert_eq!(a.tile_ui.block, Some((1, 2)));
    assert_eq!(
        a.editor.history().undo_depth(),
        depth,
        "choosing is never a step"
    );
}

#[test]
fn dragging_a_thumbnail_moves_the_tile_and_leaves_its_id() {
    let (mut a, set) = map_app();
    let ids = tile_ids(&a, set);
    let depth = a.editor.history().undo_depth();
    let (from, to) = (centre_of(&a, ID_TILE), centre_of(&a, ID_TILE + 2));
    a.pointer_down(from.0, from.1, Button::Left, PLAIN);
    draw_whole(&mut a);
    a.ui.track(to.0, to.1);
    draw_whole(&mut a);
    a.pointer_up(to.0, to.1, Button::Left);
    draw_whole(&mut a);
    assert_eq!(tile_ids(&a, set), vec![ids[1], ids[2], ids[0]]);
    assert_eq!(a.editor.history().undo_depth(), depth + 1, "one step");
    a.undo();
    assert_eq!(tile_ids(&a, set), ids);
}

#[test]
fn j_and_the_buttons_switch_between_tiles_and_pixels() {
    let (mut a, _) = map_app();
    let depth = a.editor.history().undo_depth();
    assert_eq!(a.tile_ui.mode, TileMode::Tiles, "Tiles first");
    key(&mut a, Key::Char(b'J'), PLAIN);
    assert_eq!(a.tile_ui.mode, TileMode::Pixels);
    assert!(said(&a).starts_with("Pixels mode"), "{}", said(&a));
    draw_whole(&mut a);
    let at = centre_of(&a, ID_TILES);
    click_whole(&mut a, at, PLAIN);
    assert_eq!(a.tile_ui.mode, TileMode::Tiles);
    let at = centre_of(&a, ID_TILES + 1);
    click_whole(&mut a, at, PLAIN);
    assert_eq!(a.tile_ui.mode, TileMode::Pixels);
    assert_eq!(
        a.editor.history().undo_depth(),
        depth,
        "the mode is never a step"
    );
}

#[test]
fn more_tiles_than_the_rows_hold_scroll() {
    let (mut a, set) = map_app();
    for _ in 0..40 {
        a.editor
            .apply(TileCmd::AddTile {
                tileset: set,
                from: pikslar_core::TileSource::Blank,
            })
            .expect("room");
    }
    draw_whole(&mut a);
    let grid = a.tile_ui.grid.expect("drawn");
    assert!(grid.rows >= 1 && grid.rows <= MOST_ROWS);
    let shown = a.ui.widgets().iter().filter(|w| w.1 >= ID_TILE).count();
    assert!(shown < 43, "{shown} shown");
    let list = a.tile_ui.list;
    a.panel_wheel(-2, list.x + 2, list.y + 2);
    draw_whole(&mut a);
    assert_eq!(a.tile_ui.grid.map(|g| g.first), Some(2));
}

#[test]
fn a_tiles_panel_with_no_tileset_says_so_instead_of_staying_blank() {
    use crate::workspace::model::Preset;
    let (w, h) = (1366, 705);
    let mut a = crate::test_app::app(1);
    a.resized(w, h, 1);
    a.choose_preset(Preset::Tiles);
    assert!(a.active_tileset().is_none());
    let mut px = vec![0u32; (w * h) as usize];
    for pass in 0..2 {
        pikslar_ui::font::log_text(pass == 1);
        let mut surface = pikslar_render::Surface { px: &mut px, w, h };
        a.draw_interface(
            &mut surface,
            pikslar_platform::Window::detached(),
            &mut Vec::new(),
        );
    }
    let log = pikslar_ui::font::take_text_log();
    pikslar_ui::font::log_text(false);
    let said: Vec<&str> = log.texts.iter().map(|(_, t)| t.as_str()).collect();
    assert!(said.contains(&"No tileset yet"), "{said:?}");
}
