//! Pikslar Text, the proportional interface face: capitals eight pixels high, an x-height of
//! six, two rows of descenders and a line of twelve. Glyphs are one to [`MOST_WIDE`] columns wide
//! with one pixel of gap after each.
//!
//! The drawings in `font_text_data.rs` are packed into column bitmaps while compiling: bit 0 of
//! a column is the row above the capitals, bit 1 the cap line, and so on down to the lower
//! descender row. Nothing is parsed, rasterised or kept at run time.

use pikslar_render::{Rect, Surface};

#[path = "font_text_compose.rs"]
mod compose;
#[path = "font_text_data.rs"]
mod data;
#[cfg(test)]
#[path = "font_text_latin_tests.rs"]
mod latin_tests;
#[cfg(test)]
#[path = "font_text_tests.rs"]
mod tests;

use data::{Art, ASCII, OTHER};

/// Height of the capitals and digits, in font pixels.
pub const CAP: i32 = 8;
/// Height of the short lowercase letters.
pub const X_HEIGHT: i32 = 6;
/// Rows a glyph may use above the cap line, for the marks of capitals.
pub const ABOVE: i32 = 1;
/// Rows below the baseline, for descenders and marks below.
pub const BELOW: i32 = 2;
/// From one line's cap line to the next.
pub const LINE: i32 = 12;
/// A typical character's advance, for sizing by character count.
pub const EM: i32 = 6;
/// The widest glyph, in columns.
pub const MOST_WIDE: usize = 8;
/// Every row a glyph may use: above, capitals and below.
const ROWS: i32 = ABOVE + CAP + BELOW;
/// How far a character the face lacks moves the pen, per cell (see [`crate::font::cells`]).
const FALLBACK_ADVANCE: i32 = 7;

/// One glyph: its width and its columns, bit 0 the row above the cap line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub width: i32,
    pub cols: [u16; MOST_WIDE],
}

/// Glyphs packed one after the other: `C` columns in all, `S` starts (one per glyph and one past
/// the last), so a glyph's width is the distance to the next start.
struct Packed<const C: usize, const S: usize> {
    cols: [u16; C],
    starts: [u16; S],
}

/// The width of a drawing: the length of its first row.
const fn art_width(art: &str) -> usize {
    let b = art.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i] != b'/' {
        i += 1;
    }
    i
}

/// Columns of all the drawings together.
const fn total_width(arts: &[Art]) -> usize {
    let (mut sum, mut i) = (0, 0);
    while i < arts.len() {
        sum += art_width(arts[i].2);
        i += 1;
    }
    sum
}

/// Draws one drawing into `cols` from `at`, checking its rows while compiling.
const fn pack_one<const C: usize>(cols: &mut [u16; C], at: usize, (_, top, art): Art) {
    let w = art_width(art);
    assert!(
        w >= 1 && w <= MOST_WIDE,
        "a glyph is 1 to MOST_WIDE columns wide"
    );
    let b = art.as_bytes();
    let (mut row, mut x, mut i) = (top as i32, 0, 0);
    while i < b.len() {
        match b[i] {
            b'/' => {
                assert!(x == w, "every row of a drawing is as wide as the first");
                row += 1;
                x = 0;
            }
            b'#' | b'.' => {
                assert!(x < w, "every row of a drawing is as wide as the first");
                assert!(
                    row >= -ABOVE && row < CAP + BELOW,
                    "a row outside the frame"
                );
                if b[i] == b'#' {
                    cols[at + x] |= 1 << (row + ABOVE);
                }
                x += 1;
            }
            _ => panic!("a drawing holds only '#', '.' and '/'"),
        }
        i += 1;
    }
    assert!(x == w, "every row of a drawing is as wide as the first");
}

/// Packs the drawings, in their order.
const fn pack<const C: usize, const S: usize>(arts: &[Art]) -> Packed<C, S> {
    assert!(S == arts.len() + 1);
    let mut cols = [0u16; C];
    let mut starts = [0u16; S];
    let (mut at, mut g) = (0, 0);
    while g < arts.len() {
        starts[g] = at as u16;
        pack_one(&mut cols, at, arts[g]);
        at += art_width(arts[g].2);
        g += 1;
    }
    starts[g] = at as u16;
    assert!(at == C);
    Packed { cols, starts }
}

/// Checks while compiling that the ASCII table runs from space to tilde.
const fn ascii_in_order(arts: &[Art]) -> bool {
    let mut i = 0;
    while i < arts.len() {
        if arts[i].0 as usize != ' ' as usize + i {
            return false;
        }
        i += 1;
    }
    arts.len() == 95
}

const _: () = assert!(ascii_in_order(&ASCII), "ASCII runs from space to tilde");
const ASCII_COLS: usize = total_width(&ASCII);
static ASCII_PACKED: Packed<ASCII_COLS, 96> = pack(&ASCII);

/// The characters of the hand-drawn table, checking while compiling that they are sorted.
const fn chars_of<const N: usize>(arts: &[Art; N]) -> [char; N] {
    let mut out = ['\0'; N];
    let mut i = 0;
    while i < N {
        out[i] = arts[i].0;
        assert!(
            i == 0 || out[i - 1] < out[i],
            "hand-drawn glyphs are sorted"
        );
        assert!(
            out[i] as u32 > '~' as u32,
            "hand-drawn glyphs are beyond ASCII"
        );
        i += 1;
    }
    out
}

const OTHER_COLS: usize = total_width(&OTHER);
static OTHER_PACKED: Packed<OTHER_COLS, { OTHER.len() + 1 }> = pack(&OTHER);
static OTHER_CHARS: [char; OTHER.len()] = chars_of(&OTHER);

impl<const C: usize, const S: usize> Packed<C, S> {
    fn glyph(&self, index: usize) -> Option<Glyph> {
        let start = usize::from(*self.starts.get(index)?);
        let end = usize::from(*self.starts.get(index + 1)?);
        let mut cols = [0; MOST_WIDE];
        let src = self.cols.get(start..end)?;
        cols.get_mut(..src.len())?.copy_from_slice(src);
        Some(Glyph {
            width: src.len() as i32,
            cols,
        })
    }
}

/// The glyph of `ch`, or `None` when the face does not have it.
pub fn glyph(ch: char) -> Option<Glyph> {
    if (' '..='~').contains(&ch) {
        return ASCII_PACKED.glyph(ch as usize - ' ' as usize);
    }
    if let Ok(at) = OTHER_CHARS.binary_search(&ch) {
        return OTHER_PACKED.glyph(at);
    }
    compose::composed(ch)
}

/// How far the pen moves after `ch`, the gap included, in font pixels.
pub fn advance(ch: char) -> i32 {
    match glyph(ch) {
        Some(g) => g.width + 1,
        None => crate::font::cells(ch) * FALLBACK_ADVANCE,
    }
}

/// Width in pixels of `text` at the given scale, without the gap after the last character.
pub fn text_width(text: &str, scale: i32) -> i32 {
    let sum: i32 = text.chars().map(advance).fold(0, i32::saturating_add);
    (sum - 1).max(0).saturating_mul(scale)
}

/// Draws text with its cap line at `y` and its left edge at `x`. Marks of capitals reach one row
/// above `y`, descenders two below the baseline. Characters the face lacks come from the system
/// through [`crate::fallback`] in a box of the face's height, or become `?`.
pub fn draw_text(target: &mut Surface<'_>, x: i32, y: i32, text: &str, color: u32, scale: i32) {
    let mut pen = x;
    for ch in text.chars() {
        if pen >= target.w {
            break; // nothing further along is on the surface
        }
        let step = advance(ch).saturating_mul(scale);
        match glyph(ch) {
            Some(g) => draw_glyph(target, (pen, y), &g, color, scale),
            None => {
                let box_ = Rect::new(pen, y - ABOVE * scale, step - scale, ROWS * scale);
                let baseline = (ABOVE + CAP) * scale;
                if !crate::fallback::draw(target, box_, baseline, ch, color) {
                    if let Some(q) = glyph('?') {
                        draw_glyph(target, (pen, y), &q, color, scale);
                    }
                }
            }
        }
        pen = pen.saturating_add(step);
    }
}

/// Fills each column's runs of ink as one rectangle.
fn draw_glyph(target: &mut Surface<'_>, (x, y): (i32, i32), g: &Glyph, color: u32, scale: i32) {
    for (i, &bits) in g.cols.iter().enumerate().take(g.width as usize) {
        let left = x.saturating_add(i as i32 * scale);
        if left >= target.w {
            break;
        }
        let mut row = 0;
        while row < ROWS {
            if bits >> row & 1 == 0 {
                row += 1;
                continue;
            }
            let first = row;
            while row < ROWS && bits >> row & 1 == 1 {
                row += 1;
            }
            let top = y.saturating_add((first - ABOVE) * scale);
            target.fill(Rect::new(left, top, scale, (row - first) * scale), color);
        }
    }
}
