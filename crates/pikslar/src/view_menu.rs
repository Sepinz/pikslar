//! The View menu: the interface size, automatic or chosen.

use crate::app::App;
use crate::frame_menu::{MenuKind, OpenMenu};
use pikslar_platform::{size_fits, Window, SMALLEST_UNITS};
use pikslar_render::{Rect, Surface};
use pikslar_ui::icons::MarkIcon;
use pikslar_ui::MenuItem;

/// Units left of the labels for the marks.
pub(crate) const VIEW_ROOM: i32 = 16;
/// The mark's own room at the left of that, so a gap of six units parts it from the label.
const MARK_W: i32 = 10;

/// The sizes offered, in menu order: automatic, then 1x, 2x and 3x.
const SIZES: [Option<i32>; 4] = [None, Some(1), Some(2), Some(3)];
/// After the sizes. Large text is last, so the rows before it keep their ids.
const KEYS_ITEM: &str = "Keyboard shortcuts...";
const MAKE_SLICE: &str = "Make slice from selection";
/// Rewind, after the shortcuts: the label says what turning it on does.
const REWIND_ON: &str = "Keep undo history after closing (Rewind)";
const REWIND_OFF: &str = "Stop keeping undo history (Rewind is on)";
const FORGET: &str = "Forget this file's undo history";
const FORGET_ALL: &str = "Forget all kept undo history";
/// Diagnostics, after Large text: the label says what choosing it does, as Rewind's does.
pub(crate) const DIAG_ON: &str = "Keep crash and saving notes";
pub(crate) const DIAG_OFF: &str = "Stop keeping crash and saving notes";
pub(crate) const DIAG_DELETE: &str = "Delete crash and saving notes";
/// `--no-diagnostics`: off for this run, whatever the setting says, which it leaves alone.
pub(crate) const DIAG_RUN_OFF: &str = "Crash and saving notes: off for this run";
const LABELS: [&str; 4] = [
    "Automatic size",
    "1x interface",
    "2x interface",
    "3x interface",
];
const IN_USE: [&str; 4] = [
    "Automatic size, in use",
    "1x interface, in use",
    "2x interface, in use",
    "3x interface, in use",
];
const WAITING: [&str; 4] = [
    "Automatic size, in use",
    "1x interface, chosen",
    "2x interface, chosen, window too small",
    "3x interface, chosen, window too small",
];

impl App {
    /// Opens the View menu under the point `at`.
    pub(crate) fn open_view_menu(&mut self, at: (i32, i32)) {
        self.menu = Some(OpenMenu {
            at,
            kind: MenuKind::View,
        });
        self.ui_dirty = true;
    }

    pub(crate) fn view_items(&self) -> Vec<MenuItem<'static>> {
        SIZES
            .iter()
            .enumerate()
            .map(|(n, &size)| MenuItem {
                label: match size {
                    _ if size != self.interface_size => LABELS[n],
                    Some(s) if s != self.ui.scale => WAITING[n],
                    _ => IN_USE[n],
                },
                enabled: true,
            })
            .chain([
                MenuItem {
                    label: KEYS_ITEM,
                    enabled: true,
                },
                MenuItem {
                    label: if self.rewind.on {
                        REWIND_OFF
                    } else {
                        REWIND_ON
                    },
                    enabled: true,
                },
                MenuItem {
                    label: FORGET,
                    enabled: self.editor.doc().document_id.get().is_some(),
                },
                MenuItem {
                    label: FORGET_ALL,
                    enabled: true,
                },
                MenuItem {
                    label: if self.show_slices {
                        "Hide slices"
                    } else {
                        "Show slices"
                    },
                    enabled: true,
                },
                MenuItem {
                    label: MAKE_SLICE,
                    enabled: self.editor.selection().is_some(),
                },
                MenuItem {
                    label: if self.large_text {
                        crate::large_text::LABEL_ON
                    } else {
                        crate::large_text::LABEL
                    },
                    enabled: true,
                },
                MenuItem {
                    label: match (self.diagnostics_run_off, self.diagnostics) {
                        (true, _) => DIAG_RUN_OFF,
                        (false, true) => DIAG_OFF,
                        (false, false) => DIAG_ON,
                    },
                    enabled: !self.diagnostics_run_off,
                },
                MenuItem {
                    label: DIAG_DELETE,
                    enabled: true,
                },
            ])
            .collect()
    }

    /// The mark each row of the View menu carries: a dot on the chosen size, a check on each
    /// switch that is on (slices shown, Rewind kept, Large text, diagnostics kept) and an empty box on each that
    /// is off. Rows without a state have none.
    pub(crate) fn view_marks(&self) -> [Option<MarkIcon>; 13] {
        let mut marks = [None; 13];
        for (mark, &size) in marks.iter_mut().zip(&SIZES) {
            *mark = (size == self.interface_size).then_some(MarkIcon::RadioDot);
        }
        // An off switch shows an empty box, so it never looks like a command.
        let switch = |on: bool| {
            Some(if on {
                MarkIcon::Check
            } else {
                MarkIcon::CelEmpty
            })
        };
        marks[5] = switch(self.rewind.on);
        marks[8] = switch(self.show_slices);
        marks[10] = switch(self.large_text);
        marks[11] = switch(self.diagnostics && !self.diagnostics_run_off);
        marks
    }

    /// Draws the marks in the room left of the open View menu's labels.
    pub(crate) fn draw_view_marks(&mut self, sf: &mut Surface<'_>, rect: Rect) {
        let s = self.ui.scale;
        let marks = self.view_marks();
        let look = pikslar_ui::chrome::Look::default();
        for (n, mark) in marks.iter().enumerate() {
            let Some(mark) = mark else { continue };
            let row = pikslar_ui::menu_row(rect, n, s);
            if row.bottom() > rect.bottom() {
                break;
            }
            let at = Rect::new(row.x + pikslar_ui::MENU_PAD * s, row.y, MARK_W * s, row.h);
            pikslar_ui::chrome::art_in(sf, at, mark.art(), look, s);
        }
    }

    pub(crate) fn view_menu_action(&mut self, n: usize, window: Window) {
        match SIZES.get(n) {
            Some(&size) => {
                self.choose_interface_size(size, window);
            }
            None if n == SIZES.len() => self.open_keys_dialog(),
            None if n == SIZES.len() + 1 => self.toggle_rewind(),
            None if n == SIZES.len() + 2 => self.forget_rewind(),
            None if n == SIZES.len() + 3 => self.forget_all_rewind(),
            None if n == SIZES.len() + 4 => self.toggle_slices(),
            None if n == SIZES.len() + 5 => {
                self.make_slice_from_selection();
            }
            None if n == SIZES.len() + 6 => self.toggle_large_text(),
            None if n == SIZES.len() + 7 => self.toggle_diagnostics(),
            None if n == SIZES.len() + 8 => self.delete_diagnostics(),
            None => {}
        }
    }

    /// Uses `size` from the next frame, or refuses with a message when the window has no room
    /// for it. Returns whether it was taken.
    pub(crate) fn choose_interface_size(&mut self, size: Option<i32>, window: Window) -> bool {
        let (w, h) = self.size;
        if let Some(s) = size.filter(|&s| !size_fits(s, w, h)) {
            let (need_w, need_h) = (SMALLEST_UNITS.0 * s, SMALLEST_UNITS.1 * s);
            self.notify(format!(
                "{s}x needs a window at least {need_w} by {need_h} pixels"
            ));
            return false;
        }
        self.interface_size = size;
        window.set_interface_size(size);
        self.notify(match size {
            None => "The interface size follows the display".to_string(),
            Some(s) => format!("Interface size {s}x"),
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use pikslar_platform::Window;

    #[test]
    fn a_size_that_fits_is_taken_and_one_that_does_not_is_refused() {
        let mut a = App::launch(None, Some(32), None);
        a.resized(1920, 1003, 1);
        let window = Window::detached();
        assert!(a.choose_interface_size(Some(2), window));
        assert_eq!(a.interface_size, Some(2));
        a.resized(1920, 1003, 2);
        assert!(a.view_items()[2].label.ends_with("in use"));
        a.resized(1200, 900, 1);
        assert_eq!(
            a.view_items()[2].label,
            "2x interface, chosen, window too small"
        );
        assert!(a.view_items().iter().all(|i| !i.label.ends_with("in use")));
        a.resized(1920, 1003, 2);
        assert!(!a.choose_interface_size(Some(3), window));
        assert_eq!(a.interface_size, Some(2), "unchanged");
        let said = a.message.as_ref().map(|(text, _)| text.clone());
        assert_eq!(
            said.as_deref(),
            Some("3x needs a window at least 1920 by 1440 pixels")
        );
        assert!(a.choose_interface_size(None, window));
        assert!(a.view_items()[0].label.ends_with("in use"));
    }

    #[test]
    fn the_view_menu_opens_from_the_top_bar() {
        let mut a = App::launch(None, Some(32), None);
        a.resized(1366, 705, 1);
        a.open_view_menu((10, 30));
        assert_eq!(
            a.menu.map(|m| m.kind),
            Some(crate::frame_menu::MenuKind::View)
        );
        a.view_menu_action(2, Window::detached());
        assert_eq!(a.interface_size, None, "1366 by 705 has no room for 2x");
    }
    #[test]
    fn the_toggles_and_the_chosen_size_carry_a_mark() {
        use pikslar_ui::icons::MarkIcon::{CelEmpty, Check, RadioDot};
        let mut a = App::launch(None, Some(32), None);
        a.resized(1920, 1003, 2);
        a.interface_size = None;
        let marks = a.view_marks();
        assert_eq!(marks[0], Some(RadioDot), "automatic is chosen");
        assert_eq!(marks[1..4], [None, None, None]);
        assert_eq!(marks[8], Some(Check), "slices are shown");
        assert_eq!(marks[10], Some(CelEmpty), "large text is off: an empty box");
        a.interface_size = Some(2);
        a.show_slices = false;
        a.set_large_text(true);
        let marks = a.view_marks();
        assert_eq!(marks[0], None);
        assert_eq!(marks[2], Some(RadioDot));
        assert_eq!(marks[8], Some(CelEmpty));
        assert_eq!(marks[10], Some(Check));
        assert_eq!(marks.len(), a.view_items().len());
        pikslar_ui::font::set_face(pikslar_ui::font::Face::Text);
    }

    #[test]
    fn no_diagnostics_for_one_run_shows_so_and_never_changes_the_setting() {
        let mut a = App::launch(None, Some(32), None);
        a.resized(1920, 1003, 1);
        a.use_diagnostics(true);
        let n = a.view_items().len();
        assert_eq!(a.view_items()[n - 2].label, super::DIAG_RUN_OFF);
        assert!(!a.view_items()[n - 2].enabled);
        a.view_menu_action(n - 2, Window::detached());
        assert!(a.diagnostics, "the kept choice is untouched");
        assert!(!a.settings_changed(), "and never written");
        assert!(a.message.is_some(), "the refusal says why");
    }

    #[test]
    fn diagnostics_are_a_switch_and_a_delete_at_the_end_of_the_view_menu() {
        use pikslar_ui::icons::MarkIcon::{CelEmpty, Check};
        let mut a = App::launch(None, Some(32), None);
        a.resized(1920, 1003, 1);
        let items = a.view_items();
        let n = items.len();
        assert_eq!(items[n - 2].label, super::DIAG_OFF, "on by default");
        assert_eq!(items[n - 1].label, super::DIAG_DELETE);
        assert_eq!(a.view_marks()[n - 2], Some(Check));
        assert!(!a.settings_changed());
        a.view_menu_action(n - 2, Window::detached());
        assert!(!a.diagnostics);
        assert_eq!(a.view_items()[n - 2].label, super::DIAG_ON);
        assert_eq!(a.view_marks()[n - 2], Some(CelEmpty));
        assert!(a.settings_changed(), "kept in the settings");
        assert!(a.current_settings().no_diagnostics);
        a.view_menu_action(n - 1, Window::detached());
        assert!(a.message.is_some(), "the delete says what it did");
    }
}
