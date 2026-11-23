//! The name fields share their editing: one place says which field is open, sends it an edit,
//! and carries Ctrl+C, Ctrl+X and Ctrl+V between it and the system clipboard.

use crate::app::App;
use pikslar_platform::{copy_text, paste_text, Key, Mods, TextPaste, Window};
use pikslar_ui::{edit_for, Edit, TextEdit};

impl App {
    /// The field of the name being typed, if one is open.
    pub(crate) fn naming_field(&self) -> Option<&TextEdit> {
        if let Some(session) = &self.text {
            Some(&session.edit)
        } else if self.timeline.naming.is_some() {
            Some(&self.timeline.name_edit)
        } else if let Some(naming) = &self.layer_naming {
            Some(&naming.edit)
        } else if let Some(naming) = &self.slice_naming {
            Some(&naming.edit)
        } else if let Some(naming) = &self.palette_ui.naming {
            Some(&naming.edit)
        } else if let Some(naming) = &self.tile_ui.naming {
            Some(&naming.edit)
        } else {
            self.brush_ui.naming.as_ref().map(|naming| &naming.edit)
        }
    }

    /// Sends an edit to the name being typed. Returns whether there is one.
    pub(crate) fn edit_naming(&mut self, edit: Edit) -> bool {
        if self.naming_field().is_none() {
            return false;
        }
        if self.text.is_some() {
            self.edit_text(edit);
        } else if self.timeline.naming.is_some() {
            self.edit_name(edit);
        } else if self.layer_naming.is_some() {
            self.edit_layer_name(edit);
        } else if self.slice_naming.is_some() {
            self.edit_slice_name(edit);
        } else if self.palette_ui.naming.is_some() {
            self.edit_group_name(edit);
        } else if self.tile_ui.naming.is_some() {
            self.edit_tileset_name(edit);
        } else {
            self.edit_brush_name(edit);
        }
        true
    }

    /// Handles Ctrl+C, Ctrl+X and Ctrl+V for the name being typed. Returns whether the key was one
    /// of them; any other key is for the caller.
    pub(crate) fn naming_clipboard_key(&mut self, key: Key, mods: Mods, window: Window) -> bool {
        let Some(edit @ (Edit::Copy | Edit::Cut | Edit::Paste)) = edit_for(key, mods) else {
            return false;
        };
        let selected = self.naming_field().map(|f| f.selected_text().to_string());
        match (edit, selected) {
            (_, None) => return false,
            (Edit::Paste, _) => self.paste_into_name(window),
            (_, Some(text)) if text.is_empty() => {}
            (_, Some(text)) => {
                if !copy_text(window, &text) {
                    self.notify("The clipboard was busy".to_string());
                } else if edit == Edit::Cut {
                    self.edit_naming(Edit::Cut);
                }
            }
        }
        true
    }

    fn paste_into_name(&mut self, window: Window) {
        self.report_paste(paste_text(window));
    }

    /// Types what the clipboard gave, or says why there is nothing to type.
    pub(crate) fn report_paste(&mut self, pasted: TextPaste) {
        match pasted {
            TextPaste::Text(text) => self.insert_pasted_text(&text),
            TextPaste::NoText => self.notify("There is no text to paste".to_string()),
            TextPaste::Busy => self.notify("The clipboard was busy".to_string()),
        }
    }
}
