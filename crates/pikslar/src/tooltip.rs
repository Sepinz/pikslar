//! Tooltips: the hint of the control under a resting pointer, shown beside it.
//!
//! The same hint is always in the status line; the tooltip only brings it to where the eye is.

use crate::app::{App, Gesture};
use pikslar_render::{Rect, Surface};
use pikslar_ui::{draw_tooltip, tooltip_rect, wrap, TOOLTIP_WIDTH};
use std::time::{Duration, Instant};

/// How long the pointer rests before a tooltip shows.
pub(crate) const TOOLTIP_DELAY: Duration = Duration::from_millis(600);
/// How far the pointer may drift, in screen pixels, and still be resting.
const DRIFT: i32 = 3;

/// Where the pointer came to rest, and whether its tooltip is up.
#[derive(Clone, Debug, Default)]
pub(crate) struct Rest {
    at: (i32, i32),
    /// When it came to rest; none once a press or key has used the pointer.
    since: Option<Instant>,
    pub shown: bool,
    /// The hint the tooltip shows, so a different one under the pointer replaces it.
    hint: Option<std::borrow::Cow<'static, str>>,
}

impl App {
    /// The pointer moved. Drifting a little keeps it resting; more starts a new rest.
    pub(crate) fn rest_moved(&mut self, x: i32, y: i32, now: Instant) {
        let (ax, ay) = self.rest.at;
        if self.rest.since.is_some() && (x - ax).abs() <= DRIFT && (y - ay).abs() <= DRIFT {
            return;
        }
        self.hide_tooltip();
        self.rest.at = (x, y);
        self.rest.since = Some(now);
    }

    /// A press or key: the tooltip goes, and comes back only after the pointer moves.
    pub(crate) fn rest_interrupted(&mut self) {
        self.hide_tooltip();
        self.rest.since = None;
    }

    fn hide_tooltip(&mut self) {
        if self.rest.shown {
            self.rest.shown = false;
            // It may have been over the canvas.
            self.full_canvas = true;
            self.ui_dirty = true;
        }
    }

    /// How long until a resting pointer's tooltip is due.
    pub(crate) fn tooltip_wait(&self) -> Option<Duration> {
        let since = self.rest.since.filter(|_| !self.rest.shown)?;
        Some((since + TOOLTIP_DELAY).saturating_duration_since(Instant::now()))
    }

    /// Shows the tooltip once the pointer has rested long enough over a control with a hint.
    pub(crate) fn tick_tooltip(&mut self, now: Instant) {
        let due = self
            .rest
            .since
            .is_some_and(|since| now >= since + TOOLTIP_DELAY);
        let quiet = !self.ui.down && !self.panel_open() && self.gesture == Gesture::None;
        if due && quiet && !self.rest.shown && self.hint.is_some() {
            self.rest.shown = true;
            self.rest.hint.clone_from(&self.hint);
            self.ui_dirty = true;
        }
        if due && !self.rest.shown {
            // Nothing to show here: stop waking for it until the pointer moves.
            self.rest.since = None;
        }
    }

    /// The tooltip's place, while it is up.
    pub(crate) fn tooltip_area(&self) -> Option<(Rect, Vec<String>)> {
        let hint = self.hint.as_deref().filter(|_| self.rest.shown)?;
        let s = self.ui.scale;
        let lines = wrap(hint, TOOLTIP_WIDTH * s, self.ui.text_scale());
        let bounds = Rect::new(0, 0, self.layout.bottom.w, self.layout.bottom.bottom());
        Some((tooltip_rect(s, self.rest.at, &lines, bounds), lines))
    }

    /// Drawn last, over everything. If the control under the pointer is not the one the
    /// tooltip was for, it goes, and the pointer starts resting again.
    pub(crate) fn draw_tooltip(&mut self, sf: &mut Surface<'_>, dirty: &mut Vec<Rect>) {
        if self.rest.shown && self.hint != self.rest.hint {
            self.hide_tooltip();
            self.rest.since = Some(Instant::now());
            return;
        }
        if let Some((rect, lines)) = self.tooltip_area() {
            draw_tooltip(sf, self.ui.scale, rect, &lines);
            dirty.push(rect.inset(-self.ui.scale));
        }
    }
}
