//! Slices in the app: made from the selection, found under the pointer, changed by the menu a
//! right click opens, and renamed in place. Whether they show is a view setting, never saved.

use crate::app::{App, Gesture, ToolChoice};
use crate::frame_menu::{MenuKind, OpenMenu};
use pikslar_core::{Slice, SliceCmd, SliceRect, MAX_SLICE_NAME};
use pikslar_platform::{Key, Mods};
use pikslar_ui::{edit_for, Edit, MenuItem, TextEdit};

/// A new slice's outline color, as RGBA.
const DEFAULT_COLOR: u32 = 0xff8c_00ff;
const SLICE_HINT: &str = "Ctrl right click for the slice menu";
const RENAME: &str = "Rename slice";
const SET_PIVOT: &str = "Set pivot here";
const CLEAR_PIVOT: &str = "Clear pivot";
const REMOVE: &str = "Remove slice";

/// The slice whose name is being typed, and the name so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SliceNaming {
    pub index: usize,
    pub edit: TextEdit,
}

impl App {
    /// The first "Slice N" no slice is called.
    fn default_slice_name(&self) -> String {
        let slices = &self.editor.doc().slices;
        (1..)
            .map(|n| format!("Slice {n}"))
            .find(|name| slices.iter().all(|s| &s.name != name))
            .unwrap_or_default()
    }

    /// Adds a slice with the selection's bounds, or says there is nothing to make it from.
    /// Returns whether it was added.
    pub(crate) fn make_slice_from_selection(&mut self) -> bool {
        let doc = self.editor.doc();
        let (cw, ch) = (doc.width as i32, doc.height as i32);
        let bounds = self
            .editor
            .selection()
            .map(|m| m.within(cw, ch).trimmed().bounds());
        let Some((x, y, w, h)) = bounds.filter(|&(_, _, w, h)| w > 0 && h > 0) else {
            self.notify("Select something to make a slice of".to_string());
            return false;
        };
        let slice = Slice {
            name: self.default_slice_name(),
            color: DEFAULT_COLOR,
            bounds: SliceRect {
                x,
                y,
                width: w as u32,
                height: h as u32,
            },
            pivot: None,
            center: None,
        };
        let made = self.run(SliceCmd::Add {
            slice: Box::new(slice),
        });
        // A slice made is a slice wanted in view.
        self.show_slices |= made.is_some();
        made.is_some()
    }

    /// Shows or hides the slices over the canvas.
    pub(crate) fn toggle_slices(&mut self) {
        self.show_slices = !self.show_slices;
        self.ui_dirty = true;
    }

    /// The slice drawn on top at the screen point, and the document pixel there.
    pub(crate) fn slice_at(&self, x: i32, y: i32) -> Option<(usize, (i32, i32))> {
        if !self.show_slices {
            return None;
        }
        let (px, py) = self.view.to_doc(x, y);
        let reaches = |from: i32, len: u32, v: i32| {
            v >= from && i64::from(v) < i64::from(from) + i64::from(len)
        };
        let slices = &self.editor.doc().slices;
        // The smallest wins, so a slice inside a larger one stays reachable; of equal size, the
        // one drawn later.
        let area = |s: &Slice| u64::from(s.bounds.width) * u64::from(s.bounds.height);
        let top = slices
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                reaches(s.bounds.x, s.bounds.width, px) && reaches(s.bounds.y, s.bounds.height, py)
            })
            .min_by_key(|&(index, s)| (area(s), std::cmp::Reverse(index)));
        top.map(|(index, _)| (index, (px, py)))
    }

    /// What the status line says while the pointer rests on a visible slice with a drawing tool,
    /// where the slice menu needs Ctrl.
    pub(crate) fn slice_hint(&self) -> Option<&'static str> {
        let (x, y) = self.ui.mouse;
        let free = self.menu.is_none() && !self.panel_open() && self.gesture == Gesture::None;
        let painting = matches!(self.tool, ToolChoice::Paint(_));
        let over = self.on_canvas(x, y) && self.ui.widget_at(x, y) == 0;
        (free && painting && over && self.slice_at(x, y).is_some()).then_some(SLICE_HINT)
    }

    /// Opens the slice menu if a right click at `(x, y)` is inside a slice and no tool wants it:
    /// the selection tools do not use the right button, and with a drawing tool it erases, so
    /// there the menu comes with Ctrl.
    pub(crate) fn open_slice_menu(&mut self, x: i32, y: i32, mods: Mods) -> bool {
        let selecting = !matches!(self.tool, ToolChoice::Paint(_));
        // Space held means the right button pans.
        let wanted = !self.space_held
            && (selecting || mods.ctrl)
            && self.on_canvas(x, y)
            && self.ui.widget_at(x, y) == 0;
        let Some((index, at)) = self.slice_at(x, y).filter(|_| wanted) else {
            return false;
        };
        self.finish_naming();
        self.playing = false;
        self.menu = Some(OpenMenu {
            at: (x, y),
            kind: MenuKind::Slice { index, at },
        });
        self.ui_dirty = true;
        true
    }

    /// What the menu offers for the slice, clicked at the document pixel `at`.
    pub(crate) fn slice_items(&self, index: usize, at: (i32, i32)) -> Vec<MenuItem<'static>> {
        let found = self.editor.doc().slices.get(index);
        let pivot = found.and_then(|s| s.pivot);
        let here = found.map(|s| (at.0 - s.bounds.x, at.1 - s.bounds.y));
        let item = |label, enabled| MenuItem { label, enabled };
        vec![
            item(RENAME, true),
            item(SET_PIVOT, pivot != here),
            item(CLEAR_PIVOT, pivot.is_some()),
            item(REMOVE, true),
        ]
    }

    pub(crate) fn slice_menu_action(&mut self, index: usize, at: (i32, i32), n: usize) {
        let Some(slice) = self.editor.doc().slices.get(index).cloned() else {
            return;
        };
        let pivot = |pivot| SliceCmd::Set {
            index,
            slice: Box::new(Slice {
                pivot,
                ..slice.clone()
            }),
        };
        match n {
            0 => self.start_slice_naming(index),
            1 => {
                let (x, y) = (at.0 - slice.bounds.x, at.1 - slice.bounds.y);
                self.run(pivot(Some((x, y))));
            }
            2 => {
                self.run(pivot(None));
            }
            3 => {
                self.run(SliceCmd::Remove { index });
            }
            _ => {}
        }
    }

    pub(crate) fn start_slice_naming(&mut self, index: usize) {
        let Some(slice) = self.editor.doc().slices.get(index) else {
            return;
        };
        let mut edit = TextEdit::new(&slice.name, MAX_SLICE_NAME);
        edit.apply(Edit::End { extend: false });
        self.finish_naming();
        self.show_slices = true;
        self.slice_naming = Some(SliceNaming { index, edit });
        self.ui_dirty = true;
    }

    /// Keeps the name typed, as one step, unless it is empty or the same.
    pub(crate) fn finish_slice_naming(&mut self) {
        let Some(naming) = self.slice_naming.take() else {
            return;
        };
        self.ui_dirty = true;
        let name = naming.edit.text().to_string();
        let Some(slice) = self.editor.doc().slices.get(naming.index).cloned() else {
            return;
        };
        if !name.is_empty() && name != slice.name {
            let slice = Box::new(Slice { name, ..slice });
            self.run(SliceCmd::Set {
                index: naming.index,
                slice,
            });
        }
    }

    /// While a slice is being named, the keyboard belongs to the name.
    pub(crate) fn slice_naming_key(&mut self, key: Key, mods: Mods) {
        let edit = match key {
            Key::Enter => return self.finish_slice_naming(),
            Key::Escape => {
                self.slice_naming = None;
                self.ui_dirty = true;
                return;
            }
            // Characters come as text, from the keyboard layout.
            _ => match edit_for(key, mods) {
                Some(edit) => edit,
                None => return,
            },
        };
        self.edit_slice_name(edit);
    }

    pub(crate) fn edit_slice_name(&mut self, edit: Edit) {
        let Some(naming) = &mut self.slice_naming else {
            return;
        };
        self.ui_dirty = true;
        let full = naming.edit.text().chars().count() >= MAX_SLICE_NAME;
        if !naming.edit.apply(edit) && matches!(edit, Edit::Insert(_)) && full {
            self.notify(format!("Slice names stop at {MAX_SLICE_NAME} letters"));
        }
    }
}

#[cfg(test)]
#[path = "slice_ui_tests.rs"]
mod tests;
