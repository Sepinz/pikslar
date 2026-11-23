//! Pixel Lint: its results, the marks on the canvas and the fixes it offers.

use crate::app::App;
use pikslar_core::{lint_layer, Cel, ColorMode, Finding, LintKind};
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{button, cap_height, draw_text, text_width};
use std::collections::HashSet;

pub(crate) const ID_LINT: u32 = 400;
pub const HEADER_HEIGHT: i32 = 24;
const ROW_H: i32 = 22;
/// What the results below the rows need. On a short window they wait for more room.
const DETAIL_HEIGHT: i32 = 66;
/// A finding is too small to judge below this zoom.
const FOCUS_ZOOM: i32 = 8;
/// About 60 ms of checking. Larger canvases are checked when the artist asks.
const LIVE_LINT_CELLS: usize = 1 << 20;

const KINDS: [(LintKind, &str); 4] = [
    (LintKind::Stray, "Stray pixels"),
    (LintKind::Double, "Doubled steps"),
    (LintKind::Jag, "Jagged runs"),
    (LintKind::NearColors, "Near colors"),
];

/// Which change, layer and frame a set of results describes.
type LintKey = ((u64, u64), usize, usize);

/// What the prototype knows about the art right now.
#[derive(Default)]
pub struct LintState {
    pub enabled: bool,
    pub findings: Vec<Finding>,
    /// Issues the artist decided are intentional, for this session.
    ignored: HashSet<IgnoreKey>,
    /// What the results were computed for: revision, layer and frame.
    computed_for: Option<LintKey>,
    pub focus: Option<Finding>,
}

fn kind_id(kind: LintKind) -> u8 {
    match kind {
        LintKind::Stray => 0,
        LintKind::Double => 1,
        LintKind::Jag => 2,
        LintKind::NearColors => 3,
    }
}

/// A finding's kind, its layer, and where it is in the cel's own image, so an ignored finding
/// follows the drawing when the cel moves and hides nothing on other layers.
type IgnoreKey = (u8, u32, i32, i32);

fn ignore_key(f: &Finding, layer: u32, cel: &Cel) -> IgnoreKey {
    let (x, y) = cel.position();
    (
        kind_id(f.kind),
        layer,
        f.from.0 - i32::from(x),
        f.from.1 - i32::from(y),
    )
}

/// Two short lines, sized for the panel's bitmap font.
fn explanation(kind: LintKind) -> [&'static str; 2] {
    match kind {
        LintKind::Stray => ["A lone pixel with", "nothing touching it"],
        LintKind::Double => ["An extra corner", "thickens this step"],
        LintKind::Jag => ["This run breaks a", "steady step rhythm"],
        LintKind::NearColors => ["Two nearly equal", "colors are in use"],
    }
}

impl App {
    pub fn toggle_lint(&mut self) {
        self.lint.enabled = !self.lint.enabled;
        self.lint.focus = None;
        self.lint.computed_for = None;
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    /// A revision alone is not enough, because a step that came to nothing gives its number back.
    fn lint_key(&self) -> LintKey {
        let editor = &self.editor;
        (
            (editor.revision(), editor.revision_rewinds()),
            editor.layer,
            editor.frame,
        )
    }

    /// Reruns the checks when the art changed and no drag is in progress.
    pub fn refresh_lint(&mut self) {
        let key = self.lint_key();
        if !self.lint.enabled
            || self.lint.computed_for == Some(key)
            || self.gesture != crate::app::Gesture::None
            || self.editor.is_floating()
        {
            return;
        }
        // Checking a huge canvas after every stroke would be felt, so it waits to be asked.
        if self.lint.computed_for.is_some() && self.lint_waits_for_request() {
            if self.lint.focus.take().is_some() {
                self.full_canvas = true;
                self.ui_dirty = true;
            }
            return;
        }
        let doc = &self.editor.doc();
        let layer = &doc.layers()[self.editor.layer];
        let cel = layer.cel(self.editor.frame);
        let colors = doc.colors.clone();
        let palette = layer.mode == ColorMode::Palette;
        let shown = |c: u32| if palette { colors.color(c) } else { c };
        // A reference is a picture to draw from, a group has no pixels and a tilemap layer holds
        // cells: none is art to correct.
        let mut found = lint_layer(layer, self.editor.frame, (doc.width, doc.height), shown);
        found.retain(|f| !self.lint.ignored.contains(&ignore_key(f, layer.id(), cel)));
        self.lint.findings = found;
        self.lint.computed_for = Some(key);
        if self
            .lint
            .focus
            .is_some_and(|f| !self.lint.findings.contains(&f))
        {
            self.lint.focus = None;
        }
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    fn lint_waits_for_request(&self) -> bool {
        self.editor.doc().width * self.editor.doc().height > LIVE_LINT_CELLS
    }

    /// True when the art has moved on from the results on show.
    fn lint_is_stale(&self) -> bool {
        let key = self.lint_key();
        self.lint.computed_for.is_some_and(|done| done != key)
            && self.gesture == crate::app::Gesture::None
            && !self.editor.is_floating()
            && self.lint_waits_for_request()
    }

    pub fn reset_lint(&mut self) {
        self.lint.findings.clear();
        self.lint.ignored.clear();
        self.lint.computed_for = None;
        self.lint.focus = None;
    }

    /// Pans only when the finding is off screen or near an edge, so the art stays still.
    fn reveal(&mut self, f: Finding) {
        let c = self.layout.canvas;
        let zoomed_in = self.view.num >= self.view.den * FOCUS_ZOOM;
        if !zoomed_in {
            self.view.num = FOCUS_ZOOM;
            self.view.den = 1;
        }
        let (num, den) = (self.view.num, self.view.den);
        let (sx, sy) = (
            self.view.x + (f.x * num).div_euclid(den),
            self.view.y + (f.y * num).div_euclid(den),
        );
        let margin = c.w.min(c.h) / 8;
        let comfortable = sx >= c.x + margin
            && sx <= c.right() - margin
            && sy >= c.y + margin
            && sy <= c.bottom() - margin;
        if !zoomed_in || !comfortable {
            self.view.x = c.x + c.w / 2 - (f.x * num).div_euclid(den);
            self.view.y = c.y + c.h / 2 - (f.y * num).div_euclid(den);
        }
    }

    /// Moves to the next finding of a kind and brings it into view.
    fn focus_next(&mut self, kind: LintKind) {
        let of_kind: Vec<Finding> = self
            .lint
            .findings
            .iter()
            .copied()
            .filter(|f| f.kind == kind)
            .collect();
        let at = self
            .lint
            .focus
            .and_then(|f| of_kind.iter().position(|o| *o == f));
        let next = at.map_or(0, |i| (i + 1) % of_kind.len().max(1));
        self.lint.focus = of_kind.get(next).copied();
        if let Some(f) = self.lint.focus {
            self.reveal(f);
        }
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    fn fix_focused(&mut self) {
        // Applying a floating selection could move the art out from under this finding.
        if self.editor.is_floating() {
            return;
        }
        if let Some(f) = self.lint.focus.take() {
            if !self.settle() {
                return;
            }
            self.run(pikslar_core::DrawCmd::EraseCell { x: f.x, y: f.y });
            self.notify("Fixed. Undo brings it back".to_string());
        }
    }

    fn ignore_focused(&mut self) {
        if let Some(f) = self.lint.focus.take() {
            let layer = &self.editor.doc().layers()[self.editor.layer];
            let key = ignore_key(&f, layer.id(), layer.cel(self.editor.frame));
            self.lint.ignored.insert(key);
            self.lint.findings.retain(|o| *o != f);
            self.full_canvas = true;
            self.ui_dirty = true;
        }
    }

    pub fn draw_lint_panel(&mut self, sf: &mut Surface<'_>, area: Rect, folded: bool) {
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        let toggle = Rect::new(area.right() - 44 * s, area.y, 44 * s, 22 * s);
        let header = Rect::new(area.x, area.y, toggle.x - area.x - 4 * s, 22 * s);
        self.section_header(sf, crate::panel_right::Section::Lint, header, folded);
        let label = if self.lint.enabled { "On" } else { "Off" };
        if button(
            &mut self.ui,
            sf,
            ID_LINT + 20,
            toggle,
            label,
            self.lint.enabled,
        ) {
            self.defer(App::toggle_lint);
        }
        if !self.lint.enabled || folded {
            return;
        }
        let mut y = area.y + 26 * s;
        for (index, (kind, label)) in KINDS.iter().enumerate() {
            if y + ROW_H * s > area.bottom() {
                return;
            }
            self.draw_lint_row(sf, area, y, index as u32, *kind, label);
            y += ROW_H * s;
        }
        if y + DETAIL_HEIGHT * s > area.bottom() {
            return;
        }
        if self.lint_is_stale() {
            let recheck = Rect::new(area.x, y + 4 * s, area.w, 24 * s);
            if button(&mut self.ui, sf, ID_LINT + 12, recheck, "Check again", true) {
                self.lint.computed_for = None;
                self.ui_dirty = true;
            }
            return;
        }
        match self.lint.focus {
            Some(f) => self.draw_lint_detail(sf, area, y + 4 * s, f.kind),
            None if self.lint.findings.is_empty() => {
                draw_text(sf, area.x, y + 6 * s, "Looking clean", theme::accent(), ts);
            }
            None => draw_text(
                sf,
                area.x,
                y + 6 * s,
                "Pick a row to look",
                theme::muted(),
                ts,
            ),
        }
    }

    fn draw_lint_row(
        &mut self,
        sf: &mut Surface<'_>,
        area: Rect,
        y: i32,
        index: u32,
        kind: LintKind,
        label: &str,
    ) {
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        let row = Rect::new(area.x, y, area.w, (ROW_H - 2) * s);
        let count = self.lint.findings.iter().filter(|f| f.kind == kind).count();
        let state = self.ui.interact(ID_LINT + index, row);
        if state.hover && count > 0 {
            sf.fill(row, theme::raised());
        }
        if self.lint.focus.is_some_and(|f| f.kind == kind) {
            sf.fill(Rect::new(row.x, row.y, 2 * s, row.h), theme::accent());
        }
        let color = if count > 0 {
            theme::text()
        } else {
            theme::muted()
        };
        let text_y = row.y + (row.h - cap_height() * ts) / 2;
        draw_text(sf, row.x + 6 * s, text_y, label, color, ts);
        let number = count.to_string();
        let number_x = row.right() - text_width(&number, ts) - 6 * s;
        draw_text(sf, number_x, text_y, &number, color, ts);
        if state.clicked && count > 0 {
            self.defer(move |a| a.focus_next(kind));
        }
    }

    fn draw_lint_detail(&mut self, sf: &mut Surface<'_>, area: Rect, y: i32, kind: LintKind) {
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        let [first, second] = explanation(kind);
        draw_text(sf, area.x, y, first, theme::text(), ts);
        draw_text(sf, area.x, y + 18 * s, second, theme::text(), ts);
        let buttons = y + 38 * s;
        let half = (area.w - 4 * s) / 2;
        let ignore = Rect::new(area.x, buttons, half, 24 * s);
        if button(&mut self.ui, sf, ID_LINT + 10, ignore, "Ignore", false) {
            self.defer(App::ignore_focused);
        }
        if matches!(kind, LintKind::Stray | LintKind::Double) && !self.editor.is_floating() {
            let fix = Rect::new(area.x + half + 4 * s, buttons, half, 24 * s);
            if button(&mut self.ui, sf, ID_LINT + 11, fix, "Fix", false) {
                self.defer(App::fix_focused);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::{FloatingCmd, SelectionCmd};

    fn checked_app(size: usize) -> App {
        let mut app = App::launch(None, Some(size), None);
        app.resized(800, 600, 1);
        app.toggle_lint();
        app.refresh_lint();
        app
    }

    #[test]
    fn fix_waits_while_a_selection_floats_because_the_art_may_move() {
        let mut app = checked_app(64);
        app.lint.focus = Some(Finding {
            kind: LintKind::Stray,
            x: 3,
            y: 3,
            w: 1,
            h: 1,
            from: (3, 3),
        });
        let shape = pikslar_core::Mask::rect(2, 2, 4, 4);
        app.editor
            .apply(SelectionCmd::Select {
                shape: Box::new(shape),
                op: pikslar_core::SelectOp::Replace,
            })
            .expect("select");
        app.editor
            .apply(FloatingCmd::Lift { copy: false })
            .expect("lift");
        app.fix_focused();
        assert!(app.editor.is_floating());
    }

    fn strays(app: &App) -> Vec<(i32, i32)> {
        let found = app.lint.findings.iter();
        found
            .filter(|f| f.kind == LintKind::Stray)
            .map(|f| (f.x, f.y))
            .collect()
    }

    fn speck(app: &mut App, x: i32, y: i32) {
        let tool = pikslar_core::Tool::Pencil;
        let path = vec![(x, y)];
        app.editor
            .apply(pikslar_core::DrawCmd::Stroke { tool, path })
            .expect("draw");
        app.refresh_lint();
    }

    #[test]
    fn an_ignored_finding_follows_its_cel_and_hides_nothing_on_other_layers() {
        let mut app = checked_app(64);
        speck(&mut app, 10, 10);
        assert_eq!(strays(&app), vec![(10, 10)]);
        app.lint.focus = app.lint.findings.first().copied();
        app.ignore_focused();
        app.refresh_lint();
        assert!(strays(&app).is_empty());

        let layer = app.editor.layer_id(0).expect("layer");
        let (dx, dy) = (5, 3);
        let step = pikslar_core::CelCmd::Move {
            layer,
            frame: 0,
            dx,
            dy,
        };
        app.editor.apply(step).expect("move");
        app.editor
            .apply(pikslar_core::StepCmd::EndDrag)
            .expect("end");
        app.refresh_lint();
        assert!(
            strays(&app).is_empty(),
            "still ignored where the cel took it"
        );

        let mode = ColorMode::Rgba;
        app.editor
            .apply(pikslar_core::LayerCmd::Add { at: 1, mode })
            .expect("layer");
        // The same spot in its own image as the ignored one, on another layer.
        speck(&mut app, 10, 10);
        assert_eq!(
            strays(&app),
            vec![(10, 10)],
            "another layer's speck is its own"
        );
    }

    fn move_cel(app: &mut App, dx: i32) {
        let layer = app.editor.layer_id(0).expect("layer");
        let step = pikslar_core::CelCmd::Move {
            layer,
            frame: 0,
            dx,
            dy: 0,
        };
        app.editor.apply(step).expect("move");
        app.editor
            .apply(pikslar_core::StepCmd::EndDrag)
            .expect("end");
        app.refresh_lint();
    }

    fn jags(app: &App) -> Vec<Finding> {
        let found = app.lint.findings.iter().copied();
        found.filter(|f| f.kind == LintKind::Jag).collect()
    }

    #[test]
    fn an_ignored_finding_across_the_edge_stays_ignored_after_a_move() {
        let mut app = checked_app(64);
        // A stepped line with one long run, drawn on the canvas and then moved so the long run
        // crosses the left edge.
        let mut x = 10;
        for (row, run) in [2, 2, 2, 6, 2, 2, 2].into_iter().enumerate() {
            for _ in 0..run {
                speck(&mut app, x, 4 + row as i32);
                x += 1;
            }
        }
        move_cel(&mut app, -18);
        let found = jags(&app);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].x == 0 && found[0].from.0 < 0, "{found:?}");
        app.lint.focus = Some(found[0]);
        app.ignore_focused();
        move_cel(&mut app, 1);
        assert!(jags(&app).is_empty(), "still ignored one pixel on");
    }

    #[test]
    fn small_canvases_are_rechecked_after_every_change() {
        let mut app = checked_app(64);
        app.run(pikslar_core::FrameCmd::Retime {
            from: 0,
            durations: vec![321],
        });
        app.refresh_lint();
        assert!(!app.lint_is_stale());
        assert_eq!(
            app.lint.computed_for.map(|k| k.0 .0),
            Some(app.editor.revision())
        );
    }

    #[test]
    fn huge_canvases_wait_to_be_asked_and_drop_the_stale_focus() {
        let mut app = checked_app(2048);
        let first = app.lint.computed_for;
        assert!(first.is_some());
        app.lint.focus = Some(Finding {
            kind: LintKind::Stray,
            x: 1,
            y: 1,
            w: 1,
            h: 1,
            from: (1, 1),
        });
        app.run(pikslar_core::FrameCmd::Retime {
            from: 0,
            durations: vec![321],
        });
        app.refresh_lint();
        assert_eq!(app.lint.computed_for, first);
        assert!(app.lint_is_stale());
        assert_eq!(app.lint.focus, None);
        app.lint.computed_for = None;
        app.refresh_lint();
        assert!(!app.lint_is_stale());
    }
}
