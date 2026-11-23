use super::*;

/// The rows (from the cap line) where `ch` has ink.
fn rows(ch: char) -> Vec<i32> {
    let g = glyph(ch).unwrap_or_else(|| panic!("no glyph for {ch:?}"));
    (-ABOVE..CAP + BELOW)
        .filter(|&r| g.cols.iter().any(|c| c >> (r + ABOVE) & 1 == 1))
        .collect()
}

/// Whether `ch` has ink at column `x`, row `r`.
fn ink(ch: char, x: usize, r: i32) -> bool {
    glyph(ch).is_some_and(|g| g.cols[x] >> (r + ABOVE) & 1 == 1)
}

#[test]
fn every_printable_ascii_character_has_a_glyph_of_a_sensible_width() {
    for ch in ' '..='~' {
        let g = glyph(ch).unwrap_or_else(|| panic!("no glyph for {ch:?}"));
        assert!((1..=MOST_WIDE as i32).contains(&g.width), "{ch:?}");
        assert_eq!(advance(ch), g.width + 1, "{ch:?}: one pixel of gap");
        let unused = g.cols[g.width as usize..].iter().any(|&c| c != 0);
        assert!(!unused, "{ch:?} has ink past its width");
        if ch != ' ' {
            assert!(
                g.cols[..g.width as usize].iter().any(|&c| c != 0),
                "{ch:?} is blank"
            );
        }
    }
    assert_eq!(glyph('\u{7f}'), None);
    assert_eq!(glyph('\u{1F600}'), None);
}

#[test]
fn capitals_and_digits_stand_eight_rows_on_the_baseline() {
    for ch in ('A'..='Z').chain('0'..='9') {
        let r = rows(ch);
        let tall = r.first() == Some(&0) && r.last() == Some(&(CAP - 1));
        assert!(tall || ch == 'Q', "{ch:?}: {r:?}");
    }
}

#[test]
fn short_letters_take_the_x_height_and_descenders_two_rows() {
    for ch in "acemnorsuvwxz".chars() {
        assert_eq!(
            rows(ch),
            (CAP - X_HEIGHT..CAP).collect::<Vec<_>>(),
            "{ch:?}"
        );
    }
    for ch in "gpqyj".chars() {
        assert_eq!(rows(ch).last(), Some(&(CAP + BELOW - 1)), "{ch:?}");
    }
    for ch in "bdhkl".chars() {
        assert_eq!(rows(ch).first(), Some(&0), "{ch:?}");
    }
    assert!(
        ink('i', 0, 0) && !ink('i', 0, 1),
        "the dot of i, apart from its stem"
    );
}

#[test]
fn nothing_in_ascii_reaches_the_row_above_the_capitals() {
    for ch in ' '..='~' {
        assert!(rows(ch).first().is_none_or(|&r| r >= 0), "{ch:?}");
    }
}

#[test]
fn text_is_narrower_than_the_doubled_mono_face() {
    let sample = "Layers Palette Timeline Brush size Opacity Export";
    let n = sample.chars().count() as i32;
    let w = text_width(sample, 1);
    assert!(
        w * 2 < n * 12 * 2,
        "about half the doubled face's twelve pixels"
    );
    assert!(
        w <= n * 11 / 2,
        "averaging at most 5.5 pixels a character: {w} for {n}"
    );
}

#[test]
fn width_adds_advances_less_the_last_gap_and_scales() {
    assert_eq!(text_width("", 3), 0);
    assert_eq!(text_width("i", 1), 1);
    assert_eq!(text_width("il", 1), 2 + 3 - 1);
    assert_eq!(text_width("il", 2), 8);
    assert_eq!(text_width("I", 3), 9);
}

#[test]
fn glyphs_land_where_expected_at_every_scale() {
    for scale in 1..=3 {
        let (w, h) = (12 * scale, 14 * scale);
        let mut px = vec![0u32; (w * h) as usize];
        let mut target = Surface { px: &mut px, w, h };
        draw_text(&mut target, scale, 2 * scale, "Il", 7, scale);
        let at = |x: i32, y: i32| px[(y * w + x) as usize];
        // I's top bar: columns 0 to 2 of the cell, row 0.
        assert_eq!(at(scale, 2 * scale), 7);
        assert_eq!(at(4 * scale - 1, 2 * scale + scale - 1), 7);
        assert_eq!(at(4 * scale, 2 * scale), 0, "the gap after I");
        // l starts one gap after I's three columns, and its foot turns right on row 7.
        assert_eq!(at(5 * scale, 2 * scale), 7);
        assert_eq!(at(6 * scale, 9 * scale), 7);
        assert_eq!(at(0, 0), 0);
    }
}

#[test]
fn off_screen_and_foreign_text_is_safe() {
    let mut px = vec![0u32; 16];
    let mut target = Surface {
        px: &mut px,
        w: 4,
        h: 4,
    };
    draw_text(&mut target, -3, -5, "Wé~\u{1F600}漢", 9, 3);
    draw_text(&mut target, i32::MAX - 10, 0, "WWW", 9, 1);
}

/// How many pixels differ between two glyphs drawn on the same cell, top left to top left.
fn pixels_apart(a: char, b: char) -> u32 {
    let (ga, gb) = (glyph(a).expect("glyph a"), glyph(b).expect("glyph b"));
    ga.cols
        .iter()
        .zip(gb.cols.iter())
        .map(|(x, y)| (x ^ y).count_ones())
        .sum()
}

#[test]
fn look_alike_characters_differ_by_more_than_a_pixel_or_two() {
    let pairs = [
        ('4', '9'),
        ('9', 'g'),
        ('9', 'q'),
        ('g', 'q'),
        ('0', 'O'),
        ('0', 'D'),
        ('O', 'D'),
        ('1', 'l'),
        ('1', 'I'),
        ('l', 'I'),
    ];
    for (a, b) in pairs {
        let apart = pixels_apart(a, b);
        assert!(apart >= 4, "{a:?} and {b:?} differ by only {apart} pixels");
    }
}

/// At one pixel a unit a zero with a mark inside reads as an eight ("800%" as "888%"), so the
/// zero is open: nothing inside its bowl, and it stays narrower than the capital O.
#[test]
fn a_zero_is_open_inside_so_it_never_reads_as_an_eight() {
    let inside: Vec<(usize, i32)> = (1..3)
        .flat_map(|x| (1..7).map(move |y| (x, y)))
        .filter(|&(x, y)| ink('0', x, y))
        .collect();
    assert!(inside.is_empty(), "ink inside the zero at {inside:?}");
    let width = |c: char| glyph(c).expect("glyph").width;
    assert!(width('0') < width('O'), "zero narrower than O");
    assert!(pixels_apart('0', '8') >= 4);
}

#[test]
fn the_top_of_four_is_open() {
    let top: Vec<usize> = (0..4).filter(|&x| ink('4', x, 0)).collect();
    assert_eq!(
        top.len(),
        1,
        "one pixel of the stem on the top row: {top:?}"
    );
}
