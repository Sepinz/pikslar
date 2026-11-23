//! The built-in 5x7 bitmap font: printable ASCII stored as one byte per column, and the Latin
//! letters beyond it from `glyphs_latin`. The interface draws with it, and so do text on the
//! canvas ([`rasterize`]) and the scripts ([`text_width`]), so they all agree.
//! The tiny 3x5 font is in `glyphs_tiny`.

use crate::floating::Clip;
use crate::glyphs_latin::LATIN;
use crate::glyphs_tiny::tiny_columns;

pub const GLYPH_W: i32 = 5;
pub const GLYPH_H: i32 = 7;
/// Rows a glyph may use above the cell, for the accents of capitals.
pub const ABOVE: i32 = 1;
/// Rows of a glyph's frame: the one above the cell, the seven of the cell, the descender row.
pub const FRAME_ROWS: i32 = ABOVE + GLYPH_H + 1;
const FIRST: u8 = 32;

#[rustfmt::skip]
const GLYPHS: [[u8; 5]; 95] = [
    [0x00,0x00,0x00,0x00,0x00],[0x00,0x00,0x5F,0x00,0x00],[0x00,0x07,0x00,0x07,0x00],
    [0x14,0x7F,0x14,0x7F,0x14],[0x24,0x2A,0x7F,0x2A,0x12],[0x23,0x13,0x08,0x64,0x62],
    [0x36,0x49,0x55,0x22,0x50],[0x00,0x05,0x03,0x00,0x00],[0x00,0x1C,0x22,0x41,0x00],
    [0x00,0x41,0x22,0x1C,0x00],[0x14,0x08,0x3E,0x08,0x14],[0x08,0x08,0x3E,0x08,0x08],
    [0x00,0x50,0x30,0x00,0x00],[0x08,0x08,0x08,0x08,0x08],[0x00,0x60,0x60,0x00,0x00],
    [0x20,0x10,0x08,0x04,0x02],[0x3E,0x51,0x49,0x45,0x3E],[0x00,0x42,0x7F,0x40,0x00],
    [0x42,0x61,0x51,0x49,0x46],[0x21,0x41,0x45,0x4B,0x31],[0x18,0x14,0x12,0x7F,0x10],
    [0x27,0x45,0x45,0x45,0x39],[0x3C,0x4A,0x49,0x49,0x30],[0x01,0x71,0x09,0x05,0x03],
    [0x36,0x49,0x49,0x49,0x36],[0x06,0x49,0x49,0x29,0x1E],[0x00,0x36,0x36,0x00,0x00],
    [0x00,0x56,0x36,0x00,0x00],[0x08,0x14,0x22,0x41,0x00],[0x14,0x14,0x14,0x14,0x14],
    [0x00,0x41,0x22,0x14,0x08],[0x02,0x01,0x51,0x09,0x06],[0x32,0x49,0x79,0x41,0x3E],
    [0x7E,0x11,0x11,0x11,0x7E],[0x7F,0x49,0x49,0x49,0x36],[0x3E,0x41,0x41,0x41,0x22],
    [0x7F,0x41,0x41,0x22,0x1C],[0x7F,0x49,0x49,0x49,0x41],[0x7F,0x09,0x09,0x09,0x01],
    [0x3E,0x41,0x49,0x49,0x7A],[0x7F,0x08,0x08,0x08,0x7F],[0x00,0x41,0x7F,0x41,0x00],
    [0x20,0x40,0x41,0x3F,0x01],[0x7F,0x08,0x14,0x22,0x41],[0x7F,0x40,0x40,0x40,0x40],
    [0x7F,0x02,0x0C,0x02,0x7F],[0x7F,0x04,0x08,0x10,0x7F],[0x3E,0x41,0x41,0x41,0x3E],
    [0x7F,0x09,0x09,0x09,0x06],[0x3E,0x41,0x51,0x21,0x5E],[0x7F,0x09,0x19,0x29,0x46],
    [0x46,0x49,0x49,0x49,0x31],[0x01,0x01,0x7F,0x01,0x01],[0x3F,0x40,0x40,0x40,0x3F],
    [0x1F,0x20,0x40,0x20,0x1F],[0x3F,0x40,0x38,0x40,0x3F],[0x63,0x14,0x08,0x14,0x63],
    [0x07,0x08,0x70,0x08,0x07],[0x61,0x51,0x49,0x45,0x43],[0x00,0x7F,0x41,0x41,0x00],
    [0x02,0x04,0x08,0x10,0x20],[0x00,0x41,0x41,0x7F,0x00],[0x04,0x02,0x01,0x02,0x04],
    [0x40,0x40,0x40,0x40,0x40],[0x00,0x01,0x02,0x04,0x00],[0x20,0x54,0x54,0x54,0x78],
    [0x7F,0x48,0x44,0x44,0x38],[0x38,0x44,0x44,0x44,0x20],[0x38,0x44,0x44,0x48,0x7F],
    [0x38,0x54,0x54,0x54,0x18],[0x08,0x7E,0x09,0x01,0x02],[0x18,0xA4,0xA4,0xA4,0x78],
    [0x7F,0x08,0x04,0x04,0x78],[0x00,0x44,0x7D,0x40,0x00],[0x20,0x40,0x44,0x3D,0x00],
    [0x7F,0x10,0x28,0x44,0x00],[0x00,0x41,0x7F,0x40,0x00],[0x7C,0x04,0x18,0x04,0x78],
    [0x7C,0x08,0x04,0x04,0x78],[0x38,0x44,0x44,0x44,0x38],[0xFC,0x24,0x24,0x24,0x18],
    [0x18,0x24,0x24,0x24,0xFC],[0x7C,0x08,0x04,0x04,0x08],[0x48,0x54,0x54,0x54,0x20],
    [0x04,0x3F,0x44,0x40,0x20],[0x3C,0x40,0x40,0x20,0x7C],[0x1C,0x20,0x40,0x20,0x1C],
    [0x3C,0x40,0x30,0x40,0x3C],[0x44,0x28,0x10,0x28,0x44],[0x1C,0xA0,0xA0,0xA0,0x7C],
    [0x44,0x64,0x54,0x4C,0x44],[0x00,0x08,0x36,0x41,0x00],[0x00,0x00,0x7F,0x00,0x00],
    [0x00,0x41,0x36,0x08,0x00],[0x10,0x08,0x08,0x10,0x08],
];

/// The glyph of `ch` in nine-row columns: the row above the cell, the seven of the cell, and the
/// descender row. `None` for characters the font does not have.
pub fn glyph(ch: char) -> Option<[u16; 5]> {
    if (' '..='~').contains(&ch) {
        return Some(GLYPHS[(ch as u8 - FIRST) as usize].map(|c| (c as u16) << ABOVE));
    }
    LATIN
        .binary_search_by_key(&ch, |&(c, _)| c)
        .ok()
        .map(|at| LATIN[at].1)
}

/// Whether the font draws `ch` as itself rather than as `?`.
pub fn has_glyph(ch: char) -> bool {
    glyph(ch).is_some()
}

/// Fonts the canvas text tool and the scripts can draw with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Font {
    /// The interface's 5x7 font.
    Pikslar5x7,
    /// A tiny 3x5 font of capitals, digits and punctuation, for labels and small text.
    Tiny3x5,
}

impl Font {
    /// Every font, in the order a key steps through them.
    pub const ALL: [Font; 2] = [Font::Pikslar5x7, Font::Tiny3x5];

    pub fn name(self) -> &'static str {
        match self {
            Font::Pikslar5x7 => "5x7",
            Font::Tiny3x5 => "3x5",
        }
    }

    /// The font after this one, going back instead if `back`.
    pub fn step(self, back: bool) -> Font {
        let at = Self::ALL
            .iter()
            .position(|&f| f == self)
            .unwrap_or_default();
        let n = Self::ALL.len();
        Self::ALL[(at + if back { n - 1 } else { 1 }) % n]
    }

    /// Columns of a glyph.
    pub fn cell_width(self) -> i32 {
        match self {
            Font::Pikslar5x7 => GLYPH_W,
            Font::Tiny3x5 => 3,
        }
    }

    /// Rows of a line, accents and descenders included, so a line never moves with its letters.
    pub fn line_rows(self) -> i32 {
        match self {
            Font::Pikslar5x7 => FRAME_ROWS,
            Font::Tiny3x5 => 5,
        }
    }

    /// The columns of `ch`, as bits from the top of the line, or `None` if the font lacks it.
    fn columns(self, ch: char) -> Option<([u16; 5], usize)> {
        match self {
            Font::Pikslar5x7 => glyph(ch).map(|g| (g, 5)),
            Font::Tiny3x5 => tiny_columns(ch),
        }
    }
}

/// Where the lines of a text sit against its widest line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    pub const ALL: [Align; 3] = [Align::Left, Align::Center, Align::Right];

    pub fn name(self) -> &'static str {
        match self {
            Align::Left => "Left",
            Align::Center => "Centre",
            Align::Right => "Right",
        }
    }

    /// The alignment after this one, going back instead if `back`.
    pub fn step(self, back: bool) -> Align {
        let at = Self::ALL
            .iter()
            .position(|&a| a == self)
            .unwrap_or_default();
        Self::ALL[(at + if back { 2 } else { 1 }) % 3]
    }

    /// How far a line `line` wide starts from the left of a text `whole` wide.
    fn offset(self, whole: i32, line: i32) -> i32 {
        match self {
            Align::Left => 0,
            Align::Center => (whole - line) / 2,
            Align::Right => whole - line,
        }
    }
}

/// Whether `font` draws `ch` as itself rather than as its question mark.
pub fn has_glyph_in(font: Font, ch: char) -> bool {
    font.columns(ch).is_some()
}

/// Text longer than this many characters, line breaks included, is cut.
pub const MAX_CHARS: usize = 256;
pub const MAX_SCALE: i32 = 8;
pub const MAX_SPACING: i32 = 3;

/// The lines of `text`, cut to [`MAX_CHARS`] characters, as lengths in characters.
fn line_lengths(text: &str) -> Vec<usize> {
    let mut lengths = vec![0];
    for ch in text.chars().take(MAX_CHARS) {
        if ch == '\n' {
            lengths.push(0);
        } else if let Some(last) = lengths.last_mut() {
            *last += 1;
        }
    }
    lengths
}

fn limited(scale: i32, spacing: i32) -> (i32, i32) {
    (scale.clamp(1, MAX_SCALE), spacing.clamp(0, MAX_SPACING))
}

/// Width in pixels of a line of `n` characters: no gap after the last.
fn line_width(font: Font, n: usize, scale: i32, spacing: i32) -> i32 {
    (n as i32 * (font.cell_width() + spacing) - spacing).max(0) * scale
}

/// Width in pixels of the widest line of `text` as [`rasterize`] draws it.
pub fn text_width(font: Font, text: &str, scale: i32, spacing: i32) -> i32 {
    let (scale, spacing) = limited(scale, spacing);
    line_lengths(text)
        .into_iter()
        .map(|n| line_width(font, n, scale, spacing))
        .max()
        .unwrap_or(0)
}

/// Height in pixels of `text`: its lines with one row between them.
pub fn text_height(font: Font, text: &str, scale: i32) -> i32 {
    let (scale, _) = limited(scale, 0);
    (line_lengths(text).len() as i32 * (font.line_rows() + 1) - 1) * scale
}

/// The most pixels a drawing of text may hold: 8 MiB of colors. Its characters fill far less of
/// a drawing whose short lines sit beside a long one, or under it, but the whole rectangle is
/// made. 256 characters at the largest scale, on one line or in a block, fit well inside it.
pub const MAX_TEXT_AREA: usize = 1 << 21;

/// Pixels in the drawing [`rasterize_aligned`] makes of `text`, whatever its alignment.
pub fn text_area(font: Font, text: &str, scale: i32, spacing: i32) -> usize {
    let wide = text_width(font, text, scale, spacing).max(0) as usize;
    if wide == 0 {
        return 0;
    }
    wide.saturating_mul(text_height(font, text, scale).max(0) as usize)
}

/// Where the caret goes before character `caret` of `text`, from the top-left of the drawing.
pub fn caret_position(
    font: Font,
    (scale, spacing): (i32, i32),
    align: Align,
    text: &str,
    caret: usize,
) -> (i32, i32) {
    let (scale, spacing) = limited(scale, spacing);
    let lengths = line_lengths(text);
    let before: Vec<char> = text.chars().take(caret.min(MAX_CHARS)).collect();
    let line = before.iter().filter(|&&c| c == '\n').count();
    let column = before.iter().rev().take_while(|&&c| c != '\n').count();
    let whole = text_width(font, text, scale, spacing);
    let own = lengths.get(line).copied().unwrap_or(0);
    let own = line_width(font, own, scale, spacing);
    let x = align.offset(whole, own) + column as i32 * (font.cell_width() + spacing) * scale;
    (x, line as i32 * (font.line_rows() + 1) * scale)
}

/// Draws `text` as a clip with its top-left at (0, 0), hard-edged, in `color`, its lines left
/// aligned. See [`rasterize_aligned`].
pub fn rasterize(
    font: Font,
    scale: i32,
    spacing: i32,
    text: &str,
    color: u32,
) -> (Clip, Vec<char>) {
    rasterize_aligned(font, (scale, spacing), Align::Left, text, color)
}

/// Draws `text`, whose lines break at `\n`, as a clip with its top-left at (0, 0). Every line is
/// as high as the font's line, so accents and descenders never move it. Characters the font lacks
/// are drawn as its `?` and listed once each, in order of appearance. `color` is a straight-alpha
/// color; pasting the clip maps it to a palette layer's swatch. No text, or only line breaks,
/// gives an empty clip, and so does a drawing larger than [`MAX_TEXT_AREA`], which callers
/// refuse first with [`text_area`].
pub fn rasterize_aligned(
    font: Font,
    (scale, spacing): (i32, i32),
    align: Align,
    text: &str,
    color: u32,
) -> (Clip, Vec<char>) {
    let (scale, spacing) = limited(scale, spacing);
    let whole = text_width(font, text, scale, spacing);
    if whole == 0 || text_area(font, text, scale, spacing) > MAX_TEXT_AREA {
        return (Clip::from_image(0, 0, Vec::new()), Vec::new());
    }
    let (w, h) = (whole as usize, text_height(font, text, scale) as usize);
    let mut colors = vec![0u32; w * h];
    let mut missing = Vec::new();
    let lengths = line_lengths(text);
    let s = scale as usize;
    let advance = ((font.cell_width() + spacing) * scale) as usize;
    let kept: String = text.chars().take(MAX_CHARS).collect();
    for (index, line) in kept.split('\n').enumerate() {
        let own = line_width(font, lengths[index], scale, spacing);
        let left = align.offset(whole, own) as usize;
        let top = index * (font.line_rows() as usize + 1) * s;
        for (i, ch) in line.chars().enumerate() {
            let (columns, n) = font.columns(ch).unwrap_or_else(|| {
                if !missing.contains(&ch) {
                    missing.push(ch);
                }
                font.columns('?').unwrap_or_default()
            });
            for (column, bits) in columns[..n].iter().enumerate() {
                let x = left + i * advance + column * s;
                for row in (0..font.line_rows() as usize).filter(|row| bits >> row & 1 == 1) {
                    for y in top + row * s..top + (row + 1) * s {
                        colors[y * w + x..y * w + x + s].fill(color);
                    }
                }
            }
        }
    }
    (Clip::from_image(w, h, colors), missing)
}

#[cfg(test)]
#[path = "glyphs_tests.rs"]
mod tests;
