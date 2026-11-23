//! Drawing the install review. What the plugin asks for comes first, under its name and id,
//! and is never hidden: the plugin's own description is cut to a few lines, and if the window
//! is still too small for everything, Install is withheld and the review says why.

use crate::app::App;
use crate::script_draw::{wrap, ID_SCRIPTS, LINE, WIDTH};
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{button, default_button, draw_text, MENU_LEVEL};

/// The most lines of the plugin's own description shown.
const MOST_DESCRIPTION_LINES: usize = 3;
/// Said instead of offering Install when the review does not fit.
const TOO_SMALL: &str = "The window is too small to show everything this plugin asks for. \
                         Make it larger to install it.";
/// Units above the first row (the title) and below the last (the buttons).
const TOP: i32 = 32;
const BOTTOM: i32 = 40;

/// One review line, wrapped to the panel, and the whole line it came from.
type Row = (Vec<String>, String);

impl App {
    /// The install review: everything the plugin is and asks for, then Install or not.
    pub(crate) fn draw_plugin_review(&mut self, sf: &mut Surface<'_>, dirty: &mut Vec<Rect>) {
        let Some(review) = self.plugins.review.as_ref().map(|p| p.review().clone()) else {
            return;
        };
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let screen = Rect::new(0, 0, self.layout.bottom.w, self.layout.bottom.bottom());
        let w = (WIDTH * s).min(screen.w);
        let inner = w - 32 * s;
        let rows = fitting_rows(&review, inner, ts, screen.h / s);
        let count: usize = rows.iter().map(|(r, _)| r.len()).sum();
        let wanted = (TOP + BOTTOM + count as i32 * LINE) * s;
        let fits = wanted <= screen.h;
        let h = wanted.min(screen.h);
        let panel = Rect::new((screen.w - w) / 2, (screen.h - h).max(0) / 2, w, h);
        self.ui.focus.collect(true);
        let back = self.ui.enter_level(MENU_LEVEL);
        pikslar_ui::chrome::popup(sf, panel, s);
        let left = panel.x + 16 * s;
        draw_text(
            sf,
            left,
            panel.y + 12 * s,
            "Install this plugin?",
            theme::text(),
            ts,
        );
        let row = panel.bottom() - 34 * s;
        let end = match fits {
            true => row,
            false => row - self.draw_too_small(sf, Rect::new(left, row, inner, 0)),
        };
        self.draw_review_rows(sf, &rows, Rect::new(left, panel.y + TOP * s, inner, 0), end);
        self.draw_review_buttons(sf, left, row, fits);
        self.ui.focus.collect(false);
        self.ui.enter_level(back);
        dirty.push(panel.inset(-s));
    }

    /// The rows from `at`'s top down, each only if all of it ends above `end`.
    fn draw_review_rows(&mut self, sf: &mut Surface<'_>, rows: &[Row], at: Rect, end: i32) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let mut y = at.y;
        for (wrapped, whole) in rows {
            if y + wrapped.len() as i32 * LINE * s > end {
                return;
            }
            let top = y;
            for part in wrapped {
                draw_text(sf, at.x, y, part, theme::text(), ts);
                y += LINE * s;
            }
            let r = Rect::new(at.x, top, at.w, y - top);
            self.ui.name_rect(r, &format!("Review: {whole}"));
        }
    }

    /// Why Install is not offered, just above `at`'s top; gives back the height it took.
    fn draw_too_small(&mut self, sf: &mut Surface<'_>, at: Rect) -> i32 {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let lines = wrap(TOO_SMALL, at.w, ts);
        let h = (lines.len() as i32 * LINE + 4) * s;
        let top = at.y - h;
        for (n, line) in lines.iter().enumerate() {
            draw_text(
                sf,
                at.x,
                top + n as i32 * LINE * s,
                line,
                theme::accent(),
                ts,
            );
        }
        self.ui
            .name_rect(Rect::new(at.x, top, at.w, h - 4 * s), TOO_SMALL);
        h
    }

    fn draw_review_buttons(&mut self, sf: &mut Surface<'_>, left: i32, row: i32, fits: bool) {
        let s = self.ui.scale;
        let install = Rect::new(left, row, 90 * s, 24 * s);
        let deny_x = if fits { install.right() + 8 * s } else { left };
        let deny = Rect::new(deny_x, row, 120 * s, 24 * s);
        if button(
            &mut self.ui,
            sf,
            ID_SCRIPTS + 11,
            deny,
            "Do not install",
            false,
        ) {
            self.defer(App::deny_plugin);
        }
        // Drawn after Do not install, so that is the first Tab stop: Tab from nowhere declines
        // before it can install.
        if fits && default_button(&mut self.ui, sf, ID_SCRIPTS + 10, install, "Install") {
            self.defer(App::approve_plugin);
        }
    }
}

/// The review's rows with as much of the description as `room` units of height allow: up to
/// [`MOST_DESCRIPTION_LINES`] lines, fewer, down to one, when the panel would not fit. What the
/// plugin asks for is never cut.
fn fitting_rows(review: &pikslar_plugin::InstallReview, w: i32, ts: i32, room: i32) -> Vec<Row> {
    let mut rows = Vec::new();
    for most in (1..=MOST_DESCRIPTION_LINES).rev() {
        rows = review_rows(review, w, ts, most);
        let count: usize = rows.iter().map(|(r, _)| r.len()).sum();
        if TOP + BOTTOM + count as i32 * LINE <= room {
            break;
        }
    }
    rows
}

/// The review's lines wrapped to `w`, the plugin's own description cut to `most_description`
/// lines.
fn review_rows(
    review: &pikslar_plugin::InstallReview,
    w: i32,
    ts: i32,
    most_description: usize,
) -> Vec<Row> {
    let description = review.description_line();
    review
        .lines()
        .into_iter()
        .map(|line| {
            let mut wrapped = wrap(&line, w, ts);
            if Some(&line) == description.as_ref() && wrapped.len() > most_description {
                wrapped.truncate(most_description);
                if let Some(last) = wrapped.last_mut() {
                    let room = last.chars().count().saturating_sub(3);
                    *last = format!("{}...", last.chars().take(room).collect::<String>());
                }
            }
            (wrapped, line)
        })
        .collect()
}
