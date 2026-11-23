//! A tooltip: a control's hint, shown beside the pointer after it rests there.

use crate::font::{cap_height, draw_text, text_width};
use crate::theme;
use pikslar_render::{Rect, Surface};

/// The widest a tooltip grows before its words wrap, in interface units.
pub const TOOLTIP_WIDTH: i32 = 220;
const PAD: i32 = 6;
const LINE_GAP: i32 = 3;
/// How far below and right of the pointer it starts, in interface units.
const BELOW: i32 = 18;

/// Breaks `text` into lines no wider than `max_w` at text scale `ts`. A word too long for a
/// line has a line of its own.
pub fn wrap(text: &str, max_w: i32, ts: i32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if text_width(&format!("{line} {word}"), ts) <= max_w => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// Where the tooltip for `lines` goes when the pointer rests at `at`.
pub fn tooltip_rect(scale: i32, at: (i32, i32), lines: &[String], bounds: Rect) -> Rect {
    let ts = crate::font::text_scale(scale.max(1));
    let widest = lines.iter().map(|l| text_width(l, ts)).max().unwrap_or(0);
    let w = widest + 2 * PAD * scale;
    let h = lines.len() as i32 * (cap_height() * ts + LINE_GAP * scale) + 2 * PAD * scale
        - LINE_GAP * scale;
    let below = at.1 + BELOW * scale;
    let y = if below + h > bounds.bottom() {
        at.1 - 4 * scale - h
    } else {
        below
    };
    let x = (at.0 + BELOW * scale / 2).min(bounds.right() - w);
    Rect::new(
        x.max(bounds.x),
        y.clamp(bounds.y, (bounds.bottom() - h).max(bounds.y)),
        w,
        h,
    )
}

/// Draws the lines in `rect`, from [`tooltip_rect`].
pub fn draw_tooltip(sf: &mut Surface<'_>, scale: i32, rect: Rect, lines: &[String]) {
    let ts = crate::font::text_scale(scale.max(1));
    crate::chrome::popup(sf, rect, scale);
    let step = cap_height() * ts + LINE_GAP * scale;
    for (n, line) in lines.iter().enumerate() {
        let y = rect.y + PAD * scale + n as i32 * step;
        draw_text(sf, rect.x + PAD * scale, y, line, theme::text(), ts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_wrap_to_the_width_and_a_long_word_keeps_a_line() {
        let ts = 2;
        let one = text_width("a", ts);
        let lines = wrap("Click to draw on this layer  Up, Down", 12 * one, ts);
        assert!(
            lines.iter().all(|l| text_width(l, ts) <= 12 * one),
            "{lines:?}"
        );
        assert_eq!(lines.join(" "), "Click to draw on this layer Up, Down");
        assert!(lines.len() > 1);
        assert_eq!(wrap("short", 1000, ts), vec!["short"]);
        assert_eq!(wrap("unbreakable", one, ts), vec!["unbreakable"]);
        assert!(wrap("   ", 100, ts).is_empty());
    }

    #[test]
    fn a_tooltip_sits_below_the_pointer_and_inside_the_window() {
        let bounds = Rect::new(0, 0, 800, 600);
        let lines = vec!["Show this frame for longer".to_string()];
        let r = tooltip_rect(1, (100, 100), &lines, bounds);
        assert!(r.y > 100 && r.x > 100, "{r:?}");
        let corner = tooltip_rect(1, (795, 595), &lines, bounds);
        assert!(
            corner.right() <= 800 && corner.bottom() <= 600,
            "{corner:?}"
        );
        assert!(corner.bottom() <= 595, "it does not cover the point");
        assert_eq!(tooltip_rect(2, (0, 0), &lines, bounds).h, 2 * r.h);
    }
}
