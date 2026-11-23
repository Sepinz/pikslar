use super::*;
use crate::brush_store::{BrushStore, FILE_NAME};
use pikslar_format::brush_lib::{write_library, Handle, MAX_BRUSHES, MAX_SIDE};
use std::fs;
use std::path::PathBuf;

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

fn entry(w: u16, h: u16, pixels: &[[u8; 4]]) -> LibraryBrush {
    LibraryBrush {
        name: "t".to_string(),
        width: w,
        height: h,
        paint: Paint::OwnColors,
        orientation: 0,
        handle: Handle::Centre,
        rgba: pixels.iter().flatten().copied().collect(),
    }
}

fn thumb(e: &LibraryBrush) -> Vec<u32> {
    let mut out = vec![7u32; CELLS];
    draw_thumb(e, &mut out);
    out
}

fn at(cells: &[u32], x: usize, y: usize) -> u32 {
    cells[y * SIDE + x]
}

/// The cells that are drawn at all, as a bounding box: left, top, width, height.
fn drawn_box(cells: &[u32]) -> (usize, usize, usize, usize) {
    let drawn: Vec<(usize, usize)> = (0..CELLS)
        .filter(|&i| cells[i] != 0)
        .map(|i| (i % SIDE, i / SIDE))
        .collect();
    let left = drawn.iter().map(|p| p.0).min().expect("something drawn");
    let top = drawn.iter().map(|p| p.1).min().expect("something drawn");
    let right = drawn.iter().map(|p| p.0).max().expect("something drawn");
    let bottom = drawn.iter().map(|p| p.1).max().expect("something drawn");
    (left, top, right - left + 1, bottom - top + 1)
}

#[test]
fn small_brushes_grow_by_whole_cells_and_large_ones_shrink_to_fit() {
    assert_eq!(shown_size(1, 1), (16, 16));
    assert_eq!(shown_size(3, 2), (15, 10));
    assert_eq!(shown_size(2, 3), (10, 15));
    assert_eq!(shown_size(16, 4), (16, 4));
    assert_eq!(shown_size(17, 5), (16, 4));
    assert_eq!(shown_size(256, 256), (16, 16));
    assert_eq!(shown_size(256, 1), (16, 1), "never thinner than a cell");
    assert_eq!(shown_size(1, 255), (1, 16));
    assert_eq!(shown_size(0, 0), (0, 0));
}

#[test]
fn a_one_pixel_brush_fills_the_picture() {
    let cells = thumb(&entry(1, 1, &[RED]));
    assert!(cells.iter().all(|&c| c == 0xffff_0000), "red everywhere");
}

#[test]
fn a_three_by_two_brush_is_five_times_larger_centred_with_its_hole_on_the_checker() {
    // red, clear, blue / blue, red, red
    let e = entry(3, 2, &[RED, CLEAR, BLUE, BLUE, RED, RED]);
    let cells = thumb(&e);
    // 15 by 10 cells, from (0, 3): one cell left over on the right, three above and below.
    assert_eq!(drawn_box(&cells), (0, 3, 15, 10));
    assert_eq!(at(&cells, 0, 3), 0xffff_0000);
    assert_eq!(at(&cells, 4, 7), 0xffff_0000);
    assert_eq!(at(&cells, 10, 3), 0xff00_00ff);
    assert_eq!(at(&cells, 0, 8), 0xff00_00ff);
    assert_eq!(at(&cells, 14, 12), 0xffff_0000);
    // The clear pixel shows the checker, in squares of four cells.
    assert_eq!(
        at(&cells, 5, 3),
        pikslar_render::Backdrop::NEUTRAL.checker_b
    );
    assert_eq!(
        at(&cells, 8, 3),
        pikslar_render::Backdrop::NEUTRAL.checker_a
    );
    assert_eq!(
        at(&cells, 5, 4),
        pikslar_render::Backdrop::NEUTRAL.checker_a
    );
    // Outside the brush nothing is drawn: the menu row shows through.
    assert_eq!(at(&cells, 15, 3), 0);
    assert_eq!(at(&cells, 0, 2), 0);
    assert_eq!(at(&cells, 0, 13), 0);
}

#[test]
fn an_odd_size_larger_than_the_picture_is_sampled_nearest() {
    // 17 wide, 5 tall: column x is red when x is even, row 4 is all blue.
    let pixels: Vec<[u8; 4]> = (0..5)
        .flat_map(|y| {
            (0..17).map(move |x| {
                if y == 4 {
                    BLUE
                } else if x % 2 == 0 {
                    RED
                } else {
                    CLEAR
                }
            })
        })
        .collect();
    let cells = thumb(&entry(17, 5, &pixels));
    // 16 by 4 cells from row 6; cell column tx takes source column tx * 17 / 16.
    assert_eq!(drawn_box(&cells), (0, 6, 16, 4));
    for tx in 0..16 {
        let source = tx * 17 / 16;
        let want_red = source % 2 == 0;
        assert_eq!(at(&cells, tx, 6) == 0xffff_0000, want_red, "column {tx}");
    }
    // Cell row ty takes source row ty * 5 / 4: 0, 1, 2, 3. The blue row 4 is never sampled.
    assert!((6..10).all(|y| at(&cells, 1, y) != 0xff00_00ff));
}

#[test]
fn the_largest_brush_takes_every_sixteenth_pixel() {
    let side = MAX_SIDE as usize;
    let pixels: Vec<[u8; 4]> = (0..side * side)
        .map(|i| {
            let (x, y) = (i % side, i / side);
            [x as u8, y as u8, 9, 255]
        })
        .collect();
    let cells = thumb(&entry(MAX_SIDE, MAX_SIDE, &pixels));
    for (tx, ty) in [(0, 0), (1, 0), (15, 15), (3, 9)] {
        let (r, g) = (tx as u32 * 16, ty as u32 * 16);
        assert_eq!(at(&cells, tx, ty), 0xff00_0000 | r << 16 | g << 8 | 9);
    }
}

#[test]
fn a_turned_brush_is_shown_as_it_paints() {
    // Two wide, one tall: red then blue. A quarter turn clockwise makes it one wide, two tall,
    // red above blue.
    let e = LibraryBrush {
        orientation: pikslar_core::orient::Orientation::rotate90().index(),
        ..entry(2, 1, &[RED, BLUE])
    };
    let cells = thumb(&e);
    assert_eq!(drawn_box(&cells), (4, 0, 8, 16));
    assert_eq!(at(&cells, 4, 0), 0xffff_0000);
    assert_eq!(at(&cells, 11, 15), 0xff00_00ff);
}

#[test]
fn a_shape_brush_is_shown_in_the_interface_ink_and_half_clear_colour_is_mixed() {
    let shape = LibraryBrush {
        paint: Paint::Shape,
        ..entry(2, 1, &[RED, CLEAR])
    };
    let cells = thumb(&shape);
    assert_eq!(at(&cells, 0, 4), pikslar_ui::theme::text());
    assert_eq!(
        at(&cells, 8, 4),
        pikslar_render::Backdrop::NEUTRAL.checker_b
    );
    let half = thumb(&entry(1, 1, &[[255, 255, 255, 128]]));
    // Half white over the checker's first square: each channel half way, rounded.
    let mix = |c: u32| (255 * 128 + c * 127 + 127) / 255;
    let (r, g, b) = (0x3d, 0x3d, 0x3d);
    assert_eq!(
        at(&half, 0, 0),
        0xff00_0000 | mix(r) << 16 | mix(g) << 8 | mix(b)
    );
}

#[test]
fn a_damaged_entry_draws_nothing() {
    let short = LibraryBrush {
        rgba: vec![1, 2, 3],
        ..entry(2, 2, &[RED; 4])
    };
    assert!(thumb(&short).iter().all(|&c| c == 0));
    let turned = LibraryBrush {
        orientation: 8,
        ..entry(1, 1, &[RED])
    };
    assert!(thumb(&turned).iter().all(|&c| c == 0));
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-thumbs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

fn store_with(dir: &std::path::Path, brushes: &[LibraryBrush]) -> BrushStore {
    fs::write(dir.join(FILE_NAME), write_library(brushes).expect("bytes")).expect("file");
    let mut store = BrushStore::at(Some(dir.join(FILE_NAME)));
    store.ensure_read();
    store
}

#[test]
fn the_pictures_are_built_once_and_again_only_when_the_library_changes() {
    let dir = temp_dir("cache");
    let mut store = store_with(&dir, &[entry(1, 1, &[RED])]);
    let mut thumbs = Thumbs::default();
    assert!(thumbs.update(&store), "built the first time");
    assert!(
        !thumbs.update(&store),
        "and not again while nothing changed"
    );
    assert_eq!(thumbs.of(0).map(|c| c[0]), Some(0xffff_0000));
    assert!(thumbs.of(1).is_none());
    let blue = LibraryBrush {
        name: "u".to_string(),
        ..entry(1, 1, &[BLUE])
    };
    store.add(blue).expect("added");
    assert!(thumbs.update(&store), "a brush added");
    assert_eq!(thumbs.of(1).map(|c| c[0]), Some(0xff00_00ff));
    store.rename(1, "v").expect("renamed");
    assert!(thumbs.update(&store), "a rename changes the version too");
    store.remove(0).expect("removed");
    assert!(thumbs.update(&store));
    assert_eq!(
        thumbs.of(0).map(|c| c[0]),
        Some(0xff00_00ff),
        "the blue one moved up"
    );
    assert!(thumbs.of(1).is_none());
    // A different store, as after the settings folder changes, is never mistaken for this one.
    let other = store_with(&dir, &[entry(1, 1, &[RED])]);
    assert!(thumbs.update(&other));
    assert!(
        thumbs.update(&BrushStore::at(None)),
        "and an unread one empties the pictures"
    );
    assert!(thumbs.of(0).is_none());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_full_library_of_pictures_takes_sixty_four_kibibytes_and_no_more_after_changes() {
    let dir = temp_dir("full");
    let many: Vec<LibraryBrush> = (0..MAX_BRUSHES)
        .map(|i| LibraryBrush {
            name: format!("b{i}"),
            ..entry(1, 1, &[RED])
        })
        .collect();
    let mut store = store_with(&dir, &many);
    let mut thumbs = Thumbs::default();
    thumbs.update(&store);
    assert_eq!(thumbs.bytes(), MAX_BRUSHES * CELLS * 4);
    assert_eq!(thumbs.bytes(), 64 << 10);
    store.remove(3).expect("removed");
    thumbs.update(&store);
    store.rename(0, "first").expect("renamed");
    thumbs.update(&store);
    assert_eq!(
        thumbs.bytes(),
        64 << 10,
        "the buffer is reused, never grown"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_paged_menu_puts_each_picture_in_the_row_that_shows_its_brush() {
    let rect = Rect::new(10, 10, 200, 72);
    // Three rows fit. With the list scrolled to start at brush 5, brush 5 is the top row.
    assert_eq!(shown_row(rect, 4, 5, 1), None, "scrolled out above");
    assert_eq!(shown_row(rect, 5, 5, 1), Some(menu_row(rect, 0, 1)));
    assert_eq!(shown_row(rect, 7, 5, 1), Some(menu_row(rect, 2, 1)));
    assert_eq!(shown_row(rect, 8, 5, 1), None, "below the menu");
    assert_eq!(shown_row(rect, 1, 0, 1), Some(menu_row(rect, 1, 1)));
}
