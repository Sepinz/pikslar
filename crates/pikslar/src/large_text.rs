//! The *Large text* option: interface words drawn in Pikslar Mono, doubled, instead of Pikslar
//! Text (docs/ui-redesign.md 2.6). Everything else keeps its scale; the layout follows the
//! words' widths. Kept in the settings, offered in the View menu, and a rebindable action.

use crate::app::App;
use pikslar_ui::font::{self, Face};

/// The View menu's row, off and on.
pub(crate) const LABEL: &str = "Large text";
pub(crate) const LABEL_ON: &str = "Large text, in use";

/// The face for the option's state.
pub(crate) fn face_for(large: bool) -> Face {
    if large {
        Face::Mono
    } else {
        Face::Text
    }
}

/// The text size, in percent, from which Windows' own setting turns Large text on by itself.
pub(crate) const AUTO_PERCENT: i32 = 125;

/// Whether Large text is on: the user's choice when there is one, else on from
/// [`AUTO_PERCENT`] of Windows' text size.
pub(crate) fn resolve(percent: i32, chosen: Option<bool>) -> bool {
    chosen.unwrap_or(percent >= AUTO_PERCENT)
}

impl App {
    /// Draws words in the face for `on` from now on, and lays the screen out again.
    pub(crate) fn set_large_text(&mut self, on: bool) {
        self.large_text = on;
        font::set_face(face_for(on));
        // Before the window has a size the first resize lays everything out in the new face.
        let (w, h) = self.size;
        if w > 0 && h > 0 {
            self.resized(w, h, self.ui.scale.max(1));
        }
        self.ui_dirty = true;
        self.full_canvas = true;
    }

    /// Applies the saved choice (none: automatic) with the text size this window runs at.
    pub(crate) fn apply_large_text_setting(&mut self, chosen: Option<bool>) {
        self.large_text_chosen = chosen.is_some();
        self.set_large_text(resolve(self.text_percent, chosen));
    }

    /// Turns the option over, from the View menu or its key.
    pub(crate) fn toggle_large_text(&mut self) {
        self.large_text_chosen = true;
        self.set_large_text(!self.large_text);
        self.notify(if self.large_text {
            "Large text on".to_string()
        } else {
            "Large text off".to_string()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::shortcuts::Action;
    use crate::test_app::{app, draw_whole};

    #[test]
    fn text_is_the_default_and_large_text_switches_the_face() {
        let mut a = app(1);
        assert!(!a.large_text);
        assert_eq!(font::face(), Face::Text);
        let small = a.ui.text_scale();
        a.set_large_text(true);
        assert_eq!(font::face(), Face::Mono);
        assert_eq!(a.ui.text_scale(), 2 * small, "Mono is drawn doubled");
        assert!(a.ui_dirty && a.full_canvas, "everything is drawn again");
        a.set_large_text(false);
        assert_eq!(font::face(), Face::Text);
    }

    #[test]
    fn large_text_widens_the_words_and_the_screen_still_draws() {
        let mut a = app(2);
        let title = |a: &App| a.window_ui.titles[0].w;
        draw_whole(&mut a);
        let narrow = title(&a);
        a.set_large_text(true);
        draw_whole(&mut a);
        assert!(title(&a) > narrow, "{} against {narrow}", title(&a));
    }

    #[test]
    fn the_setting_is_read_and_written_and_defaults_to_automatic() {
        assert_eq!(Settings::parse("").large_text, None);
        assert_eq!(Settings::parse("large_text = yes").large_text, Some(true));
        assert_eq!(Settings::parse("large_text = no").large_text, Some(false));
        assert_eq!(Settings::parse("large_text = auto").large_text, None);
        assert_eq!(
            Settings::parse("large_text = maybe").large_text,
            None,
            "kept default"
        );
        for explicit in [Some(true), Some(false), None] {
            let s = Settings {
                large_text: explicit,
                ..Settings::default()
            };
            assert_eq!(Settings::parse(&s.to_text()), s);
        }
        assert!(Settings::default().to_text().contains("large_text = auto"));
    }

    #[test]
    fn windows_text_size_of_125_percent_or_more_turns_it_on_unless_chosen() {
        for (percent, chosen, want) in [
            (100, None, false),
            (124, None, false),
            (125, None, true),
            (150, None, true),
            (225, None, true),
            (150, Some(false), false),
            (100, Some(true), true),
            (125, Some(true), true),
        ] {
            assert_eq!(resolve(percent, chosen), want, "{percent} {chosen:?}");
        }
    }

    #[test]
    fn the_rule_applies_when_settings_load_and_a_choice_is_kept_whatever_the_size() {
        let mut a = app(1);
        a.text_percent = 150;
        a.apply_large_text_setting(None);
        assert!(a.large_text, "automatic at 150 percent");
        assert_eq!(a.current_settings().large_text, None, "still automatic");
        assert!(!a.settings_changed());
        a.toggle_large_text();
        assert!(!a.large_text);
        assert_eq!(a.current_settings().large_text, Some(false), "now a choice");
        assert!(a.settings_changed());
        a.apply_large_text_setting(Some(false));
        assert!(!a.large_text, "a saved no is never overridden");
        a.text_percent = 100;
        a.apply_large_text_setting(None);
        assert!(!a.large_text);
        font::set_face(Face::Text);
    }

    #[test]
    fn a_change_is_a_settings_change() {
        let mut a = app(1);
        assert_eq!(a.current_settings().large_text, None);
        a.toggle_large_text();
        assert_eq!(a.current_settings().large_text, Some(true));
        assert!(a.settings_changed(), "it is saved with the others");
    }

    #[test]
    fn it_is_an_action_without_a_key_and_the_view_menu_toggles_it() {
        assert!(crate::shortcuts::ACTIONS
            .iter()
            .any(|&(act, name, ..)| act == Action::LargeText && name == "large_text"));
        let mut a = app(1);
        let at = a.view_items().iter().position(|i| i.label == LABEL);
        let at = at.expect("the View menu offers it");
        a.view_menu_action(at, pikslar_platform::Window::detached());
        assert!(a.large_text);
        assert_eq!(a.view_items()[at].label, LABEL_ON);
        assert!(a.window_action(Action::LargeText));
        assert!(!a.large_text);
    }
}
