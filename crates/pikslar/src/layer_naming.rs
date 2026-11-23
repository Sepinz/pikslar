//! Renaming a layer: a double click on its row opens the name for typing. Enter keeps it as one
//! step of undo, Escape or an empty name leaves it as it was, and a press anywhere else keeps it.

use crate::app::App;
use crate::panel_right::ID_LAYER_ROW;
use pikslar_core::command::MAX_LAYER_NAME;
use pikslar_core::LayerCmd;
use pikslar_platform::{Key, Mods};
use pikslar_ui::{edit_for, Edit, TextEdit};
use std::time::{Duration, Instant};

/// Two clicks on one row this close together are a double click, as Windows has it by default.
pub(crate) const DOUBLE_CLICK: Duration = Duration::from_millis(500);

/// The layer whose name is being typed, and the name so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LayerNaming {
    pub layer: usize,
    pub edit: TextEdit,
}

impl App {
    /// A click on a layer's row. The second of two quick ones opens its name.
    pub(crate) fn layer_row_clicked(&mut self, index: usize) {
        let now = Instant::now();
        let double = matches!(
            self.last_row_click,
            Some((row, at)) if row == index && now.duration_since(at) <= DOUBLE_CLICK
        );
        self.last_row_click = (!double).then_some((index, now));
        if double {
            self.start_layer_naming(index);
        }
    }

    pub(crate) fn start_layer_naming(&mut self, index: usize) {
        let Some(layer) = self.editor.doc().layers().get(index) else {
            return;
        };
        let edit = TextEdit::new(&layer.name, MAX_LAYER_NAME);
        self.finish_naming();
        self.layer_naming = Some(LayerNaming { layer: index, edit });
        self.ui_dirty = true;
    }

    /// Keeps the name typed, as one step, unless it is empty or the same.
    pub(crate) fn finish_layer_naming(&mut self) {
        let Some(naming) = self.layer_naming.take() else {
            return;
        };
        self.ui_dirty = true;
        if let Some(layer) = self.editor.layer_id(naming.layer) {
            let name = naming.edit.text().to_string();
            self.run(LayerCmd::Rename { layer, name });
        }
    }

    /// A press anywhere but on the row being named keeps the name.
    pub(crate) fn finish_layer_naming_unless_on_row(&mut self, x: i32, y: i32) {
        let row = self
            .layer_naming
            .as_ref()
            .map(|n| ID_LAYER_ROW + n.layer as u32);
        if row.is_some_and(|row| self.ui.widget_at(x, y) != row) {
            self.finish_layer_naming();
        }
    }

    /// While a layer is being named, the keyboard belongs to the name.
    pub(crate) fn layer_naming_key(&mut self, key: Key, mods: Mods) {
        let edit = match key {
            Key::Enter => return self.finish_layer_naming(),
            Key::Escape => {
                self.layer_naming = None;
                self.ui_dirty = true;
                return;
            }
            // Characters come as text, from the keyboard layout.
            _ => match edit_for(key, mods) {
                Some(edit) => edit,
                None => return,
            },
        };
        self.edit_layer_name(edit);
    }

    pub(crate) fn edit_layer_name(&mut self, edit: Edit) {
        let Some(naming) = &mut self.layer_naming else {
            return;
        };
        self.ui_dirty = true;
        let full = naming.edit.text().chars().count() >= MAX_LAYER_NAME;
        if !naming.edit.apply(edit) && matches!(edit, Edit::Insert(_)) && full {
            self.notify(format!("Layer names stop at {MAX_LAYER_NAME} letters"));
        }
    }
}
