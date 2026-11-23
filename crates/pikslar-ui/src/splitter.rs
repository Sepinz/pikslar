//! A splitter: the gutter between two slots, or between a dock area and the canvas, that sets
//! the size of the pane beside it (docs/ui-redesign.md 2.5, 3.3, 3.7).
//!
//! Dragged with the pointer, or focused and moved with the arrows (a step, four with Shift),
//! Home and End (least and largest size); Enter asks to fold the pane.

use crate::focus::WidgetKey;
use crate::role::{Role, State};
use crate::theme;
use crate::widgets::Ui;
use pikslar_render::{Rect, Surface};

/// The gutter's thickness, in units.
pub const GUTTER: i32 = 4;
/// The thickness the pointer finds it in, in units, where no other control lies.
pub const REACH: i32 = 8;

/// Which way the gutter moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// A standing gutter between side-by-side panes, moved left and right.
    Across,
    /// A lying gutter between stacked panes, moved up and down.
    Down,
}

/// The size the splitter sets, in units, and its limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub size: i32,
    pub least: i32,
    pub most: i32,
    /// Units an arrow moves it; Shift moves four steps.
    pub step: i32,
    /// The pane lies after the gutter (right of it, or below), so moving the gutter forward
    /// makes it smaller.
    pub after: bool,
}

/// What a splitter asks for this pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitChange {
    /// The pane's new size, in units, inside its limits.
    Resize(i32),
    /// Fold the pane, or open it.
    Fold,
}

impl Span {
    fn clamp(self, size: i32) -> i32 {
        size.clamp(self.least, self.most.max(self.least))
    }

    /// The size after the gutter moved `by` units forward.
    fn moved(self, by: i32) -> i32 {
        self.clamp(self.size + if self.after { -by } else { by })
    }

    /// What a key asks of a splitter on `axis`. Arrows across the axis do nothing.
    fn key(self, axis: Axis, key: WidgetKey, shift: bool) -> Option<SplitChange> {
        let step = if shift { 4 * self.step } else { self.step };
        let size = match (axis, key) {
            (Axis::Across, WidgetKey::Left) | (Axis::Down, WidgetKey::Up) => self.moved(-step),
            (Axis::Across, WidgetKey::Right) | (Axis::Down, WidgetKey::Down) => self.moved(step),
            (_, WidgetKey::Home) => self.clamp(self.least),
            (_, WidgetKey::End) => self.clamp(self.most),
            (_, WidgetKey::Enter) => return Some(SplitChange::Fold),
            _ => return None,
        };
        (size != self.size).then_some(SplitChange::Resize(size))
    }
}

/// The part of the gutter's axis a point is at.
fn along(axis: Axis, (x, y): (i32, i32)) -> i32 {
    match axis {
        Axis::Across => x,
        Axis::Down => y,
    }
}

/// Where the pointer finds a gutter: [`REACH`] units thick, centred, unless a control drawn
/// before it this pass lies under the pointer, which then keeps the extra.
fn reach(ui: &Ui, gutter: Rect, axis: Axis) -> Rect {
    if ui.claimed(ui.mouse.0, ui.mouse.1) {
        return gutter;
    }
    let s = ui.scale.max(1);
    let extra = ((REACH - GUTTER) / 2) * s;
    match axis {
        Axis::Across => Rect::new(gutter.x - extra, gutter.y, gutter.w + 2 * extra, gutter.h),
        Axis::Down => Rect::new(gutter.x, gutter.y - extra, gutter.w, gutter.h + 2 * extra),
    }
}

/// Draws the splitter whose gutter is `gutter` and handles the pointer and keys. Draw it
/// after the panes beside it, so their controls keep the pointer near its edges.
pub fn splitter(
    ui: &mut Ui,
    sf: &mut Surface<'_>,
    id: u32,
    gutter: Rect,
    axis: Axis,
    span: Span,
) -> Option<SplitChange> {
    let s = ui.scale.max(1);
    let hit = reach(ui, gutter, axis);
    let state = ui.interact(id, hit);
    ui.focus.register_keys(id);
    ui.describe(Role::Splitter, State::value(span.size));
    let start = along(axis, (gutter.x, gutter.y));
    if state.started {
        ui.grab = along(axis, ui.mouse) - start;
    }
    let dragged = (state.held && ui.down)
        .then(|| (along(axis, ui.mouse) - ui.grab - start).div_euclid(s))
        .map(|by| span.moved(by))
        .filter(|&size| size != span.size)
        .map(SplitChange::Resize);
    let keyed = ui
        .focus
        .take_press(id)
        .and_then(|p| span.key(axis, p.key, p.shift));
    draw(sf, gutter, axis, s, state.hover || state.held);
    ui.ring(sf, id, gutter);
    dragged.or(keyed)
}

/// The gutter in Base with a grip of three dots in its middle, wider and lighter while the
/// pointer is on it.
fn draw(sf: &mut Surface<'_>, gutter: Rect, axis: Axis, s: i32, live: bool) {
    sf.fill(gutter, theme::base());
    // In high contrast Base is the window's own colour, so the boundary becomes a line down
    // the gutter's middle (docs/ui-redesign.md 2.4), with the grip on it.
    if crate::icons::high_contrast() {
        let line = match axis {
            Axis::Across => Rect::new(gutter.x + (gutter.w - s) / 2, gutter.y, s, gutter.h),
            Axis::Down => Rect::new(gutter.x, gutter.y + (gutter.h - s) / 2, gutter.w, s),
        };
        sf.fill(line, theme::rule());
    }
    let (thick, ink) = if live {
        (2 * s, theme::edge_hover())
    } else {
        (s, theme::edge())
    };
    // On the high contrast line the grip stands out by its width.
    let thick = thick + 2 * s * i32::from(crate::icons::high_contrast());
    let (long, gap) = (2 * s, 2 * s);
    let run = 3 * long + 2 * gap;
    for n in 0..3 {
        let dot = match axis {
            Axis::Across => Rect::new(
                gutter.x + (gutter.w - thick) / 2,
                gutter.y + (gutter.h - run) / 2 + n * (long + gap),
                thick,
                long,
            ),
            Axis::Down => Rect::new(
                gutter.x + (gutter.w - run) / 2 + n * (long + gap),
                gutter.y + (gutter.h - thick) / 2,
                long,
                thick,
            ),
        };
        sf.fill(dot, ink);
    }
}

#[cfg(test)]
#[path = "splitter_tests.rs"]
mod tests;
