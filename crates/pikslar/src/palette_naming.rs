//! Renaming a palette group or ramp: the menu opens the name in the palette header for typing.
//! Enter keeps it as one step of undo, Escape or an empty name leaves it as it was, and a press
//! anywhere else keeps it. The same route as a slice's or a layer's name.

use crate::app::App;
use pikslar_core::{PaletteCmd, StepCmd, MAX_GROUP_NAME};
use pikslar_platform::{Key, Mods};
use pikslar_ui::{edit_for, Edit, TextEdit};

/// The group whose name is being typed, and the name so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GroupNaming {
    pub group: usize,
    pub edit: TextEdit,
}

impl App {
    pub(crate) fn start_group_naming(&mut self, group: usize) {
        let Some(found) = self.editor.doc().colors.groups().get(group) else {
            return;
        };
        let mut edit = TextEdit::new(&found.name, MAX_GROUP_NAME);
        edit.apply(Edit::End { extend: false });
        self.finish_naming();
        self.finish_slice_naming();
        self.palette_ui.naming = Some(GroupNaming { group, edit });
        self.ui_dirty = true;
    }

    /// Keeps the name typed, as one step, unless it is empty or the same.
    pub(crate) fn finish_group_naming(&mut self) {
        let Some(naming) = self.palette_ui.naming.take() else {
            return;
        };
        self.ui_dirty = true;
        let name = naming.edit.text().to_string();
        if name.is_empty() {
            return;
        }
        let group = naming.group;
        self.run(PaletteCmd::RenameGroup { group, name });
        // The next rename of this group is a step of its own.
        self.run(StepCmd::EndDrag);
    }

    /// While a group is being named, the keyboard belongs to the name.
    pub(crate) fn group_naming_key(&mut self, key: Key, mods: Mods) {
        let edit = match key {
            Key::Enter => return self.finish_group_naming(),
            Key::Escape => {
                self.palette_ui.naming = None;
                self.ui_dirty = true;
                return;
            }
            // Characters come as text, from the keyboard layout.
            _ => match edit_for(key, mods) {
                Some(edit) => edit,
                None => return,
            },
        };
        self.edit_group_name(edit);
    }

    pub(crate) fn edit_group_name(&mut self, edit: Edit) {
        let Some(naming) = &mut self.palette_ui.naming else {
            return;
        };
        self.ui_dirty = true;
        let full = naming.edit.text().chars().count() >= MAX_GROUP_NAME;
        if !naming.edit.apply(edit) && matches!(edit, Edit::Insert(_)) && full {
            self.notify(format!("Group names stop at {MAX_GROUP_NAME} letters"));
        }
    }
}
