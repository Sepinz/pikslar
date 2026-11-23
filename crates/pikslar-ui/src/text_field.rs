//! Drawing a line of text being edited: the selection behind it, the text, and the caret.

use crate::font::{advance, cap_height, draw_text, em};
use crate::text_edit::TextEdit;
use pikslar_render::{Rect, Surface};

/// How far the pen moves after `ch` at `scale`, the gap included.
fn step(ch: char, scale: i32) -> i32 {
    advance(ch).saturating_mul(scale.max(1))
}

/// The first character shown when `width` pixels hold the text: as many as fit, moved along
/// only as far as needed to keep the caret, and a typical character's room after it, in view.
pub fn first_shown(edit: &TextEdit, width: i32, scale: i32) -> usize {
    let room = width - em() * scale.max(1);
    let before: Vec<i32> = edit
        .text()
        .chars()
        .take(edit.caret())
        .map(|c| step(c, scale))
        .collect();
    let mut used = 0;
    let mut first = before.len();
    for wide in before.iter().rev() {
        if used + wide > room {
            break;
        }
        used += wide;
        first -= 1;
    }
    first
}

/// Draws `edit` with its top left at `at`, kept to `width` pixels: the selection behind the
/// text in `mark`, the text and the caret in `ink`. Returns the width drawn.
pub fn draw_text_edit(
    sf: &mut Surface<'_>,
    at: (i32, i32),
    width: i32,
    edit: &TextEdit,
    (ink, mark): (u32, u32),
    scale: i32,
) -> i32 {
    let first = first_shown(edit, width, scale);
    // Where each character from the first shown starts, in pixels, up to the last that fits.
    let mut starts = vec![0];
    for ch in edit.text().chars().skip(first) {
        let next = starts.last().copied().unwrap_or(0) + step(ch, scale);
        if next > width {
            break;
        }
        starts.push(next);
    }
    let shown_count = starts.len() - 1;
    let x_of = |chars: usize| {
        let i = chars.saturating_sub(first).min(shown_count);
        at.0 + starts.get(i).copied().unwrap_or(0)
    };
    // From the row above the capitals to the last row of a descender: two rows below the
    // baseline in Pikslar Text, one in Pikslar Mono.
    let below = match crate::font::face() {
        crate::font::Face::Text => 2,
        crate::font::Face::Mono => 1,
    };
    let (top, high) = (at.1 - scale, (cap_height() + 1 + below) * scale);
    let (from, to) = edit.selection();
    let (from, to) = (from.max(first), to.min(first + shown_count));
    if from < to {
        sf.fill(
            Rect::new(x_of(from) - scale, top, x_of(to) - x_of(from), high),
            mark,
        );
    }
    let shown: String = edit.text().chars().skip(first).take(shown_count).collect();
    draw_text(sf, at.0, at.1, &shown, ink, scale);
    sf.fill(Rect::new(x_of(edit.caret()) - scale, top, scale, high), ink);
    (x_of(first + shown_count) - at.0).min(width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::{set_face, text, Face, ADVANCE};
    use crate::text_edit::Edit;

    #[test]
    fn the_caret_stays_in_view() {
        set_face(Face::Mono);
        let mut e = TextEdit::new("abcdefghij", 40);
        // Room for four characters at scale 1.
        assert_eq!(
            first_shown(&e, 4 * ADVANCE, 1),
            7,
            "the end and the caret after it"
        );
        e.apply(Edit::Home { extend: false });
        assert_eq!(first_shown(&e, 4 * ADVANCE, 1), 0);
        assert_eq!(
            first_shown(&e, 0, 1),
            0,
            "no room still shows the caret's place"
        );
    }

    #[test]
    fn the_caret_is_drawn_where_the_next_character_goes() {
        set_face(Face::Mono);
        let mut px = vec![0u32; 80 * 12];
        let mut sf = Surface {
            px: &mut px,
            w: 80,
            h: 12,
        };
        let mut e = TextEdit::new("ab", 10);
        e.apply(Edit::End { extend: false });
        e.apply(Edit::Left { extend: false });
        draw_text_edit(&mut sf, (2, 2), 60, &e, (0xffff_ffff, 0xff00_00ff), 1);
        let at = |x: i32, y: i32| px[(y * 80 + x) as usize];
        assert_eq!(at(2 + ADVANCE - 1, 1), 0xffff_ffff, "caret between a and b");
        assert_eq!(at(2 + 2 * ADVANCE - 1, 1), 0, "not after b");
    }

    #[test]
    fn wide_characters_count_two_cells_for_the_caret() {
        set_face(Face::Mono);
        let mut e = TextEdit::new("漢字ab", 10);
        e.apply(Edit::End { extend: false });
        // Four cells of room: the caret's cell, b, a, and not 字's two.
        assert_eq!(first_shown(&e, 4 * ADVANCE, 1), 2);
        e.apply(Edit::Home { extend: false });
        e.apply(Edit::Right { extend: false });
        assert_eq!(first_shown(&e, 4 * ADVANCE, 1), 0);
        let mut px = vec![0u32; 80 * 12];
        let mut sf = Surface {
            px: &mut px,
            w: 80,
            h: 12,
        };
        let drawn = draw_text_edit(&mut sf, (2, 2), 60, &e, (0xffff_ffff, 0), 1);
        let at = |x: i32, y: i32| px[(y * 80 + x) as usize];
        assert_eq!(
            at(2 + 2 * ADVANCE - 1, 1),
            0xffff_ffff,
            "caret after the wide 漢"
        );
        assert_eq!(drawn, 6 * ADVANCE, "two wide and two narrow");
    }

    #[test]
    fn proportional_text_puts_the_caret_after_its_own_width() {
        let mut px = vec![0u32; 80 * 12];
        let mut sf = Surface {
            px: &mut px,
            w: 80,
            h: 12,
        };
        let mut e = TextEdit::new("il", 10);
        e.apply(Edit::End { extend: false });
        let drawn = draw_text_edit(&mut sf, (2, 2), 60, &e, (0xffff_ffff, 0), 1);
        let after = text::advance('i') + text::advance('l');
        assert_eq!(
            drawn, after,
            "the gap after the last character is the caret's"
        );
        let at = |x: i32, y: i32| px[(y * 80 + x) as usize];
        assert_eq!(
            at(2 + after - 1, 1),
            0xffff_ffff,
            "caret after l, not after 2 cells"
        );
        assert_eq!(first_shown(&e, 4 * text::EM, 1), 0, "a short word fits");
    }

    #[test]
    fn the_caret_reaches_the_bottom_of_a_descender() {
        let mut px = vec![0u32; 40 * 16];
        let mut sf = Surface {
            px: &mut px,
            w: 40,
            h: 16,
        };
        let mut e = TextEdit::new("gy", 10);
        e.apply(Edit::End { extend: false });
        draw_text_edit(&mut sf, (2, 3), 30, &e, (0xffff_ffff, 0), 1);
        let x = 2 + text::advance('g') + text::advance('y') - 1;
        let lit = |y: i32| px[(y * 40 + x) as usize] == 0xffff_ffff;
        assert!(lit(3 + text::CAP + 1), "the second descender row");
        assert!(lit(2), "the row above the capitals");
        assert!(!lit(3 + text::CAP + 2), "and no further");
    }
}
