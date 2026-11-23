use super::compose::{squash, top_row};
use super::data::{Place, COMPOSED, MARKS};
use super::*;

/// Every character from U+0000 to U+FFFF, and the pictographs the interface uses.
fn all_chars() -> impl Iterator<Item = char> {
    (0..=0xffffu32).filter_map(char::from_u32)
}

#[test]
fn every_character_mono_draws_text_draws_too() {
    let missing: Vec<char> = all_chars()
        .filter(|&ch| pikslar_core::glyphs::has_glyph(ch) && glyph(ch).is_none())
        .collect();
    assert!(missing.is_empty(), "Pikslar Text lacks {missing:?}");
    for ch in "\u{d7}\u{b7}\u{2026}\u{2190}\u{2192}\u{2191}\u{2193}\u{2713}".chars() {
        assert!(glyph(ch).is_some(), "{ch:?}");
    }
}

#[test]
fn every_glyph_keeps_to_its_width_and_frame() {
    for ch in all_chars() {
        let Some(g) = glyph(ch) else { continue };
        assert!((1..=MOST_WIDE as i32).contains(&g.width), "{ch:?}");
        assert!(
            g.cols[g.width as usize..].iter().all(|&c| c == 0),
            "{ch:?} past its width"
        );
        let frame = (1u16 << (ABOVE + CAP + BELOW)) - 1;
        assert!(
            g.cols.iter().all(|&c| c & !frame == 0),
            "{ch:?} outside the frame"
        );
        let blank = g.cols.iter().all(|&c| c == 0);
        assert!(!blank || ch == ' ' || ch == '\u{a0}', "{ch:?} is blank");
    }
}

#[test]
fn a_tall_letter_always_has_a_row_to_give_up_for_its_mark() {
    for &(ch, base, mark) in &COMPOSED {
        let (place, arts) = MARKS[mark as usize];
        let g = glyph(char::from(base)).expect("an ASCII base");
        if place != Place::Above || top_row(&g) > 0 {
            continue;
        }
        let rows = arts.iter().map(|a| a.split('/').count()).max().unwrap_or(1) as i32;
        let mut letter = g;
        for _ in 0..(rows - 1).max(1) {
            letter = squash(&letter, top_row(&letter)).unwrap_or_else(|| panic!("{ch:?}"));
        }
    }
}

#[test]
fn accents_differ_from_their_letter_and_from_each_other() {
    for (i, &(a, base_a, _)) in COMPOSED.iter().enumerate() {
        let ga = glyph(a).expect("an accented letter");
        assert_ne!(
            Some(ga),
            glyph(char::from(base_a)),
            "{a:?} looks like its letter"
        );
        for &(b, base_b, _) in &COMPOSED[i + 1..] {
            if base_a == base_b {
                assert_ne!(Some(ga), glyph(b), "{a:?} and {b:?} look the same");
            }
        }
    }
}

#[test]
fn marks_sit_above_capitals_and_clear_of_small_letters() {
    let rows = |ch: char| -> Vec<i32> {
        let g = glyph(ch).expect("a glyph");
        (-ABOVE..CAP + BELOW)
            .filter(|&r| g.cols.iter().any(|c| c >> (r + ABOVE) & 1 == 1))
            .collect()
    };
    // Á: acute in the row above and the cap line, the A from row 1 to the baseline.
    assert_eq!(rows('Á'), (-1..CAP).collect::<Vec<_>>());
    // Ä: the diaeresis alone in the row above, the cap line left clear.
    assert!(rows('Ä').starts_with(&[-1, 1]));
    // ä: the diaeresis on the cap line, a row clear of the x-height.
    assert!(rows('ä').starts_with(&[0, 2]));
    // é: the acute from the row above, a row clear of the e.
    assert!(rows('é').starts_with(&[-1, 0, 2]));
    // ç and ș: below the baseline, in the two descender rows.
    assert_eq!(rows('ç').last(), Some(&(CAP + BELOW - 1)));
    assert_eq!(rows('ș').last(), Some(&(CAP + BELOW - 1)));
    // Every accented letter keeps its letter's width.
    for &(ch, base, _) in &COMPOSED {
        assert_eq!(
            glyph(ch).expect("a glyph").width,
            glyph(char::from(base)).expect("an ASCII base").width,
            "{ch:?}"
        );
    }
}

#[test]
fn accented_text_measures_and_draws_like_any_other() {
    let word = "Žluťoučký kůň úpěl ďábelské ódy";
    assert!(
        word.chars().all(|ch| glyph(ch).is_some()),
        "all in the face"
    );
    let (w, h) = (text_width(word, 2) + 4, 30);
    let mut px = vec![0u32; (w * h) as usize];
    let mut target = Surface { px: &mut px, w, h };
    draw_text(&mut target, 2, 4, word, 9, 2);
    assert!(px.iter().filter(|&&p| p == 9).count() > 200);
    assert!(
        px[..(w * 2) as usize].iter().all(|&p| p == 0),
        "nothing above the mark row"
    );
}

/// Lines of the specimen: every glyph of the face, then interface words.
const SPECIMEN: [&str; 9] = [
    " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~",
    "¡¢£¤¥¦§¨©ª«¬\u{ad}®¯°±²³´µ¶·¸¹º»¼½¾¿ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÐÑÒÓÔÕÖ×ØÙÚÛÜÝÞß",
    "àáâãäåæçèéêëìíîïðñòóôõö÷øùúûüýþÿĀāĂăĄąĆćĈĉĊċČčĎďĐđĒēĔĕĖėĘęĚě",
    "ĜĝĞğĠġĢģĤĥĦħĨĩĪīĬĭĮįİıĲĳĴĵĶķĸĹĺĻļĽľĿŀŁłŃńŅņŇňŉŊŋŌōŎŏŐőŒœ",
    "ŔŕŖŗŘřŚśŜŝŞşŠšŢţŤťŦŧŨũŪūŬŭŮůŰűŲųŴŵŶŷŸŹźŻżŽžſȘșȚț–—‘’“”„•…€←↑→↓✓",
    "File Edit View Layers Palette Timeline Brush size 12 px Opacity 100 % Export…",
    "Żółć gęślą jaźń · Příliš žluťoučký kůň · Árvíztűrő tükörfúrógép · Şoferiţă",
    "Frame 1/24 ← → ↑ ↓ ✓ × 0123456789 Tags: walk, run; Onion skin on",
    "The quick brown fox jumps over the lazy dog. THE QUICK BROWN FOX!",
];

/// Writes the specimen at 1x, 2x and 3x as a PPM to `PIKSLAR_SPECIMEN`. Run by hand:
/// `PIKSLAR_SPECIMEN=out.ppm cargo test -p pikslar-ui specimen -- --ignored`.
#[test]
#[ignore = "writes a file; run by hand for review"]
fn specimen() {
    let Ok(path) = std::env::var("PIKSLAR_SPECIMEN") else {
        return;
    };
    let (paper, ink) = (0x1e1f24u32, 0xe8e6e1u32);
    let w = 1 + SPECIMEN.iter().map(|l| text_width(l, 3)).max().unwrap_or(0) + 16;
    let h: i32 = (1..=3)
        .map(|s| (SPECIMEN.len() as i32 * LINE + 6) * s)
        .sum::<i32>()
        + 16;
    let mut px = vec![paper; (w * h) as usize];
    let mut y = 8;
    for scale in 1..=3 {
        for line in SPECIMEN {
            let mut target = Surface { px: &mut px, w, h };
            draw_text(&mut target, 8, y + ABOVE * scale, line, ink, scale);
            y += LINE * scale;
        }
        y += 6 * scale;
    }
    let mut out = format!("P6 {w} {h} 255\n").into_bytes();
    for p in px {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    std::fs::write(path, out).expect("the specimen is written");
}
