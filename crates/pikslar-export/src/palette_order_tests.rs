//! Indexed PNGs and GIFs in palette order: when every colour the image uses is a swatch, the
//! palette follows the swatches; otherwise it is the order of value it always was.

use super::*;
use crate::gif_read::{read, shown};
use crate::gif_tests::written;
use crate::indexed_tests::declared;
use crate::{frame_output, gif_output, gif_palette, sheet_outputs, SheetOptions};
use pikslar_core::{rgba, ColorMode, Document, Grid};
use pikslar_format::png;
use std::sync::Arc;

const RED: u32 = rgba(255, 0, 0, 255);
const GREEN: u32 = rgba(0, 255, 0, 255);
const BLUE: u32 = rgba(0, 0, 255, 255);
const GOLD: u32 = rgba(255, 215, 0, 255);
const GLASS: u32 = rgba(255, 255, 255, 128);

/// An RGBA document showing `cells` on a row, with `swatches` in the palette in that order.
fn doc(cells: &[u32], swatches: &[u32]) -> Document {
    let mut d = Document::new(cells.len(), 1, ColorMode::Rgba).expect("valid");
    d.set_cel(0, 0, Grid::from_cells(cells.len(), 1, cells))
        .expect("cel");
    let table = Arc::make_mut(&mut d.colors);
    for &colour in swatches {
        table.add_swatch(colour).expect("room");
    }
    d
}

fn colours_of(d: &Document) -> Vec<u32> {
    palette_of(&Picture::visible(d), [0], false)
        .expect("fits")
        .colours()
        .to_vec()
}

#[test]
fn colours_come_in_the_order_of_the_swatches() {
    let d = doc(&[BLUE, RED, GREEN, RED], &[GREEN, BLUE, RED]);
    assert_eq!(colours_of(&d), [GREEN, BLUE, RED]);
}

#[test]
fn a_swatch_the_image_does_not_use_is_left_out() {
    let d = doc(&[BLUE, RED], &[GOLD, RED, GREEN, BLUE]);
    assert_eq!(colours_of(&d), [RED, BLUE]);
}

#[test]
fn a_used_colour_that_is_not_a_swatch_keeps_the_order_of_value() {
    let d = doc(&[BLUE, RED, GREEN], &[GREEN, BLUE]);
    assert_eq!(colours_of(&d), [RED, GREEN, BLUE]);
}

#[test]
fn no_swatches_keeps_the_order_of_value() {
    let d = doc(&[GLASS, BLUE, 0, RED], &[]);
    assert_eq!(colours_of(&d), [0, GLASS, RED, BLUE]);
}

#[test]
fn the_clear_colour_comes_first_and_a_translucent_swatch_keeps_its_place() {
    let d = doc(&[BLUE, 0, GLASS, RED], &[RED, GLASS, BLUE]);
    assert_eq!(colours_of(&d), [0, RED, GLASS, BLUE]);
}

#[test]
fn a_clear_pixel_with_a_colour_behind_it_is_not_a_swatch() {
    let hidden = 0x00ff_0000;
    let d = doc(&[hidden, RED], &[RED]);
    assert_eq!(colours_of(&d), [hidden, RED]);
}

#[test]
fn two_swatches_with_one_colour_count_at_the_first() {
    let mut d = doc(&[RED, GREEN], &[RED, GREEN, BLUE]);
    // Blue's swatch is recoloured to red: two swatches show red.
    Arc::make_mut(&mut d.colors).set_color(3, RED);
    assert_eq!(colours_of(&d), [RED, GREEN]);
}

#[test]
fn the_png_is_indexed_in_palette_order_and_shows_the_same_pixels() {
    let cells = [BLUE, 0, GLASS, RED, GREEN, BLUE];
    let d = doc(&cells, &[GREEN, RED, GLASS, BLUE]);
    let coded = written(
        frame_output(&Picture::visible(&d).with_indexed(true), 0, "f.png".into()).expect("fits"),
    );
    let plain = written(frame_output(&Picture::visible(&d), 0, "f.png".into()).expect("fits"));
    assert_eq!(
        png::decode(&coded).expect("indexed").px,
        png::decode(&plain).expect("rgba").px
    );
    assert_eq!(declared(&coded).0, [0, GREEN, RED, GLASS, BLUE]);
}

#[test]
fn a_sheet_with_space_between_frames_puts_the_clear_colour_first() {
    let mut d = doc(&[BLUE, RED], &[RED, BLUE]);
    d.add_frame(1, false).expect("room");
    d.set_cel(0, 1, Grid::from_cells(2, 1, &[RED, RED]))
        .expect("cel");
    let options = SheetOptions {
        layout: crate::Layout {
            columns: None,
            padding: 1,
        },
        ..SheetOptions::default()
    };
    let picture = Picture::visible(&d).with_indexed(true);
    let mut files = sheet_outputs(&picture, 0..=1, "s.png".into(), &options).expect("fits");
    let sheet = written(files.remove(0));
    assert_eq!(declared(&sheet).0, [0, RED, BLUE]);
}

#[test]
fn a_gif_is_in_palette_order_with_its_clear_entry_first() {
    let cells = [BLUE, 0, RED, GREEN, BLUE];
    let d = doc(&cells, &[GREEN, BLUE, RED]);
    let picture = Picture::visible(&d);
    let palette = gif_palette(&picture, [0]).expect("fits");
    assert_eq!(palette.opaque, [GREEN, BLUE, RED]);
    assert!(palette.clear);
    let r = read(&written(
        gif_output(&picture, vec![0], 0, "a.gif".into()).expect("fits"),
    ));
    assert_eq!(r.table[1..4], [[0, 255, 0], [0, 0, 255], [255, 0, 0]]);
    let plain = written(frame_output(&picture, 0, "f.png".into()).expect("fits"));
    let expected: Vec<u32> = png::decode(&plain)
        .expect("rgba")
        .px
        .into_iter()
        .map(|c| if c >> 24 == 0 { 0 } else { c })
        .collect();
    assert_eq!(shown(&r)[0], expected);
}

#[test]
fn a_gif_with_a_colour_that_is_no_swatch_keeps_the_order_of_value() {
    let d = doc(&[BLUE, RED, GREEN], &[GREEN, BLUE]);
    let palette = gif_palette(&Picture::visible(&d), [0]).expect("fits");
    assert_eq!(palette.opaque, [RED, GREEN, BLUE]);
}
