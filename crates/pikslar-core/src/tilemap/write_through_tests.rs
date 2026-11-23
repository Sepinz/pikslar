//! Editing tiles through the canvas: every orientation, the last-write rule, one undo step,
//! pixels over empty cells, links and holds, palette tilesets, locks and the redraw.

use super::write_through;
use crate::color::rgba;
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::{Damage, Editor, Tool};
use crate::grid::Grid;
use crate::image::Image;
use crate::layer::Cel;
use crate::orient::Orientation;
use crate::overlay::Overlay;
use crate::tilemap::fixture::{cell, editor_with_map, id, map_cel, TILE};
use crate::tilemap::MapCell;
use crate::tileset::TilesetId;
use crate::{DrawCmd, FrameCmd, LayerCmd};
use std::sync::Arc;

const PAINT: u32 = 0xff12_3456;

/// A tile of `w` by `h` whose every pixel differs, so a pixel written to the wrong place shows.
fn numbered(w: usize, h: usize) -> Arc<Image> {
    let px: Vec<u32> = (0..w * h).map(|i| 0xff00_0000 | (i as u32 + 1)).collect();
    Arc::new(Grid::from_cells(w, h, &px).into_image())
}

/// A 16 by 16 editor whose tilemap layer (at 1) places tile 1 turned by `o` in cell (0, 0) and
/// unturned in cell (1, 0), with tiles of `w` by `h`.
fn turned_map(o: Orientation, (w, h): (usize, usize)) -> (Editor, TilesetId) {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    let set = doc.add_tileset("T", ColorMode::Rgba, (w, h)).expect("room");
    doc.add_tile(set, numbered(w, h)).expect("fits");
    doc.add_tilemap_layer(1, set).expect("room");
    let turned = MapCell::place(id(1), o).bits();
    doc.put_cel(1, 0, map_cel([turned, cell(1), 0, 0]))
        .expect("fits");
    let mut e = Editor::new(doc);
    e.layer = 1;
    e.color = PAINT;
    (e, set)
}

fn tile_pixel(e: &Editor, set: TilesetId, x: usize, y: usize) -> u32 {
    let tiles = e.doc().tileset(set).expect("set");
    tiles
        .tile(id(1))
        .expect("tile")
        .image
        .get(x as i32, y as i32)
}

fn pencil(path: &[(i32, i32)]) -> DrawCmd {
    DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: path.to_vec(),
    }
}

/// Paints each pixel of cell (0, 0) on its own and checks it lands on the tile pixel the
/// orientation shows there, found from the definition, and nowhere else.
fn check_orientation(o: Orientation, (w, h): (usize, usize)) {
    for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))) {
        let (mut e, set) = turned_map(o, (w, h));
        let before = numbered(w, h);
        e.apply(pencil(&[(x as i32, y as i32)])).expect("paints");
        // The tile pixel the cell shows at (x, y): the one the orientation sends there.
        let source = (0..h)
            .flat_map(|sy| (0..w).map(move |sx| (sx, sy)))
            .find(|&(sx, sy)| o.apply(sx, sy, w, h) == (x, y))
            .expect("an orientation is one to one");
        for (sx, sy) in (0..h).flat_map(|sy| (0..w).map(move |sx| (sx, sy))) {
            let want = match (sx, sy) == source {
                true => PAINT,
                false => before.get(sx as i32, sy as i32),
            };
            assert_eq!(tile_pixel(&e, set, sx, sy), want, "{o:?} at ({x}, {y})");
        }
        let layer = &e.doc().layers()[1];
        assert_eq!(e.doc().layer_pixel(layer, 0, x as i32, y as i32), PAINT);
        // The unturned placement beside it shows the change too.
        let (ox, oy) = (w as i32 + source.0 as i32, source.1 as i32);
        assert_eq!(e.doc().layer_pixel(layer, 0, ox, oy), PAINT, "{o:?}");
    }
}

#[test]
fn every_orientation_of_a_square_tile_writes_the_pixel_it_shows() {
    for o in Orientation::ALL {
        check_orientation(o, (TILE, TILE));
    }
}

#[test]
fn oblong_tiles_take_their_flips() {
    for o in Orientation::ALL.into_iter().filter(|o| !o.swaps_axes()) {
        check_orientation(o, (4, 2));
        check_orientation(o, (2, 3));
    }
}

#[test]
fn a_tile_larger_than_a_storage_tile_is_written_where_shown() {
    for o in [
        Orientation::IDENTITY,
        Orientation::rotate90(),
        Orientation::anti_transpose(),
    ] {
        let mut doc = Document::new(48, 48, ColorMode::Rgba).expect("size");
        let set = doc
            .add_tileset("T", ColorMode::Rgba, (20, 20))
            .expect("room");
        doc.add_tile(set, numbered(20, 20)).expect("fits");
        doc.add_tilemap_layer(1, set).expect("room");
        let turned = MapCell::place(id(1), o).bits();
        doc.put_cel(1, 0, map_cel([turned, 0, 0, 0])).expect("fits");
        let mut e = Editor::new(doc);
        (e.layer, e.color) = (1, PAINT);
        e.apply(pencil(&[(17, 3), (19, 19)])).expect("paints");
        let layer = &e.doc().layers()[1];
        for (x, y) in [(17, 3), (19, 19)] {
            assert_eq!(e.doc().layer_pixel(layer, 0, x, y), PAINT, "{o:?}");
        }
        assert_ne!(e.doc().layer_pixel(layer, 0, 16, 3), PAINT);
    }
}

/// A 32 by 32 document of 4 by 4 tiles whose map places tile 1 at cells (0, 0) and (1, 0)
/// unturned and at cell (5, 0) flipped top to bottom.
fn wide_map() -> Document {
    let mut doc = Document::new(32, 32, ColorMode::Rgba).expect("size");
    let set = doc.add_tileset("T", ColorMode::Rgba, (4, 4)).expect("room");
    doc.add_tile(set, numbered(4, 4)).expect("fits");
    doc.add_tilemap_layer(1, set).expect("room");
    let mut cells = [0u32; 8 * 8];
    cells[0] = cell(1);
    cells[1] = cell(1);
    cells[5] = MapCell::place(id(1), Orientation::flip_vertical()).bits();
    doc.put_cel(1, 0, Cel::new(Grid::from_cells(8, 8, &cells)))
        .expect("fits");
    doc
}

#[test]
fn the_last_pixel_in_canvas_row_order_wins() {
    let doc = wide_map();
    // Canvas (21, 1) is cell (5, 0) at (1, 1), flipped to tile pixel (1, 2), which canvas
    // (1, 2) shows unturned. Row 1 comes before row 2, though (1, 2) was painted first and in
    // an earlier storage tile of the overlay.
    let mut overlay = Overlay::new(32, 32);
    overlay.put(1, 2, 0xff00_00aa);
    overlay.put(21, 1, 0xff00_00bb);
    let through = write_through(&doc, 1, 0, &overlay);
    assert_eq!(through.dropped, 0);
    let [(tile, image)] = through.images.as_slice() else {
        panic!("one tile edited");
    };
    assert_eq!(*tile, id(1));
    assert_eq!(image.get(1, 2), 0xff00_00aa);
    // Canvas (22, 3) is cell (5, 0) at (2, 3), flipped to tile pixel (2, 0): row 3 is later.
    let mut overlay = Overlay::new(32, 32);
    overlay.put(22, 3, 0xff00_0001);
    overlay.put(2, 0, 0xff00_0002);
    let image = &write_through(&doc, 1, 0, &overlay).images[0].1;
    assert_eq!(image.get(2, 0), 0xff00_0001, "row 0 first, then row 3");
    // Along one row the pixel further right wins: (1, 0) and (5, 0) are both tile pixel (1, 0).
    let mut overlay = Overlay::new(32, 32);
    overlay.put(5, 0, 0xff00_0003);
    overlay.put(1, 0, 0xff00_0004);
    let image = &write_through(&doc, 1, 0, &overlay).images[0].1;
    assert_eq!(image.get(1, 0), 0xff00_0003);
}

#[test]
fn a_stroke_is_one_step_that_undoes_and_redoes_exactly() {
    for tool in [Tool::Pencil, Tool::Line, Tool::Rect, Tool::Eraser] {
        let (mut e, _) = editor_with_map();
        e.color = PAINT;
        let (before, depth) = (e.doc().clone(), e.history().undo_depth());
        let command = DrawCmd::Stroke {
            tool,
            path: vec![(1, 1), (6, 6)],
        };
        assert!(e.apply(command).expect("paints").changed, "{tool:?}");
        assert_eq!(e.history().undo_depth(), depth + 1, "{tool:?}");
        assert!(!e.is_stroking());
        let after = e.doc().clone();
        assert_ne!(after, before);
        assert_eq!(after.check_tilemaps(), Ok(()));
        // The map is as it was: only the tiles changed.
        assert!(after.layers()[1]
            .cel(0)
            .same_image(before.layers()[1].cel(0)));
        assert!(e.undo());
        assert_eq!(e.doc(), &before);
        assert!(e.redo());
        assert_eq!(e.doc(), &after);
    }
}

#[test]
fn begin_drag_and_commit_make_the_same_one_step() {
    let (mut e, set) = editor_with_map();
    e.color = PAINT;
    e.apply(DrawCmd::Begin {
        tool: Tool::Line,
        x: 0,
        y: 0,
    })
    .expect("begins");
    e.apply(DrawCmd::To { x: 7, y: 0 }).expect("drags");
    // Nothing reaches a tile until the stroke ends.
    assert_ne!(tile_pixel(&e, set, 0, 0), PAINT);
    assert!(e.apply(DrawCmd::Commit).expect("ends").changed);
    assert_eq!(e.history().undo_depth(), 1);
    let tiles = e.doc().tileset(set).expect("set");
    for tile in [1, 2] {
        let image = &tiles.tile(id(tile)).expect("tile").image;
        assert!((0..TILE as i32).all(|x| image.get(x, 0) == PAINT));
    }
}

#[test]
fn pixels_over_empty_cells_are_dropped_and_said() {
    let (mut e, _) = editor_with_map();
    e.frame = 1; // `2 2 / 0 1`: cell (0, 1) is empty.
    e.color = PAINT;
    let before = e.doc().clone();
    let (x, y) = (1, TILE as i32 + 1);
    assert_eq!(e.apply(pencil(&[(x, y)])), Err(LimitError::NoTileHere));
    assert_eq!(e.doc(), &before);
    assert_eq!(e.history().undo_depth(), 0);
    assert_eq!(e.pixels_off_tiles(), 1);
    // Partly over a tile: kept, and the rest counted for the status line.
    let path = [(x, y), (x + TILE as i32, y)];
    let line = DrawCmd::Stroke {
        tool: Tool::Line,
        path: path.to_vec(),
    };
    assert!(e.apply(line).expect("paints").changed);
    assert_eq!(e.pixels_off_tiles(), TILE - 1, "x 1 to 3 of an empty cell");
    assert_eq!(e.history().undo_depth(), 1);
    // A stroke wholly over tiles drops nothing.
    e.apply(pencil(&[(1, 1)])).expect("paints");
    assert_eq!(e.pixels_off_tiles(), 0);
}

#[test]
fn painting_what_is_there_already_changes_nothing() {
    let (mut e, _) = editor_with_map();
    e.color = 0xff00_00ff; // tile 1's colour
    assert!(!e.apply(pencil(&[(1, 1)])).expect("runs").changed);
    assert_eq!(e.history().undo_depth(), 0);
    assert_eq!(e.pixels_off_tiles(), 0);
}

#[test]
fn holds_and_links_are_left_as_they_were() {
    let (mut e, set) = editor_with_map();
    let layer = e.layer_id(1).expect("layer");
    e.apply(FrameCmd::HoldPrevious { layer, frame: 1 })
        .expect("holds");
    e.apply(LayerCmd::Duplicate { layer }).expect("copies");
    e.layer = 1;
    e.frame = 1;
    e.color = PAINT;
    e.apply(pencil(&[(0, 0)])).expect("paints");
    let doc = e.doc();
    assert!(doc.is_held(1, 1));
    assert!(doc.layers()[1].cel(0).same_image(doc.layers()[1].cel(1)));
    assert_eq!(doc.check_links(), Ok(()));
    assert_eq!(tile_pixel(&e, set, 0, 0), PAINT);
    // Every layer and frame that places the tile shows it.
    for (layer, frame) in [(1, 0), (2, 0), (2, 1)] {
        let l = &doc.layers()[layer];
        assert_eq!(doc.layer_pixel(l, frame, 0, 0), PAINT);
        assert_eq!(
            doc.layer_pixel(l, frame, 4, 4),
            PAINT,
            "tile 1 at cell (1, 1)"
        );
    }
}

#[test]
fn a_locked_layer_refuses_and_the_fill_stays_refused() {
    let (mut e, _) = editor_with_map();
    let layer = e.layer_id(1).expect("layer");
    let before = e.doc().clone();
    let fill = DrawCmd::Begin {
        tool: Tool::Fill,
        x: 1,
        y: 1,
    };
    assert_eq!(e.apply(fill), Err(LimitError::Tilemap));
    e.apply(LayerCmd::SetLocked {
        layer,
        locked: true,
    })
    .expect("locks");
    assert_eq!(e.apply(pencil(&[(1, 1)])), Err(LimitError::Locked));
    assert_eq!(e.doc().layers()[1].cel(0), before.layers()[1].cel(0));
    assert_eq!(e.doc().tilesets, before.tilesets);
}

#[test]
fn a_palette_tileset_takes_colour_ids_and_undo_restores_the_table() {
    let mut doc = Document::new(16, 16, ColorMode::Palette).expect("size");
    let set = doc
        .add_tileset("P", ColorMode::Palette, (TILE, TILE))
        .expect("room");
    let blank = Arc::new(Grid::from_cells(TILE, TILE, &[0; TILE * TILE]).into_image());
    doc.add_tile(set, blank).expect("fits");
    doc.add_tilemap_layer(1, set).expect("room");
    doc.put_cel(1, 0, map_cel([cell(1), 0, 0, 0]))
        .expect("fits");
    let mut e = Editor::new(doc);
    e.layer = 1;
    let colors = e.doc().colors.len();
    e.color = rgba(10, 200, 30, 255);
    e.apply(pencil(&[(2, 1)])).expect("paints");
    let value = tile_pixel(&e, set, 2, 1);
    assert_eq!(e.doc().colors.color(value), e.color);
    assert_eq!(e.doc().colors.len(), colors + 1);
    let layer = &e.doc().layers()[1];
    assert_eq!(e.doc().layer_pixel(layer, 0, 2, 1), value);
    assert!(e.undo());
    assert_eq!(e.doc().colors.len(), colors);
    // A colour new to the table is not kept when every pixel is dropped.
    e.color = rgba(1, 2, 3, 255);
    assert_eq!(e.apply(pencil(&[(9, 9)])), Err(LimitError::NoTileHere));
    assert_eq!(e.doc().colors.len(), colors);
}

#[test]
fn the_redraw_names_the_edited_tile_and_the_stroke_area() {
    let (mut e, set) = editor_with_map();
    e.color = PAINT;
    e.take_damage();
    e.apply(pencil(&[(1, 1)])).expect("paints");
    let Damage::Placed(edit) = e.take_damage() else {
        panic!("a tile edit");
    };
    assert_eq!((edit.tileset(), edit.tiles()), (set, &[id(1)][..]));
    assert_eq!(edit.canvas_tiles(), &[0]);
    // Undo redraws by the edit alone.
    assert!(e.undo());
    assert!(matches!(e.take_damage(), Damage::Placed(_)));
    // A stroke that changes no tile still clears where it was drawn.
    e.frame = 2;
    assert!(e.apply(pencil(&[(1, 1)])).is_err());
    assert_eq!(e.take_damage(), Damage::Tiles(vec![0]));
}

#[test]
fn the_picker_reads_the_pixels_a_map_draws() {
    let (mut e, _) = editor_with_map();
    e.color = 0;
    e.apply(DrawCmd::Begin {
        tool: Tool::Picker,
        x: 5,
        y: 1,
    })
    .expect("picks");
    assert_eq!(e.color, 0xff00_ff00, "tile 2 at cell (1, 0)");
    assert_eq!(e.color_at(1, 1), 0xff00_00ff);
    e.frame = 2;
    assert_eq!(e.color_at(1, 1), 0);
}

/// A 256 by 256 canvas of 1 by 1 tiles, every cell placing a tile of its own.
fn every_cell_its_own_tile() -> Document {
    let mut doc = Document::new(256, 256, ColorMode::Rgba).expect("size");
    let set = doc.add_tileset("T", ColorMode::Rgba, (1, 1)).expect("room");
    let blank = Arc::new(Image::blank());
    for _ in 0..crate::tileset::MAX_TILES {
        doc.add_tile(set, blank.clone()).expect("fits");
    }
    doc.add_tilemap_layer(1, set).expect("room");
    let cells: Vec<u32> = (0..256 * 256u32).map(|i| cell(i.min(65_534) + 1)).collect();
    doc.put_cel(1, 0, Cel::new(Grid::from_cells(256, 256, &cells)))
        .expect("fits");
    doc
}

#[test]
fn a_stroke_over_tens_of_thousands_of_tiles_takes_time_linear_in_them() {
    let doc = every_cell_its_own_tile();
    let mut overlay = Overlay::new(256, 256);
    for y in 0..256 {
        for x in 0..256 {
            overlay.put(x, y, PAINT);
        }
    }
    let started = std::time::Instant::now();
    let through = write_through(&doc, 1, 0, &overlay);
    let took = started.elapsed();
    assert_eq!(through.images.len(), crate::tileset::MAX_TILES);
    assert!(through
        .images
        .iter()
        .all(|(_, image)| image.get(0, 0) == PAINT));
    // Searched linearly, 65,535 tiles took about ten seconds unoptimised; by hash, under one.
    assert!(took < std::time::Duration::from_secs(4), "{took:?}");
}

#[test]
fn a_stroke_partly_over_a_tile_that_it_leaves_as_it_was_is_not_refused() {
    let (mut e, _) = editor_with_map();
    e.frame = 1; // `2 2 / 0 1`: cell (0, 1) is empty and cell (1, 1) places tile 1.
    e.color = 0xff00_00ff; // tile 1 is already this colour
    let (y, empty, on_tile) = (TILE as i32 + 1, 1, TILE as i32 + 1);
    let line = DrawCmd::Stroke {
        tool: Tool::Line,
        path: vec![(empty, y), (on_tile, y)],
    };
    let before = e.doc().clone();
    let out = e.apply(line).expect("some pixels fell on a tile");
    assert!(!out.changed);
    assert_eq!(e.doc(), &before);
    assert_eq!(e.pixels_off_tiles(), TILE - 1);
}
