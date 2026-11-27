//! The menu a right click on the onion skin button opens: which neighbours show, how strongly,
//! in which colours, and how far.

use crate::app::App;
use crate::frame_menu::{MenuKind, OpenMenu};
use pikslar_render::onion::STRENGTHS;
use pikslar_render::{OnionColors, OnionStyle};
use pikslar_ui::MenuItem;

fn colors_label(colors: OnionColors) -> &'static str {
    match colors {
        OnionColors::RedBlue => "Colors: red and blue",
        OnionColors::GreenPurple => "Colors: green and purple",
        OnionColors::Own => "Colors: their own",
    }
}

fn depth_label(depth: usize) -> &'static str {
    match depth {
        1 => "Depth: 1 frame",
        2 => "Depth: 2 frames",
        _ => "Depth: 3 frames",
    }
}

impl App {
    /// Opens the onion menu if `(x, y)` is on the onion skin button.
    pub(crate) fn open_onion_menu(&mut self, x: i32, y: i32) -> bool {
        if !self.onion_button.contains(x, y) {
            return false;
        }
        // Playback hides the skin, so a choice made now would show no effect.
        self.playing = false;
        self.menu = Some(OpenMenu {
            at: (x, y),
            kind: MenuKind::Onion,
        });
        self.ui_dirty = true;
        true
    }

    pub(crate) fn onion_items(&self) -> Vec<MenuItem<'static>> {
        let style = self.comp.onion_style;
        let side =
            |shown: bool, hide: &'static str, show: &'static str| if shown { hide } else { show };
        vec![
            MenuItem {
                label: side(style.past, "Hide earlier frames", "Show earlier frames"),
                // One side always shows, or the skin would show nothing while switched on.
                enabled: !style.past || style.future,
            },
            MenuItem {
                label: side(style.future, "Hide later frames", "Show later frames"),
                enabled: !style.future || style.past,
            },
            MenuItem {
                label: "Stronger",
                enabled: style.stronger().is_some(),
            },
            MenuItem {
                label: "Fainter",
                enabled: style.fainter().is_some(),
            },
            MenuItem {
                label: colors_label(style.colors),
                enabled: true,
            },
            MenuItem {
                label: depth_label(self.comp.onion_depth),
                enabled: true,
            },
        ]
    }

    /// Every choice also turns the skin on, so its effect is seen.
    pub(crate) fn onion_menu_action(&mut self, n: usize) {
        let style = self.comp.onion_style;
        let next = match n {
            0 => Some(OnionStyle {
                past: !style.past,
                ..style
            }),
            1 => Some(OnionStyle {
                future: !style.future,
                ..style
            }),
            2 => style.stronger(),
            3 => style.fainter(),
            4 => Some(OnionStyle {
                colors: style.colors.next(),
                ..style
            }),
            _ => {
                self.cycle_onion_depth();
                return;
            }
        };
        let Some(next) = next.filter(|s| s.past || s.future) else {
            return;
        };
        self.set_onion_style(next);
        let said = match n {
            2 | 3 => format!("Onion skin at {}%", STRENGTHS[next.strength]),
            4 => format!("Onion skin in {}", next.colors.name()),
            _ => return,
        };
        self.notify(said);
    }

    fn set_onion_style(&mut self, style: OnionStyle) {
        self.comp.onion_style = style;
        self.comp.onion = true;
        self.editor.damage_all();
        self.ui_dirty = true;
    }
}
