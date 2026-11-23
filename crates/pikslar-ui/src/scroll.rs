//! A scroll bar beside a list: wheel elsewhere, drag or click here.

use crate::theme;
use crate::widgets::Ui;
use pikslar_render::{Rect, Surface};

/// The shortest thumb, in interface units, so it can always be caught.
const MIN_THUMB: i32 = 6;

/// The largest first row, so the last rows fill the list.
fn last_first(shown: usize, total: usize) -> usize {
    total.saturating_sub(shown)
}

/// The thumb for rows `first..first + shown` of `total`, inside `track`. The thumb reaches the
/// bottom of the track exactly when the last row is shown.
pub fn thumb(track: Rect, first: usize, shown: usize, total: usize, min_h: i32) -> Rect {
    let total = total.max(1);
    let h = (track.h as i64 * shown.min(total) as i64 / total as i64) as i32;
    let h = h.max(min_h).min(track.h);
    let most = last_first(shown, total);
    let top = if most == 0 {
        0
    } else {
        ((track.h - h) as i64 * first.min(most) as i64 / most as i64) as i32
    };
    Rect::new(track.x, track.y + top, track.w, h)
}

/// The first row for a thumb of height `h` whose top is at `top`, rounded to the nearest row.
pub fn first_at(track: Rect, h: i32, top: i32, shown: usize, total: usize) -> usize {
    let most = last_first(shown, total) as i64;
    let range = (track.h - h) as i64;
    if range <= 0 || most == 0 {
        return 0;
    }
    let along = (top - track.y).clamp(0, range as i32) as i64;
    ((along * most + range / 2) / range) as usize
}

/// A scroll bar over `hit`, drawn as a thin line in its middle. Returns the first row to show
/// when a drag or click asks for a different one. A click on the track moves the thumb's
/// middle there; a drag on the thumb keeps hold of the place it was caught.
pub fn scroll_bar(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    hit: Rect,
    first: usize,
    shown: usize,
    total: usize,
) -> Option<usize> {
    if total <= shown {
        return None;
    }
    let s = ui.scale;
    let state = ui.interact(id, hit);
    ui.describe(
        crate::role::Role::Slider,
        crate::role::State::value(first as i32),
    );
    let line = Rect::new(hit.x + (hit.w - 2 * s) / 2, hit.y, 2 * s, hit.h);
    let bar = thumb(line, first, shown, total, MIN_THUMB * s);
    if state.started {
        let y = ui.mouse.1;
        ui.grab = if bar.contains(bar.x, y) {
            y - bar.y
        } else {
            bar.h / 2
        };
    }
    // In high contrast track and thumb share one ink: the track narrows to a hairline and the
    // thumb keeps its width, so they differ by shape.
    let hc = crate::icons::high_contrast();
    let track = if hc {
        Rect::new(line.x + (line.w - s) / 2, line.y, s, line.h)
    } else {
        line
    };
    sf.fill(track, theme::rule());
    let wide = state.hover || state.held || hc;
    let (x, w, ink) = if wide {
        (line.x - s, line.w + 2 * s, theme::text())
    } else {
        (line.x, line.w, theme::muted())
    };
    sf.fill(Rect::new(x, bar.y, w, bar.h), ink);
    if !state.held {
        return None;
    }
    let wanted = first_at(line, bar.h, ui.mouse.1 - ui.grab, shown, total);
    (wanted != first).then_some(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACK: Rect = Rect::new(0, 100, 4, 200);

    #[test]
    fn the_thumb_spans_the_track_as_the_rows_span_the_list() {
        assert_eq!(thumb(TRACK, 0, 10, 40, 6), Rect::new(0, 100, 4, 50));
        assert_eq!(thumb(TRACK, 30, 10, 40, 6), Rect::new(0, 250, 4, 50));
        assert_eq!(thumb(TRACK, 15, 10, 40, 6).y, 175);
        assert_eq!(thumb(TRACK, 0, 1, 1000, 6).h, 6, "never too short to catch");
        assert_eq!(thumb(TRACK, 0, 50, 40, 6), Rect::new(0, 100, 4, 200));
        assert_eq!(
            thumb(TRACK, 99, 10, 40, 6).y,
            250,
            "a first row past the end is the end"
        );
    }

    #[test]
    fn where_the_thumb_is_dragged_gives_the_rows_back() {
        for first in 0..=30 {
            let bar = thumb(TRACK, first, 10, 40, 6);
            assert_eq!(first_at(TRACK, bar.h, bar.y, 10, 40), first);
        }
        assert_eq!(first_at(TRACK, 50, -500, 10, 40), 0);
        assert_eq!(first_at(TRACK, 50, 9_000, 10, 40), 30);
        assert_eq!(first_at(TRACK, 200, 120, 10, 10), 0);
    }

    fn pass(ui: &mut Ui, first: usize) -> Option<usize> {
        let mut px = vec![0u32; 20 * 400];
        let mut sf = Surface {
            px: &mut px,
            w: 20,
            h: 400,
        };
        ui.begin();
        let out = scroll_bar(ui, &mut sf, 9, Rect::new(0, 100, 8, 200), first, 10, 40);
        ui.end();
        out
    }

    fn at(y: i32) -> Ui {
        let mut ui = Ui::default();
        ui.scale = 1;
        ui.mouse = (4, y);
        ui
    }

    #[test]
    fn dragging_the_thumb_keeps_hold_where_it_was_caught() {
        let mut ui = at(110);
        ui.pressed = true;
        ui.down = true;
        assert_eq!(
            pass(&mut ui, 0),
            None,
            "a press without moving changes nothing"
        );
        ui.mouse.1 = 160;
        assert_eq!(pass(&mut ui, 0), Some(10));
        ui.mouse.1 = 9_000;
        assert_eq!(pass(&mut ui, 10), Some(30));
        ui.down = false;
        ui.released = true;
        pass(&mut ui, 30);
        ui.mouse.1 = 110;
        assert_eq!(pass(&mut ui, 30), None, "let go, it no longer follows");
    }

    #[test]
    fn a_click_on_the_track_brings_the_thumb_there() {
        let mut ui = at(290);
        ui.pressed = true;
        ui.down = true;
        assert_eq!(pass(&mut ui, 0), Some(30));
        let mut ui = at(200);
        ui.pressed = true;
        ui.down = true;
        assert_eq!(pass(&mut ui, 0), Some(15));
    }

    #[test]
    fn a_list_that_fits_has_no_bar() {
        let mut ui = at(150);
        ui.pressed = true;
        ui.down = true;
        let mut px = vec![0u32; 20 * 400];
        let mut sf = Surface {
            px: &mut px,
            w: 20,
            h: 400,
        };
        ui.begin();
        let hit = Rect::new(0, 100, 8, 200);
        assert_eq!(scroll_bar(&mut ui, &mut sf, 9, hit, 0, 10, 10), None);
        assert_eq!(ui.widget_at(4, 150), 0);
        assert!(px.iter().all(|&p| p == 0));
    }
}

#[cfg(test)]
mod look_tests {
    use super::*;
    use crate::theme::{SystemColors, Theme};

    /// In high contrast the track and the thumb share one ink, so the thumb is the wider of the
    /// two (final review, defect 4).
    #[test]
    fn in_high_contrast_the_thumb_is_wider_than_its_track() {
        theme::set(Theme::high_contrast(&SystemColors::BLACK));
        crate::icons::set_high_contrast(true);
        let mut ui = Ui::new();
        ui.scale = 1;
        let mut px = vec![theme::panel(); 20 * 100];
        let mut sf = Surface {
            px: &mut px,
            w: 20,
            h: 100,
        };
        ui.begin();
        scroll_bar(&mut ui, &mut sf, 1, Rect::new(4, 0, 12, 100), 0, 2, 8);
        let inked = |y: usize| {
            (0..20)
                .filter(|&x| px[y * 20 + x] != theme::panel())
                .count()
        };
        assert!(
            inked(5) > inked(90),
            "thumb {} track {}",
            inked(5),
            inked(90)
        );
        assert!(inked(90) > 0, "the track shows");
        theme::set(Theme::NORMAL);
        crate::icons::set_high_contrast(false);
    }
}
