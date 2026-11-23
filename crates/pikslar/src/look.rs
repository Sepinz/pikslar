//! Following Windows high contrast: the theme is built from the system colors while it is on,
//! and again whenever Windows says a setting changed.

use crate::app::App;
use pikslar_render::Backdrop;
use pikslar_ui::theme::{self, SystemColors, Theme};

/// The look a test asks for instead of the system's: `Some(colors)` for high contrast with
/// those colors, `None` for the normal look.
pub(crate) type ForcedLook = Option<SystemColors>;

/// Reads `--high-contrast`'s value: `black` or `white` for Windows' classic themes, `off` for the
/// normal look whatever Windows says.
pub(crate) fn parse_look(word: &str) -> Option<ForcedLook> {
    match word.to_ascii_lowercase().as_str() {
        "black" => Some(Some(SystemColors::BLACK)),
        "white" => Some(Some(SystemColors::WHITE)),
        "off" => Some(None),
        _ => None,
    }
}

fn from_system(c: pikslar_platform::SystemColors) -> SystemColors {
    SystemColors {
        window: c.window,
        window_text: c.window_text,
        button_face: c.button_face,
        button_text: c.button_text,
        highlight: c.highlight,
        highlight_text: c.highlight_text,
        gray_text: c.gray_text,
        hot_light: c.hot_light,
    }
}

/// The theme for high contrast colors, or the normal one without.
pub(crate) fn theme_for(colors: Option<SystemColors>) -> Theme {
    colors.map_or(Theme::NORMAL, |c| Theme::high_contrast(&c))
}

/// What canvases are drawn on in `theme` at interface scale `scale`: its surround, its two
/// checker squares, and a one-unit line in the Rule color just outside the document, so the
/// document's edge shows against the surround without covering any of its pixels.
pub(crate) fn backdrop_for(theme: &Theme, scale: i32) -> Backdrop {
    Backdrop {
        surround: theme.get(theme::Token::Surround),
        checker_a: theme.get(theme::Token::CheckerA),
        checker_b: theme.get(theme::Token::CheckerB),
        edge_width: scale.max(1),
        edge: theme.get(theme::Token::Rule),
    }
}

/// Whether `theme` is a dark one, so the title bar Windows draws should be dark too: its
/// header is darker than middle grey.
pub(crate) fn wants_dark_title(theme: &Theme) -> bool {
    let c = theme.get(theme::Token::Header);
    let sum = (c >> 16 & 0xff) + (c >> 8 & 0xff) + (c & 0xff);
    sum < 3 * 128
}

impl App {
    /// Draws with the look a test fixed, or the system's: called at start and whenever Windows
    /// says a setting changed. Redraws everything when the theme changes.
    pub(crate) fn follow_look(&mut self) {
        let colors = match self.forced_look {
            Some(forced) => forced,
            None => pikslar_platform::high_contrast().map(from_system),
        };
        let wanted = theme_for(colors);
        // Icons drop their shades for one colour whenever the theme is high contrast.
        pikslar_ui::icons::set_high_contrast(colors.is_some());
        if wanted != theme::current() {
            theme::set(wanted);
            pikslar_render::set_backdrop(backdrop_for(&wanted, self.ui.scale));
            self.full_canvas = true;
            self.ui_dirty = true;
        }
    }

    /// Asks Windows for a title bar that matches the theme, once each time that changes.
    pub(crate) fn update_title_bar(&mut self, window: pikslar_platform::Window) {
        let dark = wants_dark_title(&theme::current());
        if self.dark_title != Some(dark) {
            window.set_dark_title(dark);
            self.dark_title = Some(dark);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_bar_is_dark_in_the_normal_and_black_looks_and_light_in_white() {
        assert!(wants_dark_title(&Theme::NORMAL));
        assert!(wants_dark_title(&theme_for(Some(SystemColors::BLACK))));
        assert!(!wants_dark_title(&theme_for(Some(SystemColors::WHITE))));
    }

    #[test]
    fn the_title_bar_follows_a_change_of_look_once() {
        let mut app = crate::test_app::app(1);
        let window = pikslar_platform::Window::detached();
        app.update_title_bar(window);
        assert_eq!(app.dark_title, Some(true));
        app.forced_look = Some(Some(SystemColors::WHITE));
        app.follow_look();
        app.update_title_bar(window);
        assert_eq!(app.dark_title, Some(false));
        app.forced_look = Some(None);
        app.follow_look();
        app.update_title_bar(window);
        assert_eq!(app.dark_title, Some(true));
    }

    #[test]
    fn the_option_names_the_classic_themes_or_off() {
        assert_eq!(parse_look("black"), Some(Some(SystemColors::BLACK)));
        assert_eq!(parse_look("White"), Some(Some(SystemColors::WHITE)));
        assert_eq!(parse_look("off"), Some(None));
        assert_eq!(parse_look("pink"), None);
    }

    #[test]
    fn a_forced_look_is_followed_and_redraws_everything() {
        let mut app = crate::test_app::app(1);
        app.forced_look = Some(Some(SystemColors::BLACK));
        app.ui_dirty = false;
        app.follow_look();
        assert_eq!(theme::panel(), 0xff00_0000);
        assert!(
            pikslar_ui::icons::high_contrast(),
            "icons draw in one colour"
        );
        assert!(app.ui_dirty && app.full_canvas);
        app.ui_dirty = false;
        app.follow_look();
        assert!(!app.ui_dirty, "no change, no redraw");
        app.forced_look = Some(None);
        app.follow_look();
        assert_eq!(theme::current(), Theme::NORMAL);
        assert!(!pikslar_ui::icons::high_contrast());
    }

    #[test]
    fn high_contrast_draws_the_panels_in_the_window_color() {
        let mut app = crate::test_app::app(1);
        app.forced_look = Some(Some(SystemColors::WHITE));
        app.follow_look();
        let (w, h) = crate::test_app::WINDOW;
        let mut px = vec![0u32; (w * h) as usize];
        let mut surface = pikslar_render::Surface { px: &mut px, w, h };
        let window = pikslar_platform::Window::detached();
        app.draw_interface(&mut surface, window, &mut Vec::new());
        let white = px.iter().filter(|&&p| p == 0xffff_ffff).count();
        assert!(white > px.len() / 4, "{white} of {} pixels white", px.len());
        assert!(!px.contains(&Theme::NORMAL.get(theme::Token::Panel)));
    }
}

#[cfg(test)]
mod backdrop_tests {
    use super::*;
    use pikslar_render::{blit_canvas, Rect, Surface, View};

    /// A transparent one-pixel document at (3, 3) on a 5 by 5 surface, as drawn now.
    fn drawn() -> Vec<u32> {
        let mut px = vec![0u32; 25];
        let mut target = Surface {
            px: &mut px,
            w: 5,
            h: 5,
        };
        let view = View {
            num: 1,
            den: 1,
            x: 3,
            y: 3,
        };
        blit_canvas(
            &[0],
            1,
            1,
            view,
            &mut target,
            Rect::new(0, 0, 5, 5),
            &mut Vec::new(),
        );
        px
    }

    /// The color drawn well outside the document and the one behind its transparent pixel.
    fn backdrop_seen() -> (u32, u32) {
        let px = drawn();
        (px[0], px[18])
    }

    #[test]
    fn a_rule_colored_line_one_unit_wide_marks_the_document_edge() {
        let mut app = crate::test_app::app(1);
        app.forced_look = Some(None);
        app.follow_look();
        let rule = Theme::NORMAL.get(theme::Token::Rule);
        for scale in [1, 2] {
            app.resized(800, 600, scale);
            let px = drawn();
            let at = |x: usize, y: usize| px[y * 5 + x];
            assert_eq!(at(2, 3), rule, "left of the document, scale {scale}");
            assert_eq!(at(4, 3), rule, "right of it");
            assert_eq!(at(3, 2), rule, "above it");
            assert_eq!(at(2, 2), rule, "its corner");
            assert_ne!(at(3, 3), rule, "the document itself is never covered");
            let far = if scale == 1 {
                rule != at(1, 3)
            } else {
                rule == at(1, 3)
            };
            assert!(far, "one unit wide at scale {scale}");
        }
        app.resized(800, 600, 1);
    }

    #[test]
    fn the_canvas_is_drawn_on_neutral_grey_and_high_contrast_surrounds_it_in_the_window_color() {
        let mut app = crate::test_app::app(1);
        app.forced_look = Some(None);
        app.follow_look();
        let lightbox = Theme::NORMAL;
        let (outside, behind) = backdrop_seen();
        assert_eq!(outside, lightbox.get(theme::Token::Surround));
        assert_eq!(behind, lightbox.get(theme::Token::CheckerA));
        for grey in [outside, behind] {
            let (r, g, b) = ((grey >> 16) & 255, (grey >> 8) & 255, grey & 255);
            assert!(r == g && g == b, "{grey:#x} has no hue");
        }
        app.forced_look = Some(Some(SystemColors::WHITE));
        app.follow_look();
        let (outside, behind) = backdrop_seen();
        assert_eq!(outside, 0xffff_ffff, "the Window color");
        assert_eq!(
            behind,
            lightbox.get(theme::Token::CheckerA),
            "the checker stays neutral"
        );
    }
}
