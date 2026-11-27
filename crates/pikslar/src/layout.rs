//! Where each part of the interface sits. All sizes are in interface units.
//!
//! The workspace solver places every region from the arrangement in use; this module gives it
//! the measures of the interface as drawn now (a 32-unit top bar, a 44-unit tool strip, panels
//! under 20-unit tab strips, and the timeline whose cel matrix adds rows).

use crate::panel_fit::{
    layer_details, COMPACT_COLOR, FOLDED_COLOR, FULL_COLOR, LAYER_ROW, PALETTE_ROW,
};
use crate::panel_lint::HEADER_HEIGHT;
use crate::workspace::metrics::Metrics;
use crate::workspace::model::{Panel, Side, Slot, Workspace};
use crate::workspace::solve::{solve_with, AreaRect, Solved, Window};
use pikslar_render::Rect;

const TOP: i32 = 32;
/// The context bar under the top bar: the active tool's settings.
pub(crate) const CONTEXT: i32 = 28;
const SIDE: i32 = 44;
const BOTTOM: i32 = 82;
/// One row of the cel matrix.
const MATRIX_ROW_LEAST: i32 = 16;

/// One row of the cel matrix: 16 units, or as tall as the face needs.
pub fn matrix_row() -> i32 {
    pikslar_ui::fit_h(MATRIX_ROW_LEAST)
}
/// The most matrix rows shown at once; more scroll.
pub const MATRIX_ROWS: i32 = 6;
/// Where the matrix starts below the top of the timeline, and the room it leaves below.
const MATRIX_TOP: i32 = 52;
const MATRIX_GAP: i32 = 4;
/// The canvas is never made shorter than this, in screen pixels, to show matrix rows.
const MIN_CANVAS_H: i32 = 280;
/// Nor the side docks shorter than this, in interface units: below it they start folding.
const MIN_MIDDLE: i32 = 380;
/// The rows the timeline always keeps while the matrix is open and the layers fill them.
pub(crate) const MIN_ROWS: i32 = 2;
/// A slot's tab strip: 20 units, or as tall as the face needs.
pub(crate) fn strip() -> i32 {
    pikslar_ui::tabs::strip_h()
}
/// Room around a side dock panel's body: across each side, and above and below it.
pub(crate) const BODY_X: i32 = 12;
pub(crate) const BODY_Y: i32 = 6;
/// Room on a side dock's canvas edge, where its splitter lies.
pub(crate) const DOCK_EDGE: i32 = pikslar_ui::splitter::GUTTER;

/// A panel body's least height, as drawn now.
fn body_least(p: Panel) -> i32 {
    match p {
        Panel::Color => FULL_COLOR,
        // Its header and one row of swatches (table 3.2); more rows scroll.
        Panel::Palette => 26 + PALETTE_ROW,
        Panel::Layers => 28 + 2 * LAYER_ROW + layer_details(),
        Panel::Lint => HEADER_HEIGHT,
        Panel::Tiles => crate::panel_tiles::body_least(),
        Panel::Timeline => BOTTOM,
        Panel::History | Panel::Preview | Panel::Slices | Panel::Scripts => 40,
    }
}

/// What a folded panel's body keeps: the header it folds to.
fn body_folded(p: Panel) -> i32 {
    match p {
        Panel::Color => FOLDED_COLOR,
        Panel::Palette => 26,
        Panel::Layers => 28 + 2 * LAYER_ROW,
        Panel::Lint => HEADER_HEIGHT,
        Panel::Timeline => BOTTOM,
        _ => 0,
    }
}

/// Whether a folded panel keeps a header under its strip. The Palette's would only repeat the
/// name on its tab, so a folded Palette is its strip alone.
pub(crate) fn folds_to_header(p: Panel) -> bool {
    p != Panel::Palette && body_folded(p) > 0
}

/// A slot around a body this tall: its strip, and the room above and below the body.
fn slot_of(body: i32) -> i32 {
    match body {
        0 => strip(),
        n => strip() + 2 * BODY_Y + n,
    }
}

fn least(p: Panel) -> i32 {
    match p {
        Panel::Timeline => BOTTOM,
        p => slot_of(body_least(p)),
    }
}

fn folded(p: Panel) -> i32 {
    match p {
        Panel::Timeline => BOTTOM,
        p => slot_of(body_folded(p)),
    }
}

/// What a folded slot takes when drawn. The fit above still counts the Palette's header, so the
/// same windows fold it; the room the header would have had goes to the open slots beside it.
fn folded_shown(p: Panel) -> i32 {
    match p {
        Panel::Timeline => BOTTOM,
        p if folds_to_header(p) => folded(p),
        _ => slot_of(0),
    }
}

/// The height the matrix adds below the single line for `rows` rows.
fn grid_for(rows: i32) -> i32 {
    if rows > 0 {
        rows * matrix_row() + MATRIX_GAP
    } else {
        0
    }
}

/// The measures of the interface as drawn now, for the face in use.
pub(crate) fn today() -> Metrics {
    Metrics {
        menu: TOP,
        context: CONTEXT,
        status: 0,
        tools_one: SIDE,
        tools_two: SIDE,
        one_column_need: 0,
        two_column_need: 0,
        middle_least: MIN_MIDDLE,
        keep_docks: false,
        strip: strip(),
        color_compact: strip() + 2 * BODY_Y + COMPACT_COLOR,
        timeline_line: BOTTOM,
        least,
        least_across: Panel::least_across,
        folded,
        shown: folded_shown,
        row: list_row,
    }
}

/// The pitch of a panel's list rows in units: a layer, a row of swatches (see
/// [`Metrics::row`]).
fn list_row(p: Panel) -> i32 {
    match p {
        Panel::Layers => LAYER_ROW,
        Panel::Palette => PALETTE_ROW,
        _ => 0,
    }
}

/// Whether a bottom slot shows a tab strip: all but the timeline alone.
pub(crate) fn bottom_strip(slot: &Slot) -> bool {
    slot.tabs.as_slice() != [Panel::Timeline]
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub top: Rect,
    pub context: Rect,
    pub side: Rect,
    pub right: Rect,
    pub bottom: Rect,
    pub left: Rect,
    pub canvas: Rect,
    /// The cel matrix rows under the timeline, empty while it is folded.
    pub matrix: Rect,
    /// Every region and slot, as the solver placed them.
    pub(crate) solved: Solved,
}

impl Layout {
    /// The layout of Draw's arrangement with the cel matrix folded.
    #[cfg(test)]
    pub fn compute(w: i32, h: i32, s: i32) -> Layout {
        Layout::with_matrix(w, h, s, 0)
    }

    /// The layout of Draw's arrangement with up to `rows` cel matrix rows.
    #[cfg(test)]
    pub fn with_matrix(w: i32, h: i32, s: i32, rows: i32) -> Layout {
        let draw = crate::workspace::presets::default_layout(Default::default());
        Layout::arranged(&draw, (w, h, s), rows)
    }

    /// The layout of `ws` with up to `rows` cel matrix rows, as many as leave the canvas its
    /// least height. The bottom dock's size is the timeline's, with the matrix rows shown.
    ///
    /// The timeline keeps [`MIN_ROWS`] rows of cels at every scale, unless that would turn a
    /// side dock into a rail that would not be one without it.
    pub(crate) fn arranged(ws: &Workspace, size: (i32, i32, i32), rows: i32) -> Layout {
        let guarded = Layout::build(ws, size, rows, MIN_ROWS);
        if guarded.rails() == 0 {
            return guarded;
        }
        let plain = Layout::build(ws, size, rows, 0);
        if plain.rails() < guarded.rails() {
            plain
        } else {
            guarded
        }
    }

    /// How many dock areas are folded to their rails.
    fn rails(&self) -> usize {
        [Side::Left, Side::Right, Side::Bottom]
            .into_iter()
            .filter(|&side| self.solved.area(side).rail)
            .count()
    }

    fn build(ws: &Workspace, (w, h, s): (i32, i32, i32), rows: i32, floor: i32) -> Layout {
        let bottom_area = *ws.area(Side::Bottom);
        let strip = if bottom_area.slots().iter().any(bottom_strip) {
            strip()
        } else {
            0
        };
        let least = MIN_CANVAS_H.max(MIN_MIDDLE * s);
        let room = h - (TOP + CONTEXT) * s - (BOTTOM + strip) * s - MATRIX_GAP * s - least;
        // The canvas yields room to the matrix down to MIN_ROWS rows, at every scale.
        let asked = rows.clamp(0, MATRIX_ROWS);
        let rows = asked.min(room / (matrix_row() * s)).max(asked.min(floor));
        let grid = grid_for(rows);
        let kept = grid_for(asked.min(floor));
        let view = ws.with_area(Side::Bottom, bottom_area.with_size(BOTTOM + strip + grid));
        // A timeline under a tab strip needs the strip as well as its single line.
        let metrics = Metrics {
            timeline_line: BOTTOM + strip + kept,
            ..today()
        };
        let solved = solve_with(&view, Window { w, h, scale: s }, &metrics);
        let bottom = *solved.area(Side::Bottom);
        Layout {
            top: solved.menu,
            context: solved.context,
            side: solved.tools,
            right: solved.area(Side::Right).rect,
            bottom: bottom.rect,
            left: solved.area(Side::Left).rect,
            canvas: solved.canvas,
            matrix: matrix_in(&bottom, &solved.effective, s, rows),
            solved,
        }
    }

    /// How many matrix rows fit.
    pub fn matrix_rows(&self, s: i32) -> i32 {
        self.matrix.h / (matrix_row() * s.max(1))
    }

    pub fn chrome(&self) -> [Rect; 6] {
        [
            self.top,
            self.context,
            self.side,
            self.right,
            self.bottom,
            self.left,
        ]
    }
}

/// The matrix rows under the timeline's frame strip, as many of `rows` as its slot holds.
fn matrix_in(bottom: &AreaRect, ws: &Workspace, s: i32, rows: i32) -> Rect {
    let slots = ws.area(Side::Bottom).slots();
    let found = bottom
        .slots()
        .iter()
        .zip(slots)
        .find(|(r, _)| r.open && r.panel == Some(Panel::Timeline));
    let Some((r, slot)) = found else {
        return Rect::default();
    };
    let top = r.rect.y + if bottom_strip(slot) { strip() * s } else { 0 };
    let room = (r.rect.bottom() - top) / s - BOTTOM - MATRIX_GAP;
    let shown = rows.min(room / matrix_row()).max(0);
    Rect::new(
        r.rect.x,
        top + MATRIX_TOP * s,
        r.rect.w,
        shown * matrix_row() * s,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_tile_the_window_exactly() {
        let l = Layout::compute(1400, 900, 2);
        let area = |r: Rect| r.w * r.h;
        let total: i32 = l.chrome().iter().map(|&r| area(r)).sum::<i32>() + area(l.canvas);
        assert_eq!(total, 1400 * 900);
        assert_eq!(l.canvas.right(), l.right.x);
        assert_eq!(l.canvas.bottom(), l.bottom.y);
    }

    #[test]
    fn the_matrix_grows_the_bottom_panel_but_never_squeezes_the_canvas_too_far() {
        let folded = Layout::compute(1366, 705, 1);
        let open = Layout::with_matrix(1366, 705, 1, 3);
        assert_eq!(open.matrix.h, 3 * matrix_row());
        assert_eq!(open.matrix_rows(1), 3);
        assert_eq!(
            open.bottom.h,
            folded.bottom.h + 3 * matrix_row() + MATRIX_GAP
        );
        assert_eq!(
            open.canvas.h,
            folded.canvas.h - 3 * matrix_row() - MATRIX_GAP
        );
        assert_eq!(open.canvas.bottom(), open.bottom.y);
        assert!(open.matrix.y > open.bottom.y && open.matrix.bottom() < open.bottom.bottom());
        assert_eq!(
            Layout::with_matrix(1366, 705, 1, 40).matrix_rows(1),
            MATRIX_ROWS
        );
        assert_eq!(Layout::with_matrix(1366, 705, 1, 0), folded);
        for s in [1, 2, 3] {
            for h in [400, 561, 600, 700, 900, 1400, 2100] {
                let l = Layout::with_matrix(1000 * s, h, s, 6);
                let kept = l.canvas.h >= MIN_CANVAS_H && l.canvas.h >= MIN_MIDDLE * s;
                let least = l.matrix_rows(s) <= MIN_ROWS;
                assert!(l.matrix.h == 0 || kept || least, "{h} at {s}: {l:?}");
            }
        }
        assert_eq!(
            Layout::with_matrix(1000, 480, 1, 6).matrix.h,
            0,
            "a rail would follow"
        );
        // The context bar's 28 units leave room for two rows here, where three fitted.
        assert_eq!(Layout::with_matrix(1000, 561, 1, 6).matrix_rows(1), 2);
        assert_eq!(Layout::with_matrix(3840, 2050, 3, 6).matrix_rows(3), 6);
    }

    /// The canvas share limits of `docs/interface-plan.md`, in tenths of a percent: folded, and
    /// with the matrix at its most rows. Lowering one needs a written reason. 1366x705 open went
    /// from 520 to 510 when the context bar (28 units) was added; the menu bar of phase 7 gives
    /// 8 units back.
    #[test]
    fn the_canvas_keeps_its_share_of_the_named_screens() {
        let limits = [
            (1366, 705, 620, 510),
            (1920, 1017, 720, 640),
            (2560, 1377, 780, 720),
        ];
        let share = |l: Layout, w: i32, h: i32| {
            (l.canvas.w as i64 * l.canvas.h as i64 * 1000 / (w as i64 * h as i64)) as i32
        };
        for (w, h, folded, open) in limits {
            let f = share(Layout::with_matrix(w, h, 1, 0), w, h);
            let o = share(Layout::with_matrix(w, h, 1, MATRIX_ROWS), w, h);
            assert!(f >= folded, "{w}x{h} folded: {f} under {folded}");
            assert!(o >= open, "{w}x{h} most open: {o} under {open}");
        }
    }

    /// The regions as they were computed before the solver placed them, with Draw's right dock.
    fn by_hand(w: i32, h: i32, s: i32, rows: i32, floor: i32) -> [Rect; 6] {
        let right = crate::workspace::presets::RIGHT_WIDTH * s;
        let (bar, side) = (TOP * s, SIDE * s);
        let top = bar + CONTEXT * s;
        let least = MIN_CANVAS_H.max(MIN_MIDDLE * s);
        let room = h - top - BOTTOM * s - MATRIX_GAP * s - least;
        let rows = rows
            .min(MATRIX_ROWS)
            .min(room / (matrix_row() * s))
            .max(rows.clamp(0, floor));
        let grid = rows * matrix_row() * s;
        let bottom = BOTTOM * s + if rows > 0 { grid + MATRIX_GAP * s } else { 0 };
        let middle = (h - top - bottom).max(0);
        [
            Rect::new(0, 0, w, bar),
            Rect::new(0, bar, w, CONTEXT * s),
            Rect::new(0, top, side, middle),
            Rect::new((w - right).max(0), top, right, middle),
            Rect::new(0, top + middle, w, bottom),
            Rect::new(side, top, (w - side - right).max(0), middle),
        ]
    }

    #[test]
    fn the_solver_places_the_regions_where_they_always_were() {
        for s in 1..=3 {
            for w in (600 * s..=2600 * s).step_by(173) {
                for h in (480 * s..=1500 * s).step_by(97) {
                    for rows in [0, 1, 3, 6, 9] {
                        let l = Layout::with_matrix(w, h, s, rows);
                        let got = [l.top, l.context, l.side, l.right, l.bottom, l.canvas];
                        let plain = by_hand(w, h, s, rows, 0);
                        let kept = by_hand(w, h, s, rows, MIN_ROWS);
                        // The floor gives way where it would fold a side dock to a rail.
                        assert!(got == kept || got == plain, "{w}x{h} at {s}, {rows}");
                    }
                }
            }
        }
    }

    /// Animate keeps its strip, line and two cel rows at 2x, on the screens where the canvas
    /// would otherwise take the room.
    #[test]
    fn the_timeline_keeps_two_cel_rows_at_every_scale() {
        let animate =
            crate::workspace::presets::default_layout(crate::workspace::model::Preset::Animate);
        for (w, h, s) in [
            (1366, 705, 2),
            (1920, 1080, 2),
            (1920, 1003, 2),
            (1366, 705, 1),
            (2560, 1377, 3),
        ] {
            let l = Layout::arranged(&animate, (w, h, s), 3);
            assert!(
                l.matrix_rows(s) >= 2,
                "{w}x{h} at {s}: {} rows",
                l.matrix_rows(s)
            );
            assert!(l.matrix.bottom() <= l.bottom.bottom());
        }
        let one = Layout::arranged(&animate, (1920, 1080, 2), 1);
        assert_eq!(one.matrix_rows(2), 1, "never more rows than layers");
    }

    #[test]
    fn tiny_windows_never_produce_negative_sizes() {
        let l = Layout::compute(40, 30, 2);
        assert!(l.canvas.w >= 0 && l.canvas.h >= 0 && l.side.h >= 0);
    }
}
