//! The menu bar (docs/ui-redesign.md 3.1): File, Edit, View, Window and Scripts titles at the
//! left of the top bar, each opening its menu under itself. Mouse, Tab, Alt or F10 to enter
//! the bar, Left and Right between titles, Down or Enter to open one, Alt with a title's
//! underlined letter to open it from anywhere, and Left and Right inside an open menu to go to
//! its neighbour.

use crate::app::App;
use crate::frame_menu::{MenuKind, OpenMenu};
use crate::panel_side::ID_FILE;
use pikslar_platform::{Key, Mods, Window};
use pikslar_render::{Rect, Surface};
use pikslar_ui::chrome::{face, Look};
use pikslar_ui::focus::WidgetKey;
use pikslar_ui::role::{Role, State};
use pikslar_ui::theme;
use pikslar_ui::{cap_height, draw_text, menu_key, text_width, MenuKey, MenuNav};

/// A title of the bar.
struct Title {
    name: &'static str,
    /// The letter that, with Alt, opens the menu; underlined in the title.
    access: char,
    kind: MenuKind,
    hint: &'static str,
}

/// How many titles the bar has.
pub(crate) const MENUS: usize = 5;
/// The indexes of the titles that other code opens by key.
pub(crate) const VIEW: usize = 2;
pub(crate) const WINDOW: usize = 3;
pub(crate) const SCRIPTS: usize = 4;
/// The overflow button, after the titles.
pub(crate) const OVERFLOW: u32 = ID_FILE + MENUS as u32;
/// Its words.
const MORE: &str = "{bb}";

/// The titles, in order.
const BAR: [Title; MENUS] = [
    Title {
        name: "File",
        access: 'F',
        kind: MenuKind::File,
        hint: "New, open, save and export",
    },
    Title {
        name: "Edit",
        access: 'E',
        kind: MenuKind::Edit,
        hint: "Undo, copy and paste, selection, canvas size",
    },
    Title {
        name: "View",
        access: 'V',
        kind: MenuKind::View,
        hint: "Interface size, keyboard shortcuts, undo history",
    },
    Title {
        name: "Window",
        access: 'W',
        kind: MenuKind::Window,
        hint: "Show and hide panels, workspaces",
    },
    Title {
        name: "Scripts",
        access: 'S',
        kind: MenuKind::Scripts,
        hint: "Run a script, or install and run plugins",
    },
];

/// What the menu bar and the window as a whole keep between frames.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WindowUi {
    /// Where each title was last drawn, which its menu opens under.
    pub titles: [Rect; MENUS],
    /// How many titles, from the right, did not fit and are in the overflow menu.
    pub hidden: usize,
    /// Every dock area shown as its rail, until the docks are asked for again (F11).
    pub canvas_only: bool,
}

/// The title whose menu `kind` is, or belongs to: a plugin's menu belongs to Scripts.
fn home_title(kind: MenuKind) -> Option<usize> {
    match kind {
        MenuKind::Plugin(_) => Some(SCRIPTS),
        k => bar_index(k),
    }
}

/// The title index of a menu kind, if it is one of the bar's.
pub(crate) fn bar_index(kind: MenuKind) -> Option<usize> {
    BAR.iter().position(|t| t.kind == kind)
}

impl App {
    /// Draws the titles from `x`, and returns where the next thing may start. Titles that do
    /// not fit before the bar's right edge give way to a button that opens them as a menu.
    pub(crate) fn draw_menu_bar(&mut self, sf: &mut Surface<'_>, x: i32, y: i32, h: i32) -> i32 {
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        let widths = BAR.map(|t| text_width(t.name, ts) + 16 * s);
        let more_w = text_width(MORE, ts) + 16 * s;
        let room = self.layout.top.right() - 8 * s - x;
        let needs = |k: usize| {
            let titles: i32 = widths[..k].iter().map(|w| w + 4 * s).sum();
            titles + if k < MENUS { more_w + 4 * s } else { 0 }
        };
        let shown = (0..=MENUS).rev().find(|&k| needs(k) <= room).unwrap_or(0);
        self.window_ui.hidden = MENUS - shown;
        let mut x = x;
        for (i, title) in BAR.iter().enumerate().take(shown) {
            let rect = Rect::new(x, y, widths[i], h);
            self.window_ui.titles[i] = rect;
            self.note_hint(rect, title.hint);
            self.draw_title(sf, i, rect);
            x += widths[i] + 4 * s;
        }
        if shown < MENUS {
            let rect = Rect::new(x, y, more_w, h);
            self.window_ui.titles[shown..].fill(rect);
            self.draw_overflow(sf, shown, rect);
            x += more_w + 4 * s;
        }
        x
    }

    /// The button that opens the titles that did not fit.
    fn draw_overflow(&mut self, sf: &mut Surface<'_>, shown: usize, r: Rect) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        self.note_hint_named(r, "More menus", "The menus that do not fit");
        let open = self.menu.is_some_and(|m| {
            m.kind == MenuKind::More || home_title(m.kind).is_some_and(|i| i >= shown)
        });
        let state = self.ui.interact(OVERFLOW, r);
        self.ui.focus.register_keys(OVERFLOW);
        self.ui.describe(
            Role::MenuItem,
            State {
                expanded: Some(open),
                ..State::NONE
            },
        );
        self.ui.name(OVERFLOW, "More menus");
        let look = Look::of(state, open, false);
        face(sf, r, s, theme::header(), look);
        let tx = r.x + (r.w - text_width(MORE, ts)) / 2;
        let ty = r.y + (r.h - cap_height() * ts) / 2 + look.drop() * s;
        draw_text(sf, tx, ty, MORE, look.ink(), ts);
        self.ui.ring(sf, OVERFLOW, r);
        let keyed = self.ui.focus.take_press(OVERFLOW).map(|p| p.key);
        if state.clicked {
            self.defer(|a| a.open_more_menu(false));
        } else if let Some(key) = keyed {
            self.overflow_key(shown, key);
        }
    }

    fn overflow_key(&mut self, shown: usize, key: WidgetKey) {
        let to = match key {
            WidgetKey::Left => shown.saturating_sub(1),
            WidgetKey::Right | WidgetKey::Home => 0,
            WidgetKey::End => shown,
            WidgetKey::Up | WidgetKey::Down | WidgetKey::Enter => {
                self.defer(|a| a.open_more_menu(true));
                return;
            }
            WidgetKey::Menu => return,
        };
        self.ui.focus.set(self.title_focus(to));
    }

    /// The focus stop of title `i`: the title, or the overflow button when it did not fit.
    fn title_focus(&self, i: usize) -> u32 {
        if i >= MENUS - self.window_ui.hidden {
            OVERFLOW
        } else {
            ID_FILE + i as u32
        }
    }

    /// The rows of the overflow menu: the titles that did not fit.
    pub(crate) fn more_entries(&self) -> crate::frame_menu::Entries {
        BAR[MENUS - self.window_ui.hidden..]
            .iter()
            .map(|t| (std::borrow::Cow::Borrowed(t.name), true))
            .collect()
    }

    /// Opens the menu of the hidden title row `n` of the overflow menu is.
    pub(crate) fn more_menu_action(&mut self, n: usize) {
        self.open_bar_menu(MENUS - self.window_ui.hidden + n, true);
    }

    /// Opens the overflow menu under its button.
    pub(crate) fn open_more_menu(&mut self, by_key: bool) {
        let first = MENUS - self.window_ui.hidden;
        let Some(r) = self.window_ui.titles.get(first).copied() else {
            return;
        };
        self.finish_naming();
        self.menu = Some(OpenMenu {
            at: (r.x, self.layout.top.bottom()),
            kind: MenuKind::More,
        });
        self.menu_cursor = None;
        self.ctx.menu_closed();
        self.ui.focus.set(OVERFLOW);
        self.ui_dirty = true;
        self.light_first_row(by_key);
    }

    /// By key, the first row that can be chosen of the menu just opened is highlighted.
    fn light_first_row(&mut self, by_key: bool) {
        if let Some(open) = self.menu.filter(|_| by_key) {
            let rows = self.menu_entries(open.kind);
            let items = crate::frame_menu::as_items(&rows);
            if let MenuNav::Highlight(n) = menu_key(&items, None, MenuKey::Down) {
                self.menu_cursor = Some((open, n));
            }
        }
    }

    fn draw_title(&mut self, sf: &mut Surface<'_>, i: usize, r: Rect) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let title = &BAR[i];
        let id = ID_FILE + i as u32;
        let open = self.menu.is_some_and(|m| m.kind == title.kind);
        let state = self.ui.interact(id, r);
        self.ui.focus.register_keys(id);
        let shown = State {
            expanded: Some(open),
            ..State::NONE
        };
        self.ui.describe(Role::MenuItem, shown);
        self.ui.name(id, title.name);
        let look = Look::of(state, open, false);
        face(sf, r, s, theme::header(), look);
        let tx = r.x + (r.w - text_width(title.name, ts)) / 2;
        let ty = r.y + (r.h - cap_height() * ts) / 2 + look.drop() * s;
        draw_text(sf, tx, ty, title.name, look.ink(), ts);
        // The access letter is underlined, so the key to press is shown without colour.
        let lead = title
            .name
            .find(title.access)
            .map_or(title.name, |at| &title.name[..at]);
        let under_x = tx + text_width(lead, ts);
        let mut letter = [0u8; 4];
        let under_of: &str = title.access.encode_utf8(&mut letter);
        let under = Rect::new(
            under_x,
            ty + cap_height() * ts + s,
            text_width(under_of, ts),
            s,
        );
        sf.fill(under, look.ink());
        self.ui.ring(sf, id, r);
        let keyed = self.ui.focus.take_press(id).map(|p| p.key);
        if state.clicked {
            self.defer(move |a| a.open_bar_menu(i, false));
        } else if let Some(key) = keyed {
            self.title_key(i, key);
        }
    }

    /// A key on the focused title `i`.
    fn title_key(&mut self, i: usize, key: WidgetKey) {
        let to = match key {
            WidgetKey::Left => (i + MENUS - 1) % MENUS,
            WidgetKey::Right => (i + 1) % MENUS,
            WidgetKey::Home => 0,
            WidgetKey::End => MENUS - 1,
            WidgetKey::Up | WidgetKey::Down | WidgetKey::Enter => {
                self.defer(move |a| a.open_bar_menu(i, true));
                return;
            }
            WidgetKey::Menu => return,
        };
        self.ui.focus.set(self.title_focus(to));
    }

    /// Opens the menu of title `i` under it, replacing any menu open. By key, the first row
    /// that can be chosen is highlighted. Focus rests on the title.
    pub(crate) fn open_bar_menu(&mut self, i: usize, by_key: bool) {
        let Some(title) = BAR.get(i) else {
            return;
        };
        self.finish_naming();
        self.playing = false;
        let r = self.window_ui.titles[i];
        // Under the bar, not on it, so the title's focus ring is never clipped.
        let at = if r.w > 0 {
            (r.x, self.layout.top.bottom())
        } else {
            (self.layout.top.x, self.layout.top.bottom())
        };
        self.menu_cursor = None;
        match title.kind {
            MenuKind::File => self.open_file_menu(at),
            MenuKind::View => self.open_view_menu(at),
            MenuKind::Scripts => self.open_scripts_menu(at),
            kind => {
                self.menu = Some(OpenMenu { at, kind });
                self.ui_dirty = true;
            }
        }
        self.ctx.menu_closed();
        self.ui.focus.set(self.title_focus(i));
        self.light_first_row(by_key);
    }

    /// F10 or a tap of Alt: an open menu closes, focus on the bar is given up, and otherwise
    /// the first title takes focus.
    pub(crate) fn menu_bar_toggle(&mut self) {
        if self.menu.is_some_and(|m| bar_index(m.kind).is_some()) {
            self.close_menu();
        } else if self.on_menu_bar() {
            self.ui.focus.clear();
        } else {
            self.ui.focus.set(ID_FILE);
        }
        self.ui_dirty = true;
    }

    fn on_menu_bar(&self) -> bool {
        let bar = ID_FILE..=OVERFLOW;
        self.ui.focus.focused().is_some_and(|f| bar.contains(&f))
    }

    /// Alt with a letter: opens the menu whose underlined letter it is. Returns whether it did.
    pub(crate) fn bar_access(&mut self, letter: u8) -> bool {
        let c = char::from(letter).to_ascii_uppercase();
        let Some(i) = BAR.iter().position(|t| t.access == c) else {
            return false;
        };
        self.open_bar_menu(i, true);
        self.ui_dirty = true;
        true
    }

    /// Keys for an open menu that its rows do not take: F10 (or whatever the menu bar action is
    /// bound to) or a tap of Alt closes it, and from a context menu goes on to the bar; Alt with a
    /// letter opens that title's menu; Left and Right go to the next title's menu (from a plugin's
    /// menu too, which belongs to Scripts). Returns whether the key was used.
    pub(crate) fn bar_menu_key(&mut self, key: Key, mods: Mods) -> bool {
        let Some(open) = self.menu else {
            return false;
        };
        let home = home_title(open.kind);
        if key == Key::AltTap || self.is_menu_bar_key(key, mods) {
            self.close_menu();
            if home.is_none() {
                self.ui.focus.set(ID_FILE);
            }
        } else if let (Key::Left | Key::Right, Some(i)) = (key, home) {
            let to = if key == Key::Left {
                i + MENUS - 1
            } else {
                i + 1
            };
            self.open_bar_menu(to % MENUS, true);
        } else if let Key::Char(c) = key {
            if !mods.alt || mods.ctrl {
                return false;
            }
            self.bar_access(c);
        } else {
            return false;
        }
        self.ui_dirty = true;
        true
    }

    /// Whether the key is the one the menu bar action is bound to now.
    fn is_menu_bar_key(&self, key: Key, mods: Mods) -> bool {
        let letter = match key {
            Key::F(n) => crate::shortcuts::FUNCTION_BASE + n,
            Key::Char(c) => c,
            _ => return false,
        };
        let pressed = crate::shortcuts::Binding {
            letter,
            ctrl: mods.ctrl,
            shift: mods.shift,
        };
        !mods.alt && self.keymap.action_for(pressed) == Some(crate::shortcuts::Action::MenuBar)
    }

    /// A function key with the keys held: the action it is bound to.
    pub(crate) fn function_key(&mut self, n: u8, mods: Mods, window: Window) {
        if mods.alt {
            return;
        }
        let pressed = crate::shortcuts::Binding {
            letter: crate::shortcuts::FUNCTION_BASE + n,
            ctrl: mods.ctrl,
            shift: mods.shift,
        };
        if let Some(action) = self.keymap.action_for(pressed) {
            self.run_action(action, window);
        }
    }
}

#[cfg(test)]
#[path = "menu_bar_tests.rs"]
mod tests;
