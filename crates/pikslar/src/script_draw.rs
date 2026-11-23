//! Drawing the Scripts panel. It covers what is under it, as the other dialogs do, so a click
//! cannot reach the panels while it is open.

use crate::app::App;
use crate::script_panel::RunState;
use pikslar_platform::Window;
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{button, draw_text, text_width, MENU_LEVEL};

pub(crate) const ID_SCRIPTS: u32 = 950;
pub(crate) const WIDTH: i32 = 520;
/// Output lines shown at once: the newest ones.
const SHOWN_LINES: usize = 12;
const STATUS_LINES: usize = 3;
/// A text line in units: tall enough that a descender never touches the capital below it.
pub(crate) const LINE: i32 = 16;
const BUTTONS: [&str; 5] = [
    "Open script...",
    "Run script",
    "Stop script",
    "Clear output",
    "Close scripts",
];

impl App {
    pub(crate) fn draw_script_panel(
        &mut self,
        sf: &mut Surface<'_>,
        window: Window,
        dirty: &mut Vec<Rect>,
    ) {
        if !self.scripts.open {
            return;
        }
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let screen = Rect::new(0, 0, self.layout.bottom.w, self.layout.bottom.bottom());
        let lines = (STATUS_LINES + SHOWN_LINES) as i32;
        let (w, h) = (WIDTH * s, (112 + lines * LINE) * s);
        let w = w.min(screen.w);
        let panel = Rect::new(
            (screen.w - w) / 2,
            (screen.h - h).max(0) / 2,
            w,
            h.min(screen.h),
        );
        let back = self.ui.enter_level(MENU_LEVEL);
        pikslar_ui::chrome::popup(sf, panel, s);
        let left = panel.x + 16 * s;
        let inner = panel.w - 32 * s;
        let name = self.scripts.loaded.as_ref().map(|r| r.name.as_str());
        let title = panel_title(name, inner, ts);
        draw_text(sf, left, panel.y + 12 * s, &title, theme::text(), ts);
        self.draw_script_buttons(sf, Rect::new(left, panel.y + 32 * s, inner, 24 * s), window);
        let said = limits_line(self.script_limits);
        draw_text(sf, left, panel.y + 66 * s, &said, theme::muted(), ts);
        self.ui
            .name_rect(Rect::new(left, panel.y + 66 * s, inner, LINE * s), &said);
        let status_top = panel.y + 84 * s;
        self.draw_script_status(
            sf,
            Rect::new(left, status_top, inner, STATUS_LINES as i32 * LINE * s),
        );
        let out_top = status_top + (STATUS_LINES as i32 * LINE + 8) * s;
        self.draw_script_output(
            sf,
            Rect::new(left, out_top, inner, SHOWN_LINES as i32 * LINE * s),
        );
        self.ui.enter_level(back);
        dirty.push(panel.inset(-s));
    }

    fn draw_script_buttons(&mut self, sf: &mut Surface<'_>, row: Rect, window: Window) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let running = self.scripts.state != RunState::Idle;
        let mut x = row.x;
        for (n, label) in BUTTONS.iter().enumerate() {
            let r = Rect::new(x, row.y, text_width(label, ts) + 16 * s, row.h);
            x = r.right() + 6 * s;
            if n == 2 {
                self.scripts.stop_at = r;
            }
            let lit = (n == 1 && running) || (n == 2 && running);
            if !button(&mut self.ui, sf, ID_SCRIPTS + n as u32, r, label, lit) || running {
                continue;
            }
            match n {
                0 => self.defer(move |a| a.choose_script(window)),
                1 => self.defer(|a| match a.scripts.loaded.is_some() {
                    true => a.start_script(None),
                    false => a.notify("Open a script first".to_string()),
                }),
                3 => self.defer(App::clear_script_output),
                4 => self.defer(App::close_script_panel),
                _ => {}
            }
        }
    }

    fn draw_script_status(&mut self, sf: &mut Surface<'_>, area: Rect) {
        let Some(status) = self.scripts.status.clone() else {
            return;
        };
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let color = if status.failed {
            theme::danger()
        } else {
            theme::text()
        };
        let lines = wrap(&status.text, area.w, ts);
        for (n, line) in lines.iter().take(STATUS_LINES).enumerate() {
            draw_text(sf, area.x, area.y + n as i32 * LINE * s, line, color, ts);
        }
        self.ui
            .name_rect(area, &format!("Script status: {}", status.text));
    }

    fn draw_script_output(&mut self, sf: &mut Surface<'_>, area: Rect) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        pikslar_ui::chrome::well(sf, area.inset(-2 * s), s, theme::panel(), false);
        let output = &self.scripts.output;
        let first = output.len().saturating_sub(SHOWN_LINES);
        for (n, line) in output[first..].iter().enumerate() {
            let top = area.y + n as i32 * LINE * s;
            draw_text(
                sf,
                area.x,
                top,
                &fitted(line, area.w, ts),
                theme::muted(),
                ts,
            );
            if self.ui.naming() {
                let r = Rect::new(area.x, top, area.w, LINE * s);
                self.ui.name_rect(r, &format!("Output: {line}"));
            }
        }
    }
}

/// `text` in lines no wider than `w`, broken at spaces where it can be.
pub(crate) fn wrap(text: &str, w: i32, ts: i32) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split(' ') {
        let Some(last) = lines.last_mut() else {
            break;
        };
        let joined = if last.is_empty() {
            word.to_string()
        } else {
            format!("{last} {word}")
        };
        if text_width(&joined, ts) <= w || last.is_empty() {
            *last = joined;
        } else {
            lines.push(word.to_string());
        }
    }
    lines.into_iter().map(|l| fitted(&l, w, ts)).collect()
}

/// The panel's title: the loaded script's name (a file name, up to 255 characters) shown and
/// cut to the width through [`fitted`], as every other string in the panel is.
fn panel_title(name: Option<&str>, w: i32, ts: i32) -> String {
    match name {
        Some(name) => fitted(&format!("Scripts: {name}"), w, ts),
        None => "Scripts: none open".to_string(),
    }
}

/// The panel's limits line: fuel, the script's memory, the document growth a run may add,
/// and call depth. Memory and growth are separate settings, so both are named.
pub(crate) fn limits_line(l: pikslar_bind::ScriptLimits) -> String {
    format!(
        "Limits: {} fuel, {} MB memory, {} MB document growth, depth {}",
        l.fuel,
        l.memory >> 20,
        l.growth >> 20,
        l.call_depth
    )
}

/// `text`, cut short to fit `w`.
pub(crate) fn fitted(text: &str, w: i32, ts: i32) -> String {
    let out = pikslar_plugin::hidden::shown(text);
    crate::panel_bottom::fit_to_width(&out, w, ts).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first and last rows that carry ink when `text` is drawn at `top`.
    fn ink_rows(text: &str, top: i32, ts: i32) -> (i32, i32) {
        let (w, h) = (120, 120);
        let mut px = vec![0u32; (w * h) as usize];
        let mut sf = Surface { px: &mut px, w, h };
        draw_text(&mut sf, 4, top, text, 0xffff_ffff, ts);
        let rows: Vec<i32> = (0..h)
            .filter(|&y| (0..w).any(|x| px[(y * w + x) as usize] != 0))
            .collect();
        (rows[0], rows[rows.len() - 1])
    }

    #[test]
    fn lines_do_not_touch_even_with_descenders_over_capitals() {
        for s in [1, 2, 3] {
            let ts = pikslar_ui::font::text_scale(s);
            let (_, last_of_first) = ink_rows("Tg", 10 * s, ts);
            let (first_of_second, _) = ink_rows("gT", 10 * s + LINE * s, ts);
            assert!(
                last_of_first < first_of_second,
                "scale {s}: ink of one line reaches row {last_of_first}, the next starts at {first_of_second}"
            );
        }
    }

    #[test]
    fn the_limits_line_names_memory_and_growth_apart() {
        let line = limits_line(pikslar_bind::ScriptLimits::DEFAULT);
        assert_eq!(
            line,
            "Limits: 50000000 fuel, 16 MB memory, 256 MB document growth, depth 128"
        );
        // The widest it can be still fits the panel at the default text scale.
        let widest = pikslar_bind::ScriptLimits {
            fuel: 2_000_000_000,
            memory: 256 << 20,
            call_depth: 256,
            growth: 2048 << 20,
        };
        assert!(text_width(&limits_line(widest), 1) <= WIDTH - 32);
    }

    #[test]
    fn long_text_wraps_at_spaces_and_long_words_are_cut() {
        let lines = wrap("one two three four", text_width("one two", 1), 1);
        assert_eq!(lines, ["one two", "three", "four"]);
        let cut = wrap("abcdefghij", text_width("abc...", 1), 1);
        assert_eq!(cut, ["abc..."]);
        assert_eq!(fitted("a\tb\u{7}", 1000, 1), "a b");
    }

    #[test]
    fn the_title_shows_a_script_name_fitted_like_every_other_string() {
        let name = format!("a\u{202e}b{}", "c".repeat(255));
        let title = panel_title(Some(&name), 300, 2);
        assert!(title.starts_with("Scripts: ab"), "{title}");
        assert!(!title.contains('\u{202e}'));
        assert!(text_width(&title, 2) <= 300);
        assert_eq!(panel_title(None, 300, 2), "Scripts: none open");
    }

    #[test]
    fn fitting_and_wrapping_a_very_long_line_take_linear_time() {
        let long = "a".repeat(64 << 10);
        let start = std::time::Instant::now();
        let cut = fitted(&long, 500, 2);
        let lines = wrap(&format!("{long} b"), 500, 2);
        let took = start.elapsed();
        assert!(!cut.is_empty() && text_width(&cut, 2) <= 500);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| text_width(l, 2) <= 500));
        assert!(took < std::time::Duration::from_millis(500), "{took:?}");
    }
}
