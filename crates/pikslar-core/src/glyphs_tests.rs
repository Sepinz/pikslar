//! Tests for the built-in font and text rasterising.

use super::*;
use crate::color::rgba;

const INK: u32 = rgba(200, 30, 60, 255);

/// A clip as rows of '#' and '.'.
fn art(clip: &Clip) -> Vec<String> {
    clip.colors
        .chunks(clip.width.max(1))
        .map(|row| {
            row.iter()
                .map(|&c| if c == 0 { '.' } else { '#' })
                .collect()
        })
        .collect()
}

const A: [&str; 9] = [
    ".....", ".###.", "#...#", "#...#", "#...#", "#####", "#...#", "#...#", ".....",
];

#[test]
fn a_at_scale_one_matches_the_golden_pixels() {
    let (clip, missing) = rasterize(Font::Pikslar5x7, 1, 1, "A", INK);
    assert_eq!((clip.width, clip.height, clip.x, clip.y), (5, 9, 0, 0));
    assert_eq!(art(&clip), A);
    assert!(clip.colors.iter().all(|&c| c == 0 || c == INK));
    assert!(missing.is_empty());
}

#[test]
fn a_at_scale_three_is_each_pixel_tripled() {
    let (clip, _) = rasterize(Font::Pikslar5x7, 3, 1, "A", INK);
    assert_eq!((clip.width, clip.height), (15, 27));
    let want: Vec<String> = A
        .iter()
        .flat_map(|r| {
            let wide: String = r.chars().flat_map(|c| [c; 3]).collect();
            [wide.clone(), wide.clone(), wide]
        })
        .collect();
    assert_eq!(art(&clip), want);
}

#[test]
fn width_agrees_with_the_clip_and_has_no_trailing_gap() {
    for scale in 1..=8 {
        for spacing in 0..=3 {
            for text in ["", "A", "AB", "Hello, world", "é漢"] {
                let (clip, _) = rasterize(Font::Pikslar5x7, scale, spacing, text, INK);
                let width = text_width(Font::Pikslar5x7, text, scale, spacing);
                assert_eq!(clip.width as i32, width, "{text:?} x{scale} +{spacing}");
            }
        }
    }
    assert_eq!(text_width(Font::Pikslar5x7, "AB", 2, 1), 22);
}

#[test]
fn spacing_moves_the_next_character() {
    let (tight, _) = rasterize(Font::Pikslar5x7, 1, 0, "||", INK);
    let (loose, _) = rasterize(Font::Pikslar5x7, 1, 3, "||", INK);
    assert_eq!((tight.width, loose.width), (10, 13));
    assert_eq!(art(&tight)[4], "..#....#..");
    assert_eq!(art(&loose)[4], "..#.......#..");
}

#[test]
fn missing_characters_are_question_marks_and_listed_once() {
    let (clip, missing) = rasterize(Font::Pikslar5x7, 1, 1, "Ж?漢Ж", INK);
    assert_eq!(missing, vec!['Ж', '漢']);
    assert_eq!(clip.width, 4 * 5 + 3);
    let (marks, none) = rasterize(Font::Pikslar5x7, 1, 1, "????", INK);
    assert!(none.is_empty());
    assert_eq!(clip.colors, marks.colors);
}

#[test]
fn an_empty_string_gives_an_empty_clip() {
    let (clip, missing) = rasterize(Font::Pikslar5x7, 4, 2, "", INK);
    assert_eq!((clip.width, clip.height, clip.colors.len()), (0, 0, 0));
    assert!(missing.is_empty());
}

#[test]
fn scale_spacing_and_length_are_limited() {
    let (big, _) = rasterize(Font::Pikslar5x7, 99, 99, "A", INK);
    let (max, _) = rasterize(Font::Pikslar5x7, 8, 3, "A", INK);
    assert_eq!(big, max);
    let (small, _) = rasterize(Font::Pikslar5x7, -4, -4, "A", INK);
    let (one, _) = rasterize(Font::Pikslar5x7, 1, 0, "A", INK);
    assert_eq!(small, one);
    let long = "x".repeat(MAX_CHARS + 50);
    let (clip, _) = rasterize(Font::Pikslar5x7, 1, 1, &long, INK);
    assert_eq!(clip.width as i32, MAX_CHARS as i32 * 6 - 1);
    assert_eq!(text_width(Font::Pikslar5x7, &long, 1, 1), clip.width as i32);
}

#[test]
fn the_clip_is_well_formed_and_every_pixel_is_the_color_or_clear() {
    let (clip, _) = rasterize(Font::Pikslar5x7, 8, 3, "Éq", INK);
    assert!(clip.is_well_formed());
    assert!(clip.colors.iter().all(|&c| c == 0 || c == INK));
    assert!(clip.ids.is_none());
}

#[test]
fn latin_letters_have_glyphs_of_their_own() {
    let missing: String = (0xa0..=0x17f)
        .filter_map(char::from_u32)
        .filter(|&c| !has_glyph(c))
        .collect();
    assert_eq!(missing, "", "Latin-1 and Latin Extended-A");
    assert!("ȘșȚț€…–“”‘’„•".chars().all(has_glyph));
    assert!(!has_glyph('Ж') && !has_glyph('\u{1F600}'));
    assert!(
        LATIN.windows(2).all(|w| w[0].0 < w[1].0),
        "sorted for the search"
    );
    assert_ne!(glyph('é'), glyph('e'));
    assert_ne!(glyph('ě'), glyph('ĕ'), "caron and breve differ");
    assert_ne!(glyph('Ò'), glyph('ò'));
    assert!(LATIN.iter().all(|(_, g)| g.iter().all(|&c| c < 1 << 9)));
}

#[test]
fn pasted_into_a_palette_document_text_uses_the_colors_swatch() {
    use crate::document::{ColorMode, Document};
    use crate::editor::{Editor, Tool};
    let mut e = Editor::new(Document::new(32, 32, ColorMode::Palette).expect("valid"));
    e.pixel_perfect = false;
    e.color = INK;
    e.begin(Tool::Pencil, 20, 20).expect("begin");
    e.commit();
    let swatch = e.doc.layers[0].cel(0).get(20, 20);
    let colors = e.doc.colors.clone();
    let (clip, _) = rasterize(Font::Pikslar5x7, 1, 1, "A", INK);
    e.paste(&clip, 0, 0).expect("paste");
    e.anchor();
    assert_eq!(e.doc.layers[0].cel(0).get(1, 1), swatch);
    assert_eq!(e.doc.layers[0].cel(0).get(0, 0), 0);
    assert_eq!(e.doc.colors.len(), colors.len(), "no new swatch");
}

fn lit_columns(clip: &Clip, row: usize) -> Vec<usize> {
    (0..clip.width)
        .filter(|&x| clip.colors[row * clip.width + x] != 0)
        .collect()
}

#[test]
fn a_line_break_stacks_lines_with_a_row_between() {
    let (clip, missing) = rasterize(Font::Pikslar5x7, 1, 1, "A\nA", INK);
    assert_eq!((clip.width, clip.height), (5, 19));
    assert!(
        missing.is_empty(),
        "a line break is not a missing character"
    );
    let rows = art(&clip);
    assert_eq!(rows[..9], A);
    assert_eq!(rows[9], ".....", "the row between");
    assert_eq!(rows[10..], A);
    assert_eq!(text_height(Font::Pikslar5x7, "A\nA", 1), 19);
    assert_eq!(text_height(Font::Pikslar5x7, "A", 3), 27);
    assert_eq!(text_height(Font::Pikslar5x7, "", 1), 9);
}

#[test]
fn the_width_is_the_widest_line_and_agrees_with_the_clip() {
    let text = "AB\nA\nABC";
    assert_eq!(text_width(Font::Pikslar5x7, text, 1, 1), 17);
    for scale in [1, 2, 8] {
        let (clip, _) = rasterize(Font::Pikslar5x7, scale, 2, text, INK);
        let width = text_width(Font::Pikslar5x7, text, scale, 2);
        assert_eq!(clip.width as i32, width);
        assert_eq!(
            clip.height as i32,
            text_height(Font::Pikslar5x7, text, scale)
        );
    }
    let (blank, _) = rasterize(Font::Pikslar5x7, 2, 1, "\n\n", INK);
    assert_eq!(
        (blank.width, blank.height),
        (0, 0),
        "only breaks draw nothing"
    );
}

#[test]
fn centre_and_right_alignment_shift_the_shorter_lines() {
    let text = "AAA\nA";
    let at = |align| {
        let (clip, _) = rasterize_aligned(Font::Pikslar5x7, (1, 1), align, text, INK);
        assert_eq!(clip.width, 17);
        lit_columns(&clip, 15).first().copied()
    };
    assert_eq!(at(Align::Left), Some(0));
    assert_eq!(at(Align::Center), Some(6), "(17 - 5) / 2");
    assert_eq!(at(Align::Right), Some(12));
    assert_eq!(Align::Left.step(false), Align::Center);
    assert_eq!(Align::Left.step(true), Align::Right);
    assert_eq!(Align::Right.step(false), Align::Left);
}

#[test]
fn line_breaks_count_toward_the_256_character_limit() {
    let text = "x\n".repeat(200);
    let (clip, _) = rasterize(Font::Pikslar5x7, 1, 1, &text, INK);
    assert_eq!(clip.height as i32, text_height(Font::Pikslar5x7, &text, 1));
    assert_eq!(
        clip.height,
        129 * 10 - 1,
        "128 lines of x and the empty one"
    );
}

#[test]
fn the_caret_follows_the_line_and_the_alignment() {
    let f = Font::Pikslar5x7;
    let caret = |align, text: &str, at| caret_position(f, (1, 1), align, text, at);
    assert_eq!(caret(Align::Left, "ab", 0), (0, 0));
    assert_eq!(caret(Align::Left, "ab", 2), (12, 0));
    assert_eq!(caret(Align::Left, "ab\ncd", 3), (0, 10));
    assert_eq!(caret(Align::Left, "ab\ncd", 5), (12, 10));
    assert_eq!(
        caret(Align::Right, "abc\nd", 4),
        (12, 10),
        "a short line starts 12 in"
    );
    assert_eq!(caret(Align::Center, "abc\nd", 4), (6, 10));
    assert_eq!(caret_position(f, (3, 1), Align::Left, "a\nb", 3), (18, 30));
}

const TINY: Font = Font::Tiny3x5;
const TINY_A: [&str; 5] = [".#.", "#.#", "###", "#.#", "#.#"];

#[test]
fn tiny_a_at_scale_one_and_three_matches_the_golden_pixels() {
    let (clip, missing) = rasterize(TINY, 1, 1, "A", INK);
    assert_eq!((clip.width, clip.height), (3, 5));
    assert_eq!(art(&clip), TINY_A);
    assert!(missing.is_empty());
    let (big, _) = rasterize(TINY, 3, 1, "A", INK);
    assert_eq!((big.width, big.height), (9, 15));
    let want: Vec<String> = TINY_A
        .iter()
        .flat_map(|r| {
            let wide: String = r.chars().flat_map(|c| [c; 3]).collect();
            [wide.clone(), wide.clone(), wide]
        })
        .collect();
    assert_eq!(art(&big), want);
}

#[test]
fn tiny_letters_have_one_case_and_the_widths_follow_its_cells() {
    let (lower, _) = rasterize(TINY, 2, 1, "abc xyz", INK);
    let (upper, _) = rasterize(TINY, 2, 1, "ABC XYZ", INK);
    assert_eq!(lower, upper);
    assert_eq!(text_width(TINY, "AB", 1, 1), 7);
    assert_eq!(text_width(TINY, "AB", 2, 0), 12);
    assert_eq!(text_height(TINY, "A", 2), 10);
    assert_eq!(
        text_height(TINY, "A\nA", 1),
        11,
        "a line is five rows and one between"
    );
    let (two, _) = rasterize(TINY, 1, 1, "A\nA", INK);
    assert_eq!(art(&two)[5], "...");
    assert_eq!(art(&two)[6..], TINY_A);
    for text in ["", "A", "Hi there", "a\nbc\n"] {
        let (clip, _) = rasterize(TINY, 3, 2, text, INK);
        assert_eq!(clip.width as i32, text_width(TINY, text, 3, 2), "{text:?}");
        let height = if text.is_empty() {
            0
        } else {
            text_height(TINY, text, 3)
        };
        assert_eq!(clip.height as i32, height, "{text:?}");
    }
}

#[test]
fn tiny_draws_what_it_lacks_as_its_question_mark() {
    let (clip, missing) = rasterize(TINY, 1, 1, "\u{e9}\u{416}?", INK);
    assert_eq!(missing, vec!['\u{e9}', '\u{416}']);
    let (marks, none) = rasterize(TINY, 1, 1, "???", INK);
    assert!(none.is_empty());
    assert_eq!(clip.colors, marks.colors);
}

#[test]
fn every_printable_ascii_character_has_a_distinct_tiny_glyph() {
    use crate::glyphs_tiny::{tiny_chars, tiny_rows};
    let drawn: Vec<char> = tiny_chars().collect();
    assert!(drawn.windows(2).all(|w| w[0] < w[1]), "sorted");
    for c in ' '..='~' {
        let shown = if c.is_ascii_lowercase() {
            c.to_ascii_uppercase()
        } else {
            c
        };
        assert!(drawn.contains(&shown), "{c:?}");
        assert!(has_glyph_in(TINY, c), "{c:?}");
    }
    let mut seen: Vec<[&str; 5]> = Vec::new();
    for c in drawn {
        let rows = tiny_rows(c).expect("drawn");
        assert!(rows
            .iter()
            .all(|r| r.len() == 3 && r.bytes().all(|b| b == b'#' || b == b'.')));
        assert_eq!(rows.iter().any(|r| r.contains('#')), c != ' ', "{c:?}");
        assert!(!seen.contains(&rows), "{c:?} looks like another glyph");
        seen.push(rows);
    }
}

#[test]
fn fonts_step_through_both_and_have_names() {
    assert_eq!(Font::ALL, [Font::Pikslar5x7, Font::Tiny3x5]);
    assert_eq!(Font::Pikslar5x7.step(false), Font::Tiny3x5);
    assert_eq!(Font::Pikslar5x7.step(true), Font::Tiny3x5);
    assert_eq!(Font::Tiny3x5.step(false), Font::Pikslar5x7);
    assert_eq!(
        (Font::Pikslar5x7.name(), Font::Tiny3x5.name()),
        ("5x7", "3x5")
    );
}

/// One long line over many empty ones: few characters, but a drawing far larger than they fill.
fn sparse_tall() -> String {
    let mut text = "W".repeat(144);
    text.push_str(&"\n".repeat(101));
    text
}

#[test]
fn the_area_counts_the_whole_drawing_and_large_drawings_are_not_made() {
    let small = "Hello\nworld";
    let (clip, _) = rasterize(Font::Pikslar5x7, 3, 1, small, rgba(1, 2, 3, 255));
    assert_eq!(
        text_area(Font::Pikslar5x7, small, 3, 1),
        clip.width * clip.height
    );
    let tall = sparse_tall();
    let area = text_area(Font::Pikslar5x7, &tall, MAX_SCALE, 1);
    assert!(area > MAX_TEXT_AREA, "{area}");
    let (clip, _) = rasterize(Font::Pikslar5x7, MAX_SCALE, 1, &tall, rgba(1, 2, 3, 255));
    assert_eq!((clip.width, clip.height), (0, 0), "refused, not allocated");
}

#[test]
fn ordinary_text_at_the_largest_size_fits_the_area() {
    let line = "W".repeat(MAX_CHARS);
    assert!(text_area(Font::Pikslar5x7, &line, MAX_SCALE, MAX_SPACING) <= MAX_TEXT_AREA);
    let block: String = (0..16)
        .map(|_| "W".repeat(15))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text_area(Font::Pikslar5x7, &block, MAX_SCALE, 1) <= MAX_TEXT_AREA);
}
