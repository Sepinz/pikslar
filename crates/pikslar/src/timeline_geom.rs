//! Where the timeline's parts sit in its body (docs/ui-redesign.md 3.5), in one pure function so
//! that the tests can check every width and scale without drawing anything.
//!
//! From the top: a rule, the tags row, the single line (transport, frame chips, timing, the
//! unfold chevron), the cel rows when open, and the status row (messages and Rewind). The layer
//! names have their own column to the left of the cels, whose width the user sets; the frame
//! chips start where the cels do, so the two line up.

use crate::panel_timeline::CELL;
use crate::workspace::model::{NAMES_DEFAULT, NAMES_RANGE};
use pikslar_render::Rect;
use pikslar_ui::splitter::GUTTER;

/// Room at the body's left and right edges.
pub(crate) const MARGIN: i32 = 8;
/// The tags row's top and height.
const TAGS_Y: i32 = 3;
pub(crate) const TAGS_H: i32 = 16;
/// The single line's top and height.
pub(crate) const LINE_Y: i32 = 22;
pub(crate) const LINE_H: i32 = 28;
/// A line button, and the distance between neighbours.
const BUTTON: i32 = 24;
const PITCH: i32 = BUTTON + 2;
/// The timing control: readout, less and more.
pub(crate) const TIMING_W: i32 = 186;
/// The status row, and how far its bottom is above the body's.
pub(crate) const STATUS_H: i32 = 20;
const STATUS_FOOT: i32 = 26;
/// The frames the chip strip keeps at least, whatever width the names take.
const STRIP_LEAST: i32 = 2 * CELL;
/// The transport's seven ids: play, previous and next left of the strip; new, duplicate, remove
/// and onion right of it.
const LEFT_BUTTONS: usize = 3;
const BUTTONS: usize = 7;
const RIGHT_BUTTONS: i32 = (BUTTONS - LEFT_BUTTONS) as i32;
/// The right end of the line: the chevron, its gap, the timing and the buttons with their gaps.
const RIGHT_FIXED: i32 =
    MARGIN + BUTTON + 4 + TIMING_W + MARGIN + RIGHT_BUTTONS * PITCH - 2 + MARGIN;

/// The parts of the timeline body, in screen pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Geom {
    /// The layer-name column, as tall as the line; the cel rows below share its x range.
    pub names: Rect,
    /// The names column's drawn width and the most it could be, in units.
    pub names_units: i32,
    pub names_most: i32,
    /// The vertical gutter right of the names, as tall as the line.
    pub gutter: Rect,
    /// The seven transport buttons, in id order.
    pub transport: [Rect; BUTTONS],
    pub tags_label: Rect,
    /// The tags row, over the frame strip's columns.
    pub tags: Rect,
    pub strip: Rect,
    pub timing: Rect,
    pub chevron: Rect,
    pub status: Rect,
}

/// The names column's width in units for a body `width` units wide: what was asked, kept to its
/// range and to what leaves the strip its least frames.
pub(crate) fn names_width(width: i32, asked: i32) -> i32 {
    let room = width - 2 * MARGIN - GUTTER - 4 - RIGHT_FIXED - STRIP_LEAST;
    let most = NAMES_RANGE.1.min(room).max(NAMES_RANGE.0);
    asked.clamp(NAMES_RANGE.0, most)
}

/// The widest the names column can be in a body `width` units wide.
pub(crate) fn names_most(width: i32) -> i32 {
    names_width(width, NAMES_RANGE.1)
}

impl Geom {
    /// The parts of the body `panel` at `s` pixels a unit, with a names column `asked` units wide.
    pub fn of(panel: Rect, s: i32, asked: i32) -> Geom {
        let names_w = names_width(panel.w / s.max(1), asked);
        let u = |n: i32| n * s;
        let line = Rect::new(panel.x, panel.y + u(LINE_Y), panel.w, u(LINE_H));
        let button_y = line.y + u((LINE_H - BUTTON) / 2);
        let names = Rect::new(panel.x + u(MARGIN), line.y, u(names_w), line.h);
        let gutter = Rect::new(names.right(), line.y, u(GUTTER), line.h);
        let chevron = Rect::new(
            panel.right() - u(MARGIN + BUTTON),
            button_y,
            u(BUTTON),
            u(BUTTON),
        );
        let timing = Rect::new(
            chevron.x - u(4 + TIMING_W),
            button_y,
            u(TIMING_W),
            u(BUTTON),
        );
        let right_x = timing.x - u(MARGIN + RIGHT_BUTTONS * PITCH - 2);
        let strip_x = gutter.right() + u(4);
        let strip_w = (right_x - u(MARGIN) - strip_x).max(0);
        let strip = Rect::new(strip_x, line.y, strip_w, line.h);
        let mut transport = [Rect::default(); BUTTONS];
        for (i, slot) in transport.iter_mut().enumerate() {
            let x = if i < LEFT_BUTTONS {
                names.x + u(i as i32 * PITCH)
            } else {
                right_x + u((i - LEFT_BUTTONS) as i32 * PITCH)
            };
            *slot = Rect::new(x, button_y, u(BUTTON), u(BUTTON));
        }
        let tags_y = panel.y + u(TAGS_Y);
        Geom {
            names,
            names_units: names_w,
            names_most: names_most(panel.w / s.max(1)),
            gutter,
            transport,
            tags_label: Rect::new(names.x, tags_y, u(names_w), u(TAGS_H)),
            tags: Rect::new(strip.x, tags_y, strip.w, u(TAGS_H)),
            strip,
            timing,
            chevron,
            status: Rect::new(
                panel.x + u(MARGIN),
                panel.bottom() - u(STATUS_FOOT),
                (panel.w - u(2 * MARGIN)).max(0),
                u(STATUS_H),
            ),
        }
    }

    /// Every rectangle that holds a control of the line, the tags row or the status row.
    #[cfg(test)]
    pub fn parts(&self) -> Vec<Rect> {
        let mut all = self.transport.to_vec();
        all.extend([
            self.strip,
            self.timing,
            self.chevron,
            self.tags,
            self.status,
        ]);
        all
    }
}

/// The column width kept when nothing was chosen.
pub(crate) const DEFAULT_NAMES: i32 = NAMES_DEFAULT;

#[cfg(test)]
#[path = "timeline_geom_tests.rs"]
mod tests;
