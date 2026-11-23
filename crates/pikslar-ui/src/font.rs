//! The interface's text drawing, in one of two faces: Pikslar Text, the proportional face of
//! [`text`] (the default), or Pikslar Mono, the built-in 5x7 font of [`pikslar_core::glyphs`],
//! drawn doubled for the *Large text* option.

use std::cell::{Cell, RefCell};

use pikslar_core::glyphs::{glyph, ABOVE};
pub use pikslar_core::glyphs::{GLYPH_H, GLYPH_W};
use pikslar_render::{Rect, Surface};

/// Pikslar Text, the proportional face.
#[path = "font_text.rs"]
pub mod text;

/// How far the pen moves for each character in Pikslar Mono, the gap after it included.
pub const ADVANCE: i32 = GLYPH_W + 1;

/// The face interface words are drawn in. Both draw with the cap line at the `y` they are
/// given; Text is narrower and its line taller (see [`text::LINE`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Face {
    /// Pikslar Mono, 5 by 7 with a fixed advance, drawn doubled: the *Large text* option.
    Mono,
    /// Pikslar Text, proportional, capitals 8 high: the default.
    #[default]
    Text,
}

thread_local! {
    static FACE: Cell<Face> = const { Cell::new(Face::Text) };
    static LOG: RefCell<Option<DrawLog>> = const { RefCell::new(None) };
}

/// What was drawn while logging: every text's rectangle with its words, and every face or well.
#[derive(Debug, Default)]
pub struct DrawLog {
    pub texts: Vec<(Rect, String)>,
    pub boxes: Vec<Rect>,
}

/// Starts or stops keeping what is drawn on this thread, for tests that check words against the
/// controls that hold them. Off by default; a drawn text then costs one flag check.
pub fn log_text(on: bool) {
    LOG.with(|l| *l.borrow_mut() = on.then(DrawLog::default));
}

/// What was drawn since logging started or this was last called.
pub fn take_text_log() -> DrawLog {
    LOG.with(|l| {
        l.borrow_mut()
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    })
}

/// Notes a drawn control face or well, while logging.
pub(crate) fn note_box(r: Rect) {
    LOG.with(|l| {
        if let Some(log) = l.borrow_mut().as_mut() {
            log.boxes.push(r);
        }
    });
}

/// The rows below the baseline a text reaches, in font pixels: one in Mono, two in Text.
fn ink_below(text: &str) -> i32 {
    let hangs = text.chars().any(|c| "gjpqy,;()[]{}|Q@$".contains(c));
    match (hangs, face()) {
        (false, _) => 0,
        (true, Face::Mono) => 1,
        (true, Face::Text) => 2,
    }
}

fn note_text(x: i32, y: i32, text: &str, scale: i32) {
    LOG.with(|l| {
        if let Some(log) = l.borrow_mut().as_mut() {
            let h = (cap_height() + ink_below(text)) * scale;
            let r = Rect::new(x, y, text_width(text, scale), h);
            log.texts.push((r, text.to_string()));
        }
    });
}

/// Draws and measures interface words in `face` from now on, on this thread.
pub fn set_face(face: Face) {
    FACE.with(|f| f.set(face));
}

/// The face interface words are drawn in now, on this thread.
pub fn face() -> Face {
    FACE.with(Cell::get)
}

/// How many pixels a font pixel is drawn at on an interface of scale `scale`: Pikslar Text is
/// drawn a pixel to the unit, Pikslar Mono doubled to stay readable.
pub fn text_scale(scale: i32) -> i32 {
    match face() {
        Face::Mono => scale * 2,
        Face::Text => scale,
    }
}

/// The height of the capitals in font pixels in the current face, for centring a line.
pub fn cap_height() -> i32 {
    match face() {
        Face::Mono => GLYPH_H,
        Face::Text => text::CAP,
    }
}

/// The height in units of a one-line control whose least is `base`: tall enough for the face's
/// capitals with four units of padding above and below, in whole fours (the spacing grid of
/// docs/ui-redesign.md 2.5). Pikslar Text keeps every base of 16 or more; the doubled Mono of
/// *Large text* grows the 16, 20 and 22-unit controls to 24.
pub fn fit_h(base: i32) -> i32 {
    let need = cap_height() * text_scale(1) + 8;
    base.max((need + 3) / 4 * 4)
}

/// A typical character's advance in font pixels in the current face, for sizing fields and
/// columns by how many characters they hold.
pub fn em() -> i32 {
    match face() {
        Face::Mono => ADVANCE,
        Face::Text => text::EM,
    }
}

/// How far the pen moves after `ch` in the current face, the gap included, at scale 1.
pub fn advance(ch: char) -> i32 {
    match face() {
        Face::Mono => cells(ch) * ADVANCE,
        Face::Text => text::advance(ch),
    }
}

/// Width in pixels of `text` at the given scale, in the current face.
pub fn text_width(text: &str, scale: i32) -> i32 {
    if face() == Face::Text {
        return text::text_width(text, scale);
    }
    (text.chars().map(cells).sum::<i32>() * ADVANCE - 1).max(0) * scale
}

/// How many cells `ch` takes: two for the wide characters of Chinese, Japanese and Korean and
/// for pictographs, one for every other.
pub fn cells(ch: char) -> i32 {
    let wide = matches!(ch as u32,
        0x1100..=0x115f | 0x2e80..=0x303e | 0x3041..=0x33ff | 0x3400..=0x4dbf
        | 0x4e00..=0x9fff | 0xa000..=0xa4cf | 0xac00..=0xd7a3 | 0xf900..=0xfaff
        | 0xfe30..=0xfe4f | 0xff00..=0xff60 | 0xffe0..=0xffe6 | 0x1f300..=0x1f64f
        | 0x1f900..=0x1f9ff | 0x20000..=0x3fffd);
    1 + i32::from(wide)
}

/// Draws text in the current face with its top-left at (x, y). Accents on capitals reach one
/// row above it. Characters the font does not have come from the system, through
/// [`crate::fallback`], or become `?` when it has none either.
pub fn draw_text(target: &mut Surface<'_>, x: i32, y: i32, text: &str, color: u32, scale: i32) {
    note_text(x, y, text, scale);
    if face() == Face::Text {
        return text::draw_text(target, x, y, text, color, scale);
    }
    let mut pen = x;
    for ch in text.chars() {
        let wide = cells(ch);
        let drawn = match glyph(ch) {
            Some(columns) => {
                draw_columns(target, (pen, y), &columns, color, scale);
                true
            }
            None => {
                // The system's glyph gets the cells' whole box, from the row above to the
                // descender row, with the baseline where the font's is.
                let w = (wide * ADVANCE - 1) * scale;
                let box_ = Rect::new(pen, y - ABOVE * scale, w, (GLYPH_H + 1 + ABOVE) * scale);
                crate::fallback::draw(target, box_, (GLYPH_H + ABOVE) * scale, ch, color)
            }
        };
        if !drawn {
            let columns = glyph('?').unwrap_or_default();
            draw_columns(target, (pen, y), &columns, color, scale);
        }
        pen += wide * ADVANCE * scale;
    }
}

fn draw_columns(
    target: &mut Surface<'_>,
    (x, y): (i32, i32),
    columns: &[u16; 5],
    color: u32,
    scale: i32,
) {
    for (column, bits) in columns.iter().enumerate() {
        // One row below the baseline holds the descenders of g, p, q and y.
        for row in 0..=GLYPH_H + ABOVE {
            if bits >> row & 1 == 1 {
                let top = y + (row - ABOVE) * scale;
                let px = Rect::new(x + column as i32 * scale, top, scale, scale);
                target.fill(px, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_scales_and_has_no_trailing_gap() {
        set_face(Face::Mono);
        assert_eq!(text_width("", 2), 0);
        assert_eq!(text_width("A", 1), 5);
        assert_eq!(text_width("AB", 2), 22);
    }

    #[test]
    fn glyphs_land_where_expected() {
        set_face(Face::Mono);
        let mut px = vec![0u32; 6 * 7];
        let mut target = Surface {
            px: &mut px,
            w: 6,
            h: 7,
        };
        draw_text(&mut target, 0, 0, "|", 9, 1);
        assert_eq!(px[2], 9);
        assert_eq!(px[6 * 6 + 2], 9);
        assert_eq!(px[0], 0);
    }

    #[test]
    fn off_screen_and_foreign_text_is_safe() {
        set_face(Face::Mono);
        let mut px = vec![0u32; 16];
        let mut target = Surface {
            px: &mut px,
            w: 4,
            h: 4,
        };
        draw_text(&mut target, -3, -3, "é~\u{1F600}", 9, 3);
    }

    /// Draws one character at (0, 2) on a 6 by 12 surface and lists the rows it touches,
    /// counted from the cell's top.
    fn rows_of(ch: char) -> Vec<i32> {
        set_face(Face::Mono);
        let mut px = vec![0u32; 6 * 12];
        let mut target = Surface {
            px: &mut px,
            w: 6,
            h: 12,
        };
        draw_text(&mut target, 0, 2, &ch.to_string(), 9, 1);
        (0..12)
            .filter(|y| px[(y * 6) as usize..(y * 6 + 6) as usize].contains(&9))
            .map(|y| y - 2)
            .collect()
    }

    #[test]
    fn accents_go_over_capitals_and_cedillas_under_the_letter() {
        set_face(Face::Mono);
        assert_eq!(rows_of('E').first(), Some(&0));
        assert_eq!(rows_of('É').first(), Some(&-1), "one row above, never more");
        assert_eq!(
            rows_of('é').first(),
            Some(&0),
            "inside the cell over a short letter"
        );
        // A capital with a mark stays taller than the lowercase letter with it.
        assert_eq!(rows_of('Ò').iter().filter(|&&r| r >= 0).count(), 7);
        assert_eq!(rows_of('ò').iter().filter(|&&r| r >= 0).count(), 7);
        assert_ne!(glyph('Ò'), glyph('ò'));
        assert_eq!(rows_of('ç').last(), Some(&7), "in the descender row");
        assert_eq!(rows_of('c').last(), Some(&6));
        assert_eq!(rows_of('é').last(), rows_of('e').last());
    }

    #[test]
    fn wide_characters_take_two_cells() {
        set_face(Face::Mono);
        assert_eq!(cells('a'), 1);
        assert_eq!(cells('Ж'), 1);
        assert_eq!(cells('漢'), 2);
        assert_eq!(cells('한'), 2);
        assert_eq!(cells('ア'), 2);
        assert_eq!(text_width("a漢", 1), 3 * ADVANCE - 1);
        assert_eq!(text_width("漢", 2), (2 * ADVANCE - 1) * 2);
    }

    /// A source that fills the whole box it is given, so the box can be seen.
    fn fill_box(_: char, w: i32, h: i32, _: i32) -> Option<Vec<u8>> {
        Some(vec![1; (w * h) as usize])
    }

    #[test]
    fn the_system_draws_what_the_font_lacks_in_the_cells_box() {
        set_face(Face::Mono);
        let mut px = vec![0u32; 30 * 14];
        let mut target = Surface {
            px: &mut px,
            w: 30,
            h: 14,
        };
        crate::fallback::set_source(Some(fill_box));
        draw_text(&mut target, 1, 2, "漢a", 9, 1);
        crate::fallback::set_source(None);
        let at = |x: i32, y: i32| px[(y * 30 + x) as usize];
        assert_eq!(at(1, 1), 9, "from the row above the cell");
        assert_eq!(at(1, 2 + 7), 9, "to the descender row");
        assert_eq!(at(1, 2 + 8), 0);
        assert_eq!(
            at(1 + 2 * ADVANCE - 2, 5),
            9,
            "two cells wide, less the gap"
        );
        assert_eq!(at(1 + 2 * ADVANCE - 1, 5), 0);
        let a_drawn = (0..14).any(|y| (0..GLYPH_W).any(|x| at(1 + 2 * ADVANCE + x, y) == 9));
        assert!(a_drawn, "the next character two cells along");
        assert_eq!(rows_of('Ж'), rows_of('?'), "no source: a question mark");
    }

    #[test]
    fn text_is_the_face_until_large_text_is_chosen() {
        assert_eq!(face(), Face::Text);
        assert_eq!(text_scale(2), 2, "a font pixel a unit");
        assert_eq!(cap_height(), 8);
        set_face(Face::Mono);
        assert_eq!(text_scale(2), 4, "Mono is drawn doubled");
        assert_eq!(cap_height(), GLYPH_H);
        assert_eq!(em(), ADVANCE);
        set_face(Face::Text);
        assert_eq!(em(), text::EM);
        set_face(Face::Mono);
        assert_eq!(advance('i'), ADVANCE);
        assert_eq!(text_width("Layers", 1), 6 * ADVANCE - 1);
        set_face(Face::Text);
        assert_eq!(advance('i'), text::advance('i'));
        assert_eq!(text_width("Layers", 2), text::text_width("Layers", 2));
        assert!(
            text_width("Layers", 1) < 6 * ADVANCE - 1,
            "Text is narrower"
        );
        set_face(Face::Mono);
        assert_eq!(text_width("Layers", 1), 6 * ADVANCE - 1);
        set_face(Face::Text);
    }

    #[test]
    fn draw_text_follows_the_face() {
        let draw = |face: Face| {
            set_face(face);
            let mut px = vec![0u32; 40 * 14];
            let mut target = Surface {
                px: &mut px,
                w: 40,
                h: 14,
            };
            draw_text(&mut target, 0, 2, "Hi", 9, 1);
            set_face(Face::Mono);
            px
        };
        let mut want = vec![0u32; 40 * 14];
        let mut target = Surface {
            px: &mut want,
            w: 40,
            h: 14,
        };
        text::draw_text(&mut target, 0, 2, "Hi", 9, 1);
        assert_eq!(draw(Face::Text), want);
        assert_ne!(draw(Face::Mono), want);
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;

    #[test]
    fn control_heights_follow_the_capitals_of_the_face() {
        set_face(Face::Text);
        assert_eq!([fit_h(16), fit_h(20), fit_h(24)], [16, 20, 24]);
        set_face(Face::Mono);
        assert_eq!(
            [fit_h(16), fit_h(20), fit_h(22), fit_h(28)],
            [24, 24, 24, 28]
        );
        set_face(Face::Text);
    }
}
