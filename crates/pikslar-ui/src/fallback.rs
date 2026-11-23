//! Characters the built-in font does not have, such as Cyrillic, Greek or Chinese, drawn by the
//! system the first time they are shown and kept for the next frames.
//!
//! Nothing happens until such a character is drawn, so launch and memory are unchanged for text
//! the built-in font covers. What is kept is bounded: at most [`MOST_KEPT`] glyphs, each one byte a
//! pixel of its box, and all are dropped together when the limit is reached.

use pikslar_render::{Rect, Surface};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// Draws `ch` into a `w` by `h` box whose baseline is `baseline` rows from its top. Returns one
/// byte a pixel, row by row, nonzero where the glyph is, or `None` if there is no glyph for it.
pub type Rasterize = fn(ch: char, w: i32, h: i32, baseline: i32) -> Option<Vec<u8>>;

/// The most glyphs kept at once: more than any screen shows, so a frame never has to draw one
/// twice, and under a megabyte at the largest interface size.
pub const MOST_KEPT: usize = 1024;

type Key = (char, i32, i32);

thread_local! {
    static SOURCE: Cell<Option<Rasterize>> = const { Cell::new(None) };
    static KEPT: RefCell<HashMap<Key, Option<Vec<u8>>>> = RefCell::new(HashMap::new());
}

/// Sets where characters the built-in font lacks come from; `None` draws them as `?`. Drops
/// every glyph kept so far.
pub fn set_source(source: Option<Rasterize>) {
    SOURCE.with(|s| s.set(source));
    KEPT.with(|k| k.borrow_mut().clear());
}

/// How many glyphs are kept, and the bytes their pixels hold.
pub fn kept() -> (usize, usize) {
    KEPT.with(|k| {
        let k = k.borrow();
        let bytes = k.values().flatten().map(Vec::len).sum();
        (k.len(), bytes)
    })
}

/// Draws `ch` from the source in the `box_` given, its baseline `baseline` pixels from the top.
/// Returns false, drawing nothing, when there is no source or no glyph.
pub(crate) fn draw(
    target: &mut Surface<'_>,
    box_: Rect,
    baseline: i32,
    ch: char,
    color: u32,
) -> bool {
    let Some(source) = SOURCE.with(Cell::get) else {
        return false;
    };
    let (w, h) = (box_.w, box_.h);
    if w <= 0 || h <= 0 {
        return false;
    }
    KEPT.with(|kept| {
        let mut kept = kept.borrow_mut();
        let key = (ch, w, h);
        if kept.len() >= MOST_KEPT && !kept.contains_key(&key) {
            kept.clear();
        }
        let mask = kept.entry(key).or_insert_with(|| {
            // A source that answers with the wrong size is treated as having no glyph.
            source(ch, w, h, baseline).filter(|m| m.len() == (w * h) as usize)
        });
        let Some(mask) = mask else {
            return false;
        };
        for (y, row) in mask.chunks_exact(w as usize).enumerate() {
            fill_runs(target, (box_.x, box_.y + y as i32), row, color);
        }
        true
    })
}

/// Fills each run of lit pixels in one row of a mask, starting at `(x, y)`.
fn fill_runs(target: &mut Surface<'_>, (x, y): (i32, i32), row: &[u8], color: u32) {
    let mut start = None;
    for (i, &on) in row.iter().chain(std::iter::once(&0)).enumerate() {
        match (start, on != 0) {
            (None, true) => start = Some(i as i32),
            (Some(from), false) => {
                target.fill(Rect::new(x + from, y, i as i32 - from, 1), color);
                start = None;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source with a glyph only for Ж: a frame around its box.
    fn frames(ch: char, w: i32, h: i32, _baseline: i32) -> Option<Vec<u8>> {
        (ch == 'Ж').then(|| {
            (0..w * h)
                .map(|i| u8::from(i % w == 0 || i % w == w - 1 || i / w == 0 || i / w == h - 1))
                .collect()
        })
    }

    fn wrong_size(_: char, _: i32, _: i32, _: i32) -> Option<Vec<u8>> {
        Some(vec![1; 3])
    }

    fn surface(px: &mut [u32]) -> Surface<'_> {
        Surface { px, w: 20, h: 20 }
    }

    #[test]
    fn a_glyph_is_drawn_from_the_source_once_and_kept() {
        set_source(Some(frames));
        let mut px = vec![0u32; 400];
        let r = Rect::new(2, 3, 5, 9);
        assert!(draw(&mut surface(&mut px), r, 8, 'Ж', 7));
        assert_eq!(px[3 * 20 + 2], 7, "the box's corner");
        assert_eq!(px[5 * 20 + 4], 0, "inside the frame");
        assert_eq!(px[3 * 20 + 6], 7, "the top edge's last pixel");
        assert_eq!(px[3 * 20 + 7], 0, "and none after it");
        assert!(draw(&mut surface(&mut px), r, 8, 'Ж', 7));
        assert_eq!(kept(), (1, 45));
        assert!(!draw(&mut surface(&mut px), r, 8, 'ש', 7), "no glyph");
        assert_eq!(kept().0, 2, "and the answer is kept too");
        set_source(None);
        assert_eq!(kept(), (0, 0));
        assert!(!draw(&mut surface(&mut px), r, 8, 'Ж', 7), "no source");
    }

    #[test]
    fn what_is_kept_stays_bounded() {
        set_source(Some(frames));
        let mut px = vec![0u32; 400];
        for h in 1..=MOST_KEPT as i32 + 10 {
            draw(&mut surface(&mut px), Rect::new(0, 0, 3, h), 0, 'Ж', 1);
        }
        assert!(kept().0 <= MOST_KEPT);
        set_source(Some(wrong_size));
        assert!(!draw(
            &mut surface(&mut px),
            Rect::new(0, 0, 5, 5),
            4,
            'Ж',
            1
        ));
        set_source(None);
    }
}
