//! The Keyboard shortcuts dialog: every letter shortcut, on three pages, each changed by
//! clicking it and pressing the new key. It is drawn over everything and covers what is under
//! it, so a click cannot reach the panels while it is open.

use crate::app::App;
use crate::shortcuts::{
    binding_name, is_reserved, letter_of, Action, Binding, Keymap, ACTIONS, FUNCTION_BASE,
};
use pikslar_platform::{Key, Mods};
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{button, default_button, draw_text, MENU_LEVEL};

const ID_KEYS: u32 = 900;
/// The pages, and which of [`ACTIONS`] each holds.
const PAGES: [(&str, usize, usize); 5] = [
    ("Tools", 0, 12),
    ("Text", 12, 17),
    ("Edit", 17, 32),
    ("Frames", 32, 49),
    ("Menus", 49, ACTIONS.len()),
];
const ROW: i32 = 22;
const WIDTH: i32 = 420;
const MOST_ROWS: i32 = 15;
const LABEL_W: i32 = 210;
/// The keys of the keyboard cursor, which are named keys and cannot be changed.
const CURSOR_KEYS: [&str; 3] = [
    "Keyboard cursor: arrows move",
    "it, Shift by 10. Space applies",
    "the tool; hold Space to drag.",
];

/// The dialog's state: the page shown, and the action waiting for its new key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeysDialog {
    pub page: usize,
    pub waiting: Option<Action>,
}

impl App {
    pub(crate) fn open_keys_dialog(&mut self) {
        self.close_menu();
        self.finish_naming();
        self.keys_dialog = Some(KeysDialog::default());
        self.ui.focus.clear();
        self.ui_dirty = true;
    }

    pub(crate) fn close_keys_dialog(&mut self) {
        self.keys_dialog = None;
        self.ui.focus.clear();
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    /// While the dialog is open the keyboard belongs to it. Waiting, a letter or digit with
    /// Ctrl and Shift as held becomes the key, Delete or Backspace leaves none, and Escape
    /// keeps the old one. Otherwise Escape closes the dialog.
    pub(crate) fn keys_dialog_key(&mut self, key: Key, mods: Mods) {
        let Some(dialog) = self.keys_dialog else {
            return;
        };
        self.ui_dirty = true;
        let Some(action) = dialog.waiting else {
            if key == Key::Escape {
                self.close_keys_dialog();
            } else if !self.focus_key(key, mods) {
                self.keys_arrow(key, dialog);
            }
            return;
        };
        if mods.alt {
            self.notify("Alt is kept for the menus: pick another key".to_string());
            return;
        }
        let binding = match key {
            Key::Escape => return self.stop_waiting(),
            Key::Delete | Key::Backspace => None,
            Key::F(n) => Some(Binding {
                letter: FUNCTION_BASE + n,
                ctrl: mods.ctrl,
                shift: mods.shift,
            }),
            Key::Char(c) => match letter_of(c) {
                Some(letter) => Some(Binding {
                    letter,
                    ctrl: mods.ctrl,
                    shift: mods.shift,
                }),
                None => return,
            },
            _ => {
                self.notify(
                    "Shortcuts use a letter, digit or function key, with Ctrl or Shift".to_string(),
                );
                return;
            }
        };
        if binding.is_some_and(is_reserved) {
            let why = "F6 and F10 with Shift belong to the window: pick another key";
            self.notify(why.to_string());
            return;
        }
        self.set_binding(action, binding);
        self.stop_waiting();
    }

    /// Left and Right move along the page tabs, Up and Down along the rows. Up from the first
    /// row is the page's tab, and Down from a tab is the page's first row.
    fn keys_arrow(&mut self, key: Key, dialog: KeysDialog) {
        let (_, first, end) = PAGES[dialog.page.min(PAGES.len() - 1)];
        let row = |id: u32| (ID_KEYS + 10 + first as u32..ID_KEYS + 10 + end as u32).contains(&id);
        let tab = |id: u32| (ID_KEYS..ID_KEYS + PAGES.len() as u32).contains(&id);
        let focus = &mut self.ui.focus;
        let on_tab = focus.focused().is_some_and(tab);
        let on_first = focus.focused() == Some(ID_KEYS + 10 + first as u32);
        match key {
            Key::Left | Key::Right => {
                focus.step_in(key == Key::Left, tab);
            }
            Key::Down if on_tab => focus.set(ID_KEYS + 10 + first as u32),
            Key::Up if on_first => focus.set(ID_KEYS + dialog.page as u32),
            Key::Up | Key::Down => {
                focus.step_in(key == Key::Up, row);
            }
            _ => return,
        }
        self.ui_dirty = true;
    }

    fn stop_waiting(&mut self) {
        if let Some(d) = &mut self.keys_dialog {
            d.waiting = None;
        }
    }

    /// Gives `action` the binding and says which action, if any, lost it.
    pub(crate) fn set_binding(&mut self, action: Action, binding: Option<Binding>) {
        let Some(taken) = self.keymap.bind(action, binding) else {
            return;
        };
        self.notify(format!(
            "{} no longer has {}",
            action_label(taken),
            binding_name(binding)
        ));
    }

    pub(crate) fn draw_keys_dialog(&mut self, sf: &mut Surface<'_>, dirty: &mut Vec<Rect>) {
        let Some(mut dialog) = self.keys_dialog else {
            return;
        };
        let s = self.ui.scale;
        let screen = Rect::new(0, 0, self.layout.bottom.w, self.layout.bottom.bottom());
        let (w, h) = (WIDTH * s, (86 + MOST_ROWS * ROW + 60) * s);
        let panel = Rect::new(
            (screen.w - w) / 2,
            (screen.h - h).max(0) / 2,
            w,
            h.min(screen.h),
        );
        self.ui.focus.collect(true);
        let back = self.ui.enter_level(MENU_LEVEL);
        pikslar_ui::chrome::popup(sf, panel, s);
        let left = panel.x + 16 * s;
        let ts = self.ui.text_scale();
        draw_text(
            sf,
            left,
            panel.y + 12 * s,
            "Keyboard shortcuts",
            theme::text(),
            ts,
        );
        self.draw_key_pages(sf, &mut dialog, left, panel.y + 38 * s);
        self.draw_key_rows(sf, &mut dialog, left, panel.y + 80 * s);
        self.draw_key_buttons(sf, Rect::new(left, panel.bottom() - 36 * s, w, 28 * s));
        self.ui.focus.collect(false);
        self.ui.enter_level(back);
        if self.keys_dialog.is_some() {
            self.keys_dialog = Some(dialog);
        }
        dirty.push(panel.inset(-s));
    }

    fn draw_key_pages(&mut self, sf: &mut Surface<'_>, d: &mut KeysDialog, x: i32, y: i32) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let mut at = x;
        for (n, (name, ..)) in PAGES.iter().enumerate() {
            let w = pikslar_ui::text_width(name, ts) + 16 * s;
            let r = Rect::new(at, y, w, 28 * s);
            if button(&mut self.ui, sf, ID_KEYS + n as u32, r, name, d.page == n) {
                *d = KeysDialog {
                    page: n,
                    waiting: None,
                };
                self.ui_dirty = true;
            }
            at += w + 4 * s;
        }
    }

    fn draw_key_rows(&mut self, sf: &mut Surface<'_>, d: &mut KeysDialog, x: i32, y: i32) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let (_, first, end) = PAGES[d.page.min(PAGES.len() - 1)];
        for (row, &(action, _, label, _)) in ACTIONS[first..end].iter().enumerate() {
            let top = y + row as i32 * ROW * s;
            draw_text(sf, x, top + 4 * s, label, theme::text(), ts);
            let waiting = d.waiting == Some(action);
            let name = if waiting {
                "Press a key".to_string()
            } else {
                binding_name(self.keymap.binding(action))
            };
            let r = Rect::new(
                x + LABEL_W * s,
                top,
                (WIDTH - 32 - LABEL_W) * s,
                (ROW - 2) * s,
            );
            let id = ID_KEYS + 10 + (first + row) as u32;
            if button(&mut self.ui, sf, id, r, &name, waiting) {
                d.waiting = (!waiting).then_some(action);
                self.ui_dirty = true;
            }
        }
        let hint = if d.waiting.is_some() {
            "Delete: none. Esc: keep"
        } else {
            "Click or Enter, then a new key"
        };
        let below = y + MOST_ROWS * ROW * s + 6 * s;
        draw_text(sf, x, below, hint, theme::muted(), ts);
        if first == 0 {
            let top = y + (end - first) as i32 * ROW * s + 4 * s;
            for (n, line) in CURSOR_KEYS.iter().enumerate() {
                let at = top + n as i32 * 16 * s;
                draw_text(sf, x, at, line, theme::muted(), ts);
            }
        }
    }

    fn draw_key_buttons(&mut self, sf: &mut Surface<'_>, row: Rect) {
        let s = self.ui.scale;
        let reset = Rect::new(row.x, row.y, 120 * s, row.h);
        let close = Rect::new(row.x + 128 * s, row.y, 100 * s, row.h);
        if button(&mut self.ui, sf, ID_KEYS + 5, reset, "Usual keys", false) {
            self.defer(|a| {
                a.keymap = Keymap::default();
                a.stop_waiting();
                a.notify("Every shortcut is back on its usual key".to_string());
            });
        }
        if default_button(&mut self.ui, sf, ID_KEYS + 6, close, "Close") {
            self.defer(App::close_keys_dialog);
        }
        let ts = self.ui.text_scale();
        version_footer(sf, close.right() + 12 * s, row.x + row.w - 32 * s, row, ts);
    }
}

/// The program's version, right-aligned in the footer beside the buttons, in the footer
/// hint's muted text (D7: no new element). Shortened, then left out, when it has no room.
fn version_footer(sf: &mut Surface<'_>, from: i32, to: i32, row: Rect, ts: i32) {
    if let Some(text) = crate::about::footer_text(to - from, ts) {
        let x = to - pikslar_ui::text_width(&text, ts);
        let y = row.y + (row.h - pikslar_ui::cap_height() * ts) / 2;
        draw_text(sf, x, y, &text, theme::muted(), ts);
    }
}

/// The name of `action` on screen.
pub(crate) fn action_label(action: Action) -> &'static str {
    ACTIONS
        .iter()
        .find(|(a, ..)| *a == action)
        .map_or("", |(_, _, label, _)| label)
}

#[cfg(test)]
#[path = "keys_dialog_tests.rs"]
mod tests;
