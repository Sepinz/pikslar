//! A tileset as one PNG, for the TMJ export: its tiles in panel order, left to right and then
//! down, ceil(sqrt(n)) tiles a row, with no margin and no spacing. A palette-mode tileset's
//! colour numbers become the colours they stand for, so every atlas is 8-bit RGBA.

use crate::ExportError;
use pikslar_core::tileset::Tileset;
use pikslar_core::{ColorMode, ColorTable, MAX_DIMENSION};
use pikslar_format::png_write::PngStream;
use std::io::Write;

/// Where an atlas puts its tiles, and how large it is in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasLayout {
    pub columns: usize,
    pub rows: usize,
    pub width: usize,
    pub height: usize,
}

impl AtlasLayout {
    /// `count` tiles of `tile` pixels: ceil(sqrt(count)) a row, as many rows as they need.
    #[inline(never)]
    pub fn new(count: usize, (tw, th): (usize, usize)) -> AtlasLayout {
        let root = count.isqrt();
        let columns = if root * root < count { root + 1 } else { root };
        let rows = count.div_ceil(columns.max(1));
        AtlasLayout {
            columns,
            rows,
            width: columns * tw,
            height: rows * th,
        }
    }

    /// The top left pixel of the tile at `index` in panel order.
    pub fn place(&self, index: usize, (tw, th): (usize, usize)) -> (usize, usize) {
        let columns = self.columns.max(1);
        ((index % columns) * tw, (index / columns) * th)
    }
}

/// The layout of `set`'s atlas, or why it cannot be written: no tiles, or larger than an
/// exported image may be.
#[inline(never)]
pub fn atlas_layout(set: &Tileset) -> Result<AtlasLayout, ExportError> {
    let layout = AtlasLayout::new(set.len(), set.tile_size());
    let fits = |side: usize| (1..=MAX_DIMENSION).contains(&side);
    if !fits(layout.width) || !fits(layout.height) {
        return Err(ExportError::AtlasTooLarge {
            tileset: set.name().to_string(),
            width: layout.width,
            height: layout.height,
        });
    }
    Ok(layout)
}

/// The colour a stored value of a tile shows, as the compositor reads it.
fn shown(mode: ColorMode, colors: &ColorTable, value: u32) -> u32 {
    match mode {
        ColorMode::Rgba => value,
        ColorMode::Palette => colors.color(value & 0xffff),
    }
}

/// Fills `band` with one row of tiles of the atlas: rows `row * th` to `row * th + th - 1`.
fn fill_band(
    set: &Tileset,
    colors: &ColorTable,
    layout: &AtlasLayout,
    row: usize,
    band: &mut [u32],
) {
    let (tw, th) = set.tile_size();
    band.fill(0);
    let first = row * layout.columns;
    let tiles = set
        .tiles()
        .iter()
        .enumerate()
        .skip(first)
        .take(layout.columns);
    for (index, entry) in tiles {
        let (x0, _) = layout.place(index, (tw, th));
        for y in 0..th {
            let line = &mut band[y * layout.width + x0..][..tw];
            for (x, cell) in line.iter_mut().enumerate() {
                *cell = shown(set.mode(), colors, entry.image.get(x as i32, y as i32));
            }
        }
    }
}

/// Writes `set`'s atlas as an 8-bit RGBA PNG into `out`, one row of tiles at a time.
#[inline(never)]
pub fn write_atlas_png(
    set: &Tileset,
    colors: &ColorTable,
    out: &mut dyn Write,
) -> Result<(), ExportError> {
    let layout = atlas_layout(set)?;
    let th = set.tile_size().1;
    let mut png = PngStream::new(out, layout.width, layout.height)?;
    let mut band = vec![0u32; layout.width * th];
    for row in 0..layout.rows {
        fill_band(set, colors, &layout, row, &mut band);
        png.rows(&band)?;
    }
    png.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_are_the_ceiling_of_the_square_root() {
        for (count, columns, rows) in [
            (1, 1, 1),
            (2, 2, 1),
            (3, 2, 2),
            (4, 2, 2),
            (5, 3, 2),
            (9, 3, 3),
            (10, 4, 3),
            (16, 4, 4),
            (17, 5, 4),
            (65_535, 256, 256),
        ] {
            let layout = AtlasLayout::new(count, (3, 2));
            assert_eq!((layout.columns, layout.rows), (columns, rows), "{count}");
            assert_eq!((layout.width, layout.height), (columns * 3, rows * 2));
            assert!(layout.columns * layout.rows >= count);
            assert!((layout.columns - 1) * (layout.columns - 1) < count);
        }
    }

    #[test]
    fn tiles_go_left_to_right_then_down() {
        let layout = AtlasLayout::new(5, (4, 3));
        let places: Vec<_> = (0..5).map(|i| layout.place(i, (4, 3))).collect();
        assert_eq!(places, [(0, 0), (4, 0), (8, 0), (0, 3), (4, 3)]);
    }
}
