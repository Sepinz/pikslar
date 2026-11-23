//! Accented letters of Pikslar Text, made when drawn from an ASCII letter and one of the marks
//! in `font_text_data.rs`. Marks and the table of letters are packed while compiling; a letter
//! costs four bytes and nothing is kept per accented glyph.
//!
//! A mark above a short letter sits one row clear of it, from the row above the capitals down.
//! A letter that reaches the cap line (a capital, `h`, `l`) loses a row that repeats its
//! neighbour, two for a ring, so the mark fits in the row above the capitals and the cap line.

use super::data::{MarkDef, Place, COMPOSED, MARKS};
use super::{glyph, Glyph, ABOVE, CAP, MOST_WIDE};

/// The most drawings a mark has, columns a drawing has and rows a drawing has.
const ARTS: usize = 3;
const MARK_W: usize = 5;
const MARK_H: usize = 3;

/// One drawing of a mark: its width, its height and its columns, bit 0 its top row.
#[derive(Clone, Copy)]
struct MarkArt {
    w: u8,
    h: u8,
    cols: [u8; MARK_W],
}

/// A mark packed: where it goes and its drawings, narrow to wide.
#[derive(Clone, Copy)]
struct Mark {
    place: Place,
    count: u8,
    arts: [MarkArt; ARTS],
}

const fn pack_art(art: &str) -> MarkArt {
    let b = art.as_bytes();
    let mut out = MarkArt {
        w: 0,
        h: 1,
        cols: [0; MARK_W],
    };
    let (mut x, mut row, mut i) = (0usize, 0usize, 0);
    while i < b.len() {
        match b[i] {
            b'/' => {
                assert!(
                    row == 0 || x == out.w as usize,
                    "a mark's rows are equally wide"
                );
                out.w = x as u8;
                x = 0;
                row += 1;
                out.h += 1;
            }
            b'#' | b'.' => {
                assert!(x < MARK_W && row < MARK_H, "a mark is at most 5 by 3");
                if b[i] == b'#' {
                    out.cols[x] |= 1 << row;
                }
                x += 1;
            }
            _ => panic!("a mark holds only '#', '.' and '/'"),
        }
        i += 1;
    }
    assert!(
        row == 0 || x == out.w as usize,
        "a mark's rows are equally wide"
    );
    out.w = x as u8;
    out
}

const fn pack_marks(defs: &[MarkDef]) -> [Mark; MARKS.len()] {
    let empty = MarkArt {
        w: 0,
        h: 0,
        cols: [0; MARK_W],
    };
    let mut out = [Mark {
        place: Place::Above,
        count: 0,
        arts: [empty; ARTS],
    }; MARKS.len()];
    let mut m = 0;
    while m < defs.len() {
        let (place, arts) = defs[m];
        assert!(
            !arts.is_empty() && arts.len() <= ARTS,
            "a mark has one to three drawings"
        );
        out[m].place = place;
        out[m].count = arts.len() as u8;
        let mut a = 0;
        while a < arts.len() {
            out[m].arts[a] = pack_art(arts[a]);
            assert!(
                a == 0 || out[m].arts[a - 1].w < out[m].arts[a].w,
                "narrow to wide"
            );
            a += 1;
        }
        m += 1;
    }
    out
}

/// Each letter as its character (high 16 bits), base letter and mark; checked sorted, with
/// ASCII bases and known marks, while compiling.
const fn pack_letters(table: &[(char, u8, u8)]) -> [u32; COMPOSED.len()] {
    let mut out = [0u32; COMPOSED.len()];
    let mut i = 0;
    while i < table.len() {
        let (ch, base, mark) = table[i];
        assert!(
            (ch as u32) > 0x7e && (ch as u32) <= 0xffff,
            "a letter beyond ASCII, in 16 bits"
        );
        assert!(base > b' ' && base <= b'~', "an ASCII base");
        assert!((mark as usize) < MARKS.len(), "a known mark");
        out[i] = (ch as u32) << 16 | (base as u32) << 8 | mark as u32;
        assert!(
            i == 0 || out[i - 1] >> 16 < out[i] >> 16,
            "letters are sorted"
        );
        i += 1;
    }
    out
}

static PACKED_MARKS: [Mark; MARKS.len()] = pack_marks(&MARKS);
static LETTERS: [u32; COMPOSED.len()] = pack_letters(&COMPOSED);

/// The accented letter `ch`, or `None` when it is not one the face makes.
pub(super) fn composed(ch: char) -> Option<Glyph> {
    let key = u32::from(ch);
    let at = LETTERS.binary_search_by_key(&key, |e| e >> 16).ok()?;
    let entry = LETTERS[at];
    let base = glyph(char::from((entry >> 8) as u8))?;
    Some(compose(base, PACKED_MARKS.get((entry & 0xff) as usize)?))
}

/// The drawing of `mark` for a letter `width` wide: the widest that fits and centres exactly,
/// else the widest that fits, else the narrowest.
fn art_for(mark: &Mark, width: i32) -> MarkArt {
    let arts = &mark.arts[..usize::from(mark.count)];
    let fits = |a: &&MarkArt| i32::from(a.w) <= width;
    arts.iter()
        .rev()
        .filter(fits)
        .find(|a| (width - i32::from(a.w)) % 2 == 0)
        .or_else(|| arts.iter().rev().find(fits))
        .copied()
        .unwrap_or(arts[0])
}

/// The first row (from the cap line) with ink, or the baseline row for a blank glyph.
pub(super) fn top_row(g: &Glyph) -> i32 {
    let ink = g.cols.iter().fold(0u16, |all, c| all | c);
    if ink == 0 {
        CAP - 1
    } else {
        ink.trailing_zeros() as i32 - ABOVE
    }
}

/// Removes the first row below `top` that repeats the one above it, moving everything above it
/// down a row. Returns `None` when no row repeats.
pub(super) fn squash(g: &Glyph, top: i32) -> Option<Glyph> {
    let width = g.width as usize;
    let row = (top + 1..CAP).find(|&r| {
        let (b, a) = (r + ABOVE, r - 1 + ABOVE);
        g.cols[..width].iter().all(|c| c >> b & 1 == c >> a & 1)
    })?;
    let bit = (row + ABOVE) as u32;
    let below = !((1u16 << (bit + 1)) - 1);
    let above = (1u16 << bit) - 1;
    let mut cols = [0u16; MOST_WIDE];
    for (out, &c) in cols.iter_mut().zip(&g.cols).take(width) {
        *out = (c & below) | (c & above) << 1;
    }
    Some(Glyph {
        width: g.width,
        cols,
    })
}

/// `base` with `mark` added.
fn compose(base: Glyph, mark: &Mark) -> Glyph {
    let art = art_for(mark, base.width);
    let (aw, ah) = (i32::from(art.w), i32::from(art.h));
    let (mut letter, top) = match mark.place {
        Place::Above => place_above(base, ah),
        Place::Below => (base, CAP),
        Place::BelowRight => (base, CAP),
    };
    let left = match mark.place {
        Place::BelowRight => base.width - aw,
        _ => (base.width - aw + 1) / 2,
    }
    .max(0);
    let shift = top + ABOVE;
    for (x, &bits) in art.cols.iter().enumerate().take(art.w as usize) {
        if let Some(col) = letter.cols.get_mut(left as usize + x) {
            if (left as usize + x) < base.width as usize {
                *col |= u16::from(bits) << shift;
            }
        }
    }
    letter
}

/// The letter made room for a mark `h` rows high above it, and the mark's top row.
fn place_above(base: Glyph, h: i32) -> (Glyph, i32) {
    let top = top_row(&base);
    if top > 0 {
        return (base, (top - 1 - h).max(-ABOVE));
    }
    let mut letter = base;
    for _ in 0..(h - 1).max(1) {
        let t = top_row(&letter);
        letter = squash(&letter, t).unwrap_or(letter);
    }
    (letter, -ABOVE)
}
