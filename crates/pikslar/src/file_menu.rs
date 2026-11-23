//! The File menu and the Edit menu of the menu bar: rows that do what their shortcut does, with
//! the shortcut written at the right from the live keymap, so rebinding keeps it true.

use crate::app::App;
use crate::frame_menu::Entries;
use crate::shortcuts::Action;
use pikslar_platform::Window;
use std::borrow::Cow;

/// What a row of the File menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileRow {
    Do(Action),
    /// Row `n` of the Open list: Open, a recent file, or Place reference.
    Open(usize),
}

/// The Edit menu's rows: its words, and the action each does.
const EDIT: [(&str, Action); 13] = [
    ("Undo", Action::Undo),
    ("Redo", Action::Redo),
    ("Cut", Action::Cut),
    ("Copy", Action::Copy),
    ("Paste", Action::Paste),
    ("Select all", Action::SelectAll),
    ("Select none", Action::SelectNone),
    ("Invert selection", Action::Invert),
    ("Flip across", Action::FlipHorizontal),
    ("Flip up-down", Action::FlipVertical),
    ("Rotate right", Action::RotateClockwise),
    ("Rotate left", Action::RotateCounter),
    ("Canvas size...", Action::CanvasSize),
];

impl App {
    fn file_rows(&self) -> Vec<(Cow<'static, str>, bool, FileRow)> {
        let key = |text: &str, a: Action| Cow::Owned(self.keymap.tab_hint(text, a));
        let mut rows = vec![(
            key("New sprite...", Action::New),
            true,
            FileRow::Do(Action::New),
        )];
        for (n, (label, enabled)) in self.open_items().into_iter().enumerate() {
            let label = if n == 0 {
                key("Open a file...", Action::Open)
            } else {
                label
            };
            rows.push((label, enabled, FileRow::Open(n)));
        }
        for (text, action) in [
            ("Save", Action::Save),
            ("Save as...", Action::SaveAs),
            ("Export...", Action::Export),
            ("Export sheet...", Action::ExportSheet),
        ] {
            rows.push((key(text, action), true, FileRow::Do(action)));
        }
        rows
    }

    pub(crate) fn file_entries(&self) -> Entries {
        self.file_rows()
            .into_iter()
            .map(|(l, e, _)| (l, e))
            .collect()
    }

    pub(crate) fn file_menu_action(&mut self, n: usize, window: Window) {
        match self.file_rows().get(n) {
            Some(&(_, true, FileRow::Do(action))) => self.run_action(action, window),
            Some(&(_, true, FileRow::Open(k))) => self.open_menu_action(k, window),
            _ => {}
        }
        self.ui_dirty = true;
    }

    /// Whether the Edit row for `action` can be chosen now.
    fn edit_enabled(&self, action: Action) -> bool {
        match action {
            Action::Undo => self.editor.history().undo_depth() > 0,
            Action::Redo => self.editor.history().redo_depth() > 0,
            Action::SelectNone | Action::Invert => self.has_selection(),
            _ => true,
        }
    }

    pub(crate) fn edit_entries(&self) -> Entries {
        EDIT.iter()
            .map(|&(text, action)| {
                let label = Cow::Owned(self.keymap.tab_hint(text, action));
                (label, self.edit_enabled(action))
            })
            .collect()
    }

    pub(crate) fn edit_menu_action(&mut self, n: usize, window: Window) {
        if let Some(&(_, action)) = EDIT.get(n).filter(|&&(_, a)| self.edit_enabled(a)) {
            self.run_action(action, window);
        }
        self.ui_dirty = true;
    }
}

#[cfg(test)]
#[path = "file_menu_tests.rs"]
mod tests;
