//! The workspace solver (section 3.8 of docs/ui-redesign.md): a pure function from an
//! arrangement and a window to the rectangle of every region and slot. It allocates nothing.
//!
//! Order: the fixed bars first; the bottom dock clamped so that the middle keeps its least
//! height; inside each side dock, slots that do not fit fold in the documented order, then
//! Color compacts, then the area becomes a rail; the side docks' widths clamped so the canvas
//! keeps 280 pixels, folding the left area to its rail, then the right, when that fails.
//!
//! The result carries the arrangement as solved (`effective`): sizes clamped, automatic folds
//! applied. Solving that again gives the same result. It is not saved: a window made larger
//! again unfolds what the solver folded.

use super::metrics::Metrics;
use super::model::{Area, Panel, Side, Slot, Workspace, MOST_SLOTS, RAIL};
#[cfg(test)]
use super::presets::TIMELINE_LINE;
use pikslar_render::Rect;
use std::cmp::Reverse;

/// The redesign's fixed bars (units); see [`Metrics::LIGHTBOX`].
pub(crate) const MENU: i32 = Metrics::LIGHTBOX.menu;
/// The height the tool strip needs in one column, and in two (units), in the redesign.
pub(crate) const ONE_COLUMN_NEED: i32 = Metrics::LIGHTBOX.one_column_need;
pub(crate) const TWO_COLUMN_NEED: i32 = Metrics::LIGHTBOX.two_column_need;
/// The canvas is never smaller than this, in screen pixels, while the window allows it.
pub(crate) const MIN_CANVAS: i32 = 280;

/// The client area in pixels and the interface size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Window {
    pub w: i32,
    pub h: i32,
    pub scale: i32,
}

/// Where one slot sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SlotRect {
    pub rect: Rect,
    /// False when folded to its tab strip.
    pub open: bool,
    pub panel: Option<Panel>,
}

/// Where one dock area sits, and its slots unless it is a rail or empty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AreaRect {
    pub rect: Rect,
    pub rail: bool,
    slots: [SlotRect; MOST_SLOTS],
    count: u8,
}

impl AreaRect {
    pub(crate) fn slots(&self) -> &[SlotRect] {
        &self.slots[..usize::from(self.count)]
    }
}

/// Every region of the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Solved {
    pub menu: Rect,
    pub context: Rect,
    pub tools: Rect,
    /// 1 or 2.
    pub tool_columns: i32,
    pub canvas: Rect,
    pub status: Rect,
    areas: [AreaRect; 3],
    /// The arrangement as solved; see the module notes.
    pub effective: Workspace,
}

impl Solved {
    pub(crate) fn area(&self, side: Side) -> &AreaRect {
        &self.areas[side.index()]
    }

    /// The regions that tile the window: bars, tool strip, canvas and the three areas.
    pub(crate) fn regions(&self) -> [Rect; 8] {
        [
            self.menu,
            self.context,
            self.tools,
            self.canvas,
            self.status,
            self.area(Side::Right).rect,
            self.area(Side::Bottom).rect,
            self.area(Side::Left).rect,
        ]
    }
}

/// The direction slots stack in: down the side docks, across the bottom dock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    Down,
    Across,
}

/// The measures of one solve: the interface size and the metrics.
#[derive(Clone, Copy)]
struct Units<'a> {
    s: i32,
    m: &'a Metrics,
}

/// A slot's least length along `axis` with the redesign's metrics, in units.
#[cfg(test)]
fn least(slot: &Slot, compact: bool, axis: Axis) -> i32 {
    least_in(&Metrics::LIGHTBOX, slot, compact, axis)
}

/// The length a closed slot takes along `axis` when drawn, in units.
fn shown_in(m: &Metrics, slot: &Slot, axis: Axis) -> i32 {
    match (slot.active_panel(), axis) {
        (Some(p), Axis::Down) => (m.shown)(p),
        _ => least_in(m, slot, false, axis),
    }
}

/// A slot's least length along `axis`, in units.
fn least_in(m: &Metrics, slot: &Slot, compact: bool, axis: Axis) -> i32 {
    match (slot.active_panel(), slot.open, axis) {
        (None, _, _) => m.strip,
        (Some(p), false, Axis::Down) => (m.folded)(p),
        (Some(_), false, Axis::Across) => m.strip,
        (Some(p), true, Axis::Across) => (m.least_across)(p),
        (Some(Panel::Color), true, Axis::Down) if compact => m.color_compact,
        (Some(p), true, Axis::Down) => (m.least)(p),
    }
}

fn holds_color(area: &Area) -> bool {
    area.slots().iter().any(|s| s.tabs.contains(Panel::Color))
}

/// The next slot to fold: the open one whose panel folds first, the later slot on a tie.
fn next_fold(area: &Area) -> Option<usize> {
    area.slots()
        .iter()
        .enumerate()
        .filter(|(_, s)| s.open)
        .filter_map(|(i, s)| Some((s.active_panel()?.fold_rank()?, Reverse(i))))
        .min()
        .map(|(_, Reverse(i))| i)
}

/// Folds slots, then compacts Color (side docks only), until the least needs fit `length`
/// pixels; none when even that is not enough and the area must become a rail. Once it fits,
/// slots folded on the way open again, last folded first, while they still fit: compacting
/// Color may have made the room, and nothing folds that need not.
fn fold_to_fit(
    area: Area,
    compact: bool,
    length: i32,
    u: Units,
    axis: Axis,
) -> Option<(Area, bool)> {
    let need = |a: &Area, c: bool| -> i32 {
        a.slots()
            .iter()
            .map(|sl| least_in(u.m, sl, c, axis))
            .sum::<i32>()
            * u.s
    };
    let with_open = |a: Area, i: usize, open: bool| {
        a.with_slot(
            i,
            Slot {
                open,
                ..a.slots()[i]
            },
        )
    };
    let (mut a, mut c) = (area, compact);
    let mut folded = Vec::new();
    while need(&a, c) > length {
        if let Some(i) = next_fold(&a) {
            a = with_open(a, i, false);
            folded.push(i);
        } else if axis == Axis::Down && !c && holds_color(&a) {
            c = true;
        } else {
            return None;
        }
    }
    for &i in folded.iter().rev() {
        let opened = with_open(a, i, true);
        if need(&opened, c) <= length {
            a = opened;
        }
    }
    Some((a, c))
}

/// The slot that takes the rest: the last open one without a size, else the last open one,
/// else the last.
fn taker(slots: &[Slot]) -> usize {
    slots
        .iter()
        .rposition(|s| s.open && s.size == 0)
        .or_else(|| slots.iter().rposition(|s| s.open))
        .unwrap_or(slots.len().saturating_sub(1))
}

/// Each slot's length in pixels along the area, and the area with sizes as used. The least
/// needs must fit (see [`fold_to_fit`]).
fn lengths(area: Area, compact: bool, length: i32, u: Units, axis: Axis) -> ([i32; 3], Area) {
    let (s, m) = (u.s, u.m);
    let slots = area.slots();
    let t = taker(slots);
    let mut units = [0; MOST_SLOTS];
    for (i, sl) in slots.iter().enumerate() {
        let lo = least_in(m, sl, compact, axis);
        units[i] = match (i == t, sl.open, sl.size) {
            (true, _, _) => 0,
            (false, false, _) => shown_in(m, sl, axis),
            (false, true, 0) => lo,
            // A compacted Color keeps only what it draws; the rest goes to the taker.
            (false, true, _) if compact && sl.active_panel() == Some(Panel::Color) => lo,
            (false, true, n) => n.max(lo),
        };
    }
    let taker_least = slots.get(t).map_or(0, |sl| least_in(m, sl, compact, axis));
    let mut over = (units.iter().sum::<i32>() + taker_least) * s - length;
    let mut used = area;
    for i in (0..slots.len()).rev() {
        let sl = slots[i];
        if over > 0 && i != t && sl.open && sl.size > 0 {
            let cut = ((over + s - 1) / s).min(units[i] - least_in(m, &sl, compact, axis));
            units[i] -= cut;
            over -= cut * s;
        }
        if i != t && sl.open && sl.size > 0 {
            used = used.with_slot(
                i,
                Slot {
                    size: units[i],
                    ..sl
                },
            );
        }
    }
    let mut px = units.map(|n| n * s);
    if t < slots.len() {
        px[t] = (length - px.iter().sum::<i32>()).max(0);
    }
    (px, used)
}

/// Lays an area's slots out in `rect`.
fn place(area: &Area, rect: Rect, compact: bool, u: Units, axis: Axis) -> (AreaRect, Area) {
    let length = match axis {
        Axis::Down => rect.h,
        Axis::Across => rect.w,
    };
    let (px, used) = lengths(*area, compact, length, u, axis);
    let mut out = AreaRect {
        rect,
        ..AreaRect::default()
    };
    let mut at = 0;
    for (i, sl) in used.slots().iter().enumerate() {
        let r = match axis {
            Axis::Down => Rect::new(rect.x, rect.y + at, rect.w, px[i]),
            Axis::Across => Rect::new(rect.x + at, rect.y, px[i], rect.h),
        };
        at += px[i];
        out.slots[i] = SlotRect {
            rect: r,
            open: sl.open,
            panel: sl.active_panel(),
        };
        out.count += 1;
    }
    (out, used)
}

fn rail(area: Area) -> Area {
    area.with_open(false)
}

/// The least height the open side docks need with nothing folded, in units.
fn side_want(ws: &Workspace, m: &Metrics) -> i32 {
    [Side::Left, Side::Right]
        .into_iter()
        .map(|side| ws.area(side))
        .filter(|a| a.open)
        .map(|a| {
            a.slots()
                .iter()
                .map(|sl| least_in(m, sl, ws.color_compact, Axis::Down))
                .sum::<i32>()
        })
        .max()
        .unwrap_or(0)
}

/// The bottom dock's height in pixels and the area as used; `inner` is the height between the
/// context bar and the status bar.
fn bottom(ws: &Workspace, win: Window, inner: i32, m: &Metrics) -> (i32, Area) {
    let (area, s) = (*ws.area(Side::Bottom), win.scale);
    let u = Units { s, m };
    let rail_h = (RAIL * s).min(inner);
    if area.is_empty() {
        return (0, area);
    }
    if !area.open {
        return (rail_h, area);
    }
    let floor = (m.two_column_need * s).max(MIN_CANVAS);
    let lo = area
        .slots()
        .iter()
        .map(|sl| least_in(m, sl, ws.color_compact, Axis::Down))
        .fold(m.timeline_line, i32::max);
    let Some((area, _)) = fold_to_fit(area, ws.color_compact, win.w, u, Axis::Across) else {
        return (rail_h, rail(area));
    };
    if inner - lo * s < floor {
        return (rail_h, rail(area));
    }
    let docks = if m.keep_docks { side_want(ws, m) } else { 0 };
    let want = floor.max(m.middle_least * s).max(docks * s);
    let hi = (win.h / 2).min(inner - want) / s;
    let size = area.size.clamp(lo, hi.max(lo));
    (size * s, area.with_size(size))
}

/// A side dock fitted into the middle's height: folded as needed, or a rail.
fn fit_side(area: Area, compact: bool, middle: i32, u: Units) -> (Area, bool) {
    if area.is_empty() || !area.open {
        return (area, compact);
    }
    let own = compact && holds_color(&area);
    match fold_to_fit(area, own, middle, u, Axis::Down) {
        Some((a, c)) => (a, compact || c),
        None => (rail(area), compact),
    }
}

/// A side dock's width in units as asked: nothing when empty, a rail when folded.
fn asked_width(area: &Area, side: Side) -> i32 {
    let (lo, hi) = side.range();
    match (area.is_empty(), area.open) {
        (true, _) => 0,
        (false, false) => RAIL,
        (false, true) => area.size.clamp(lo, hi),
    }
}

/// The side docks' widths, narrowed to their least and then folded to rails, left first each
/// time, until the canvas keeps [`MIN_CANVAS`] pixels of `room`.
fn widths(left: Area, right: Area, room: i32, s: i32) -> (Area, Area) {
    let short = |l: &Area, r: &Area| {
        MIN_CANVAS - (room - (asked_width(l, Side::Left) + asked_width(r, Side::Right)) * s)
    };
    let narrowed = |a: Area, side: Side, deficit: i32| -> Area {
        if deficit <= 0 || a.is_empty() || !a.open {
            return a;
        }
        let lo = side.range().0;
        a.with_size(lo.max(asked_width(&a, side) - (deficit + s - 1) / s))
    };
    let folded = |a: Area, deficit: i32| -> Area {
        if deficit <= 0 || a.is_empty() {
            return a;
        }
        rail(a)
    };
    let l = narrowed(left, Side::Left, short(&left, &right));
    let r = narrowed(right, Side::Right, short(&l, &right));
    if short(&l, &r) <= 0 {
        return (l, r);
    }
    // The left folds; the right narrows again from what it asked, as the rail frees room.
    let l = folded(left, short(&l, &r));
    let r = narrowed(right, Side::Right, short(&l, &right));
    let r = folded(r, short(&l, &r));
    (l, r)
}

/// Solves `ws` for `win` with the redesign's fixed metrics. Test only: the app solves with
/// `layout::today()`, whose strip follows the face.
#[cfg(test)]
pub(crate) fn solve(ws: &Workspace, win: Window) -> Solved {
    solve_with(ws, win, &Metrics::LIGHTBOX)
}

/// Solves `ws` for `win` with metrics `m`.
pub(crate) fn solve_with(ws: &Workspace, win: Window, m: &Metrics) -> Solved {
    let s = win.scale.max(1);
    let win = Window {
        w: win.w.max(0),
        h: win.h.max(0),
        scale: s,
    };
    let u = Units { s, m };
    let menu_h = (m.menu * s).min(win.h);
    let context_h = (m.context * s).min(win.h - menu_h);
    let status_h = (m.status * s).min(win.h - menu_h - context_h);
    let top = menu_h + context_h;
    let inner = win.h - top - status_h;
    let (bottom_h, bottom_area) = bottom(ws, win, inner, m);
    let middle = inner - bottom_h;
    let columns = if middle >= m.one_column_need * s {
        1
    } else {
        2
    };
    let tools_w = (if columns == 1 {
        m.tools_one
    } else {
        m.tools_two
    } * s)
        .min(win.w);
    let (left, compact) = fit_side(*ws.area(Side::Left), ws.color_compact, middle, u);
    let (right, compact) = fit_side(*ws.area(Side::Right), compact, middle, u);
    let (left, right) = widths(left, right, win.w - tools_w, s);
    let effective = ws
        .with_area(Side::Bottom, bottom_area)
        .with_compact(compact);
    let frame = Frame {
        win,
        m,
        top,
        middle,
        bottom_h,
        tools_w,
    };
    frame.layout(effective, left, right, columns)
}

/// The fixed measures of one solve, in pixels.
struct Frame<'a> {
    win: Window,
    m: &'a Metrics,
    top: i32,
    middle: i32,
    bottom_h: i32,
    tools_w: i32,
}

impl Frame<'_> {
    fn layout(&self, ws: Workspace, left: Area, right: Area, columns: i32) -> Solved {
        let (w, h, s) = (self.win.w, self.win.h, self.win.scale);
        let (top, middle) = (self.top, self.middle);
        let left_x = self.tools_w;
        let left_w = (asked_width(&left, Side::Left) * s).min(w - left_x);
        let right_w = (asked_width(&right, Side::Right) * s).min(w - left_x - left_w);
        let canvas_x = left_x + left_w;
        let right_x = w - right_w;
        let side_rect = |x, width| Rect::new(x, top, width, middle);
        let (l_out, left) = self.area_axis(left, side_rect(left_x, left_w), ws, Axis::Down);
        let (r_out, right) = self.area_axis(right, side_rect(right_x, right_w), ws, Axis::Down);
        let bottom_rect = Rect::new(0, top + middle, w, self.bottom_h);
        let bottom = *ws.area(Side::Bottom);
        let (b_out, bottom) = self.area_axis(bottom, bottom_rect, ws, Axis::Across);
        let mut areas = [AreaRect::default(); 3];
        areas[Side::Left.index()] = l_out;
        areas[Side::Right.index()] = r_out;
        areas[Side::Bottom.index()] = b_out;
        let status_y = top + middle + self.bottom_h;
        let menu_h = (self.m.menu * s).min(h);
        Solved {
            menu: Rect::new(0, 0, w, menu_h),
            context: Rect::new(0, menu_h, w, top - menu_h),
            tools: side_rect(0, self.tools_w),
            tool_columns: columns,
            canvas: side_rect(canvas_x, right_x - canvas_x),
            status: Rect::new(0, status_y, w, h - status_y),
            areas,
            effective: ws
                .with_area(Side::Left, left)
                .with_area(Side::Right, right)
                .with_area(Side::Bottom, bottom),
        }
    }

    fn area_axis(&self, area: Area, rect: Rect, ws: Workspace, axis: Axis) -> (AreaRect, Area) {
        if area.is_empty() || !area.open {
            let out = AreaRect {
                rect,
                rail: !area.is_empty(),
                ..AreaRect::default()
            };
            return (out, area);
        }
        let u = Units {
            s: self.win.scale,
            m: self.m,
        };
        place(&area, rect, ws.color_compact && holds_color(&area), u, axis)
    }
}

#[cfg(test)]
#[path = "solve_tests.rs"]
mod tests;
