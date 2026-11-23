//! Blocks of pixels of one cel: written by pasting and placing them, erased by deleting a
//! selected rectangle, as the interface does, so the same checks refuse them.

use super::Script;
use crate::{input, ApiError, Op, Reply};
use pikslar_core::glyphs::{
    rasterize, text_area, text_height, text_width, Font, MAX_CHARS, MAX_SCALE, MAX_TEXT_AREA,
};
use pikslar_core::MAX_DIMENSION;
use pikslar_core::{Clip, DrawCmd, FloatingCmd, Mask, SelectOp, SelectionCmd};
use std::sync::Arc;

/// The most pixels one paste holds: 256 KiB of colors.
const STRIP: usize = 1 << 16;
/// The most pixels one `Pixels` read gives back: 64 MiB of colors, as many as the largest
/// memory a script may be set to could hold as a list.
pub const MOST_READ_PIXELS: usize = 1 << 24;

impl Script<'_> {
    pub(super) fn block(&mut self, op: Op) -> Result<Reply, ApiError> {
        match op {
            Op::PutPixels {
                layer,
                frame,
                x,
                y,
                width,
                colors,
            } => {
                self.choose_cel(layer, frame)?;
                let rows = rows_of(width, colors.len());
                let (x, y, w, h) = input::rect(self.doc(), (x, y), width, rows)?;
                if w * h != colors.len() {
                    return Err(ApiError::OutOfRange {
                        what: "the number of colors, a whole number of rows",
                        value: i64::try_from(colors.len()).unwrap_or(i64::MAX),
                        min: width,
                        max: width.saturating_mul(rows.max(1)),
                    });
                }
                for &c in &colors {
                    input::color(c)?;
                }
                self.paste_strips((x, y, w, h), |from, to| {
                    let row = |c: &i64| input::color(*c).unwrap_or(0);
                    colors[from * w..to * w].iter().map(row).collect()
                })?;
            }
            Op::FillRect {
                layer,
                frame,
                x,
                y,
                width,
                height,
                color,
            } => {
                self.choose_cel(layer, frame)?;
                let (x, y, w, h) = input::rect(self.doc(), (x, y), width, height)?;
                match input::color(color)? {
                    0 => self.erase(x, y, w, h)?,
                    color => {
                        self.paste_strips((x, y, w, h), |from, to| vec![color; (to - from) * w])?
                    }
                }
            }
            Op::DrawText {
                layer,
                frame,
                x,
                y,
                text,
                color,
                scale,
            } => {
                self.choose_cel(layer, frame)?;
                let (x, y) = input::pixel(self.doc(), x, y)?;
                let color = text_color(color)?;
                let scale = input::ranged("the text scale", scale, 1, i64::from(MAX_SCALE))?;
                let chars = i64::try_from(text.chars().count()).unwrap_or(i64::MAX);
                input::ranged("the length of the text", chars, 0, MAX_CHARS as i64)?;
                let scale = i32::try_from(scale).unwrap_or(1);
                // A drawing wider or higher than a canvas can be is refused before it is made.
                let font = Font::Pikslar5x7;
                let wide = i64::from(text_width(font, &text, scale, 1));
                let high = i64::from(text_height(font, &text, scale));
                let most = MAX_DIMENSION as i64;
                input::ranged("the width of the text", wide, 0, most)?;
                input::ranged("the height of the text", high, 0, most)?;
                let area = i64::try_from(text_area(font, &text, scale, 1)).unwrap_or(i64::MAX);
                input::ranged("the area of the text", area, 0, MAX_TEXT_AREA as i64)?;
                let (clip, _) = rasterize(Font::Pikslar5x7, scale, 1, &text, color);
                self.paste(x, y, clip)?;
            }
            Op::Pixels {
                layer,
                frame,
                x,
                y,
                width,
                height,
            } => {
                self.choose_cel(layer, frame)?;
                let (x, y, w, h) = input::rect(self.doc(), (x, y), width, height)?;
                read_fits(w, h)?;
                return Ok(Reply::Pixels(self.read(x, y, w, h)));
            }
            other => return self.call(other),
        }
        Ok(Reply::Done)
    }

    /// Pastes a block a strip of rows at a time, so no paste holds more than [`STRIP`] pixels
    /// however large the block: `rows(from, to)` gives the colors of rows `from..to`.
    fn paste_strips(
        &mut self,
        (x, y, w, h): (i32, i32, usize, usize),
        rows: impl Fn(usize, usize) -> Vec<u32>,
    ) -> Result<(), ApiError> {
        let step = (STRIP / w.max(1)).max(1);
        for from in (0..h).step_by(step) {
            let to = (from + step).min(h);
            let clip = Clip::from_image(w, to - from, rows(from, to));
            self.paste(x, y + to_i32(from), clip)?;
        }
        Ok(())
    }

    /// Pastes the colors that are not 0 and places them at once, leaving no selection.
    fn paste(&mut self, x: i32, y: i32, clip: Clip) -> Result<(), ApiError> {
        if clip.colors.iter().all(|&c| c == 0) {
            return Ok(());
        }
        let clip = Arc::new(clip);
        self.apply(FloatingCmd::Paste { clip, x, y })?;
        self.apply(FloatingCmd::Anchor)?;
        self.apply(SelectionCmd::None)?;
        Ok(())
    }

    fn erase(&mut self, x: i32, y: i32, w: usize, h: usize) -> Result<(), ApiError> {
        let (w, h) = (to_i32(w), to_i32(h));
        let shape = Box::new(Mask::rect(x, y, w, h));
        let select = SelectionCmd::Select {
            shape,
            op: SelectOp::Replace,
        };
        self.apply(select)?;
        let deleted = self.apply(DrawCmd::DeleteSelected);
        self.apply(SelectionCmd::None)?;
        deleted.map(|_| ())
    }

    fn read(&self, x: i32, y: i32, w: usize, h: usize) -> Vec<u32> {
        let (w, h) = (to_i32(w), to_i32(h));
        let rows = (y..y + h).flat_map(|py| (x..x + w).map(move |px| (px, py)));
        rows.map(|(px, py)| self.editor.color_at(px, py)).collect()
    }
}

/// A color that shows: text in no color would draw nothing.
fn text_color(color: i64) -> Result<u32, ApiError> {
    match input::color(color)? {
        0 => Err(ApiError::OutOfRange {
            what: "the text color, which must not be clear",
            value: 0,
            min: 1,
            max: i64::from(u32::MAX),
        }),
        shown => Ok(shown),
    }
}

/// Refuses a read of more than [`MOST_READ_PIXELS`] before anything is allocated.
fn read_fits(w: usize, h: usize) -> Result<(), ApiError> {
    let most = i64::try_from(MOST_READ_PIXELS).unwrap_or(i64::MAX);
    let area = i64::try_from(w.saturating_mul(h)).unwrap_or(i64::MAX);
    input::ranged("a block's area", area, 1, most).map(|_| ())
}

/// How many rows `count` colors make at `width` a row, or 0 when the width is not positive.
fn rows_of(width: i64, count: usize) -> i64 {
    let count = i64::try_from(count).unwrap_or(i64::MAX);
    match width > 0 {
        true => count / width,
        false => 0,
    }
}

/// A size inside the canvas, which the document's limits keep within `i32`.
fn to_i32(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}
