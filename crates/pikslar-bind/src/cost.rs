//! What each host function burns, before it does its work.
//!
//! The interpreter's own work costs 5 to 18 ns a fuel unit. Host work is priced to cost no
//! more than that per unit on the stress documents (measured by `pikslar-bench script`), so
//! the default 50,000,000 units end any run, whatever mix of host calls it makes, within
//! about a second.

/// A read that copies nothing that grows with the document.
pub const READ: u64 = 4;
/// Every operation that runs a command, before the terms below.
pub const EDIT: u64 = 256;
/// Writing one pixel into a block already waiting to be written.
pub const SET: u64 = 1;
/// Pixels per fuel unit, for work that touches every pixel of an area.
pub const PIXELS_PER_UNIT: u64 = 16;
/// Each pixel a block fill or a flood fill writes: a strip of pasted or filled pixels costs
/// about 12 ns a pixel on the large document.
pub const FILL_PIXEL: u64 = 1;
/// Each pixel of a line: a line crosses a new tile every few pixels, and copies it.
pub const LINE_PIXEL: u64 = 4;

/// Pixels in the drawing a text draw makes: the whole rectangle of its lines, at most 256
/// characters and the largest scale, as the API makes it. 0 for a drawing past
/// [`pikslar_core::glyphs::MAX_TEXT_AREA`], which the API refuses without making it.
pub fn text_area(text: &str, scale: i64) -> usize {
    use pikslar_core::glyphs::{self, Font, MAX_SCALE, MAX_TEXT_AREA};
    let scale = i32::try_from(scale.clamp(1, i64::from(MAX_SCALE))).unwrap_or(1);
    let area = glyphs::text_area(Font::Pikslar5x7, text, scale, 1);
    if area > MAX_TEXT_AREA {
        0
    } else {
        area
    }
}

/// The most bytes the paste behind a text draw holds at once for each pixel of the drawing. The
/// clip is 4 (a color each); the cells made from it, 4; a palette layer's second conversion, when
/// pasting moved the color table, 4 more while the first is still held; the mask of its shape at
/// most 1.75 (a bit a pixel, and a whole word in each row, rows being at least 5 wide).
pub const TEXT_PASTE_BYTES_PER_PIXEL: usize = 14;

/// What a text draw pays beyond [`edit`], before any glyph is drawn: every pixel of the drawing,
/// each priced as a block fill's, whether a letter lights it or not, since all are made.
pub fn text(area: usize) -> u64 {
    (area as u64).saturating_mul(FILL_PIXEL)
}

/// A command, priced by the document it works on: each command looks at the active cel's
/// tiles and at its layers and frames.
pub fn edit(size: (usize, usize, usize, usize)) -> u64 {
    let (w, h, layers, frames) = size;
    let area = (w as u64).saturating_mul(h as u64);
    EDIT.saturating_add(area / 1024)
        .saturating_add(layers as u64)
        .saturating_add(frames as u64)
}

/// Cel entries moved per fuel unit when a frame is added or removed: every layer's list of
/// cels shifts. On 256 layers of 512 frames, an add and a remove cost about 260 µs, so a loop
/// of them at the default fuel stopped only after 6 seconds when priced like any edit.
pub const CELS_PER_UNIT: u64 = 8;

/// What a frame added, copied or removed pays beyond [`edit`]: every cel entry it moves.
pub fn moved_cels(size: (usize, usize, usize, usize)) -> u64 {
    let (_, _, layers, frames) = size;
    (layers as u64).saturating_mul(frames as u64) / CELS_PER_UNIT
}

/// Each cel a duplicated layer or group copies: a 16-byte entry sharing its image, so a
/// copy adds well under `growth::MOST_BYTES_PER_UNIT` bytes per unit it pays.
pub const COPIED_CEL: u64 = 1;

/// Frames looked through per fuel unit: a read names its frame by handle, and finding it
/// walks the list (to the end, to refuse a handle two frames hold). On the widest document,
/// 4096 frames, a `get_pixel` costs about 1 µs.
pub const FRAMES_PER_UNIT: u64 = 64;
/// Layers looked through per fuel unit, for finding a layer by id: each compares an id
/// kept further from the list than a frame's handle.
pub const LAYERS_PER_UNIT: u64 = 16;
/// Colors per fuel unit of a palette edit: it looks through the swatches and may copy the
/// whole table, about 300 µs for 65,536 colors.
pub const COLORS_PER_UNIT: u64 = 4;

/// A read that finds a layer by id, priced by how many layers there are.
pub fn layer_read(size: (usize, usize, usize, usize)) -> u64 {
    READ.saturating_add(size.2 as u64 / LAYERS_PER_UNIT)
}

/// A read that finds a layer and a frame by id, priced by how many of each there are.
pub fn cel_read(size: (usize, usize, usize, usize)) -> u64 {
    layer_read(size).saturating_add(size.3 as u64 / FRAMES_PER_UNIT)
}

/// What a palette edit adds for the colors of the table.
pub fn palette(colors: usize) -> u64 {
    colors as u64 / COLORS_PER_UNIT
}

/// Work over `pixels` pixels.
pub fn pixels(pixels: u64) -> u64 {
    pixels / PIXELS_PER_UNIT
}

/// Starting a new block of waiting pixels: the paste that will write it.
pub fn new_block(size: (usize, usize, usize, usize)) -> u64 {
    edit(size).saturating_add(pixels(crate::pending::BLOCK_AREA as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_cost_more_on_larger_documents() {
        assert_eq!(edit((16, 16, 1, 1)), EDIT + 2);
        assert!(edit((1024, 1024, 12, 400)) > edit((128, 128, 12, 400)));
        assert_eq!(
            edit((usize::MAX, usize::MAX, usize::MAX, usize::MAX)),
            u64::MAX
        );
        assert_eq!(pixels(64), 4);
    }
}
