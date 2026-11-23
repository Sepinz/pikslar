//! Ctrl+S is never dropped without a word (B1, section 13, requirement 2). Every state in which
//! the key routing in `keys.rs` keeps a key from the shortcuts is listed here, each with what
//! to do about it; Save then says "Not saved: ..." instead of saving. The state's own dialog or
//! field keeps the keyboard: nothing is saved underneath it and nothing is closed for it.
//!
//! What `key_down` does with Ctrl+S, branch by branch, in its order:
//! - a name being typed (tag, layer, slice, group, tileset, brush): blocked, below;
//! - the text tool's session: lets Ctrl+S through, and saving keeps the text;
//! - the Keyboard shortcuts dialog: blocked, unless it is waiting for a key, which Ctrl+S then
//!   becomes (the person is choosing it);
//! - Canvas size, a plugin review, a menu (over the Scripts panel too), the Scripts panel,
//!   Export and New sprite: blocked, read from the one list the routing reads (`modal.rs`);
//! - the keyboard cursor: takes only arrows and Space, so lets Ctrl+S through;
//! - a held mouse button: blocked until it is let go;
//! - dock and slider keys: never letters, so let Ctrl+S through;
//! - a held Ctrl+S repeating: the first press acted, and repeats say nothing more.

use crate::app::App;
use crate::modal::Modal;
use crate::save_state::PIXELS_NOT_PLACED;
use crate::shortcuts::{Action, Binding};
use pikslar_platform::{Key, Mods, Window};

/// A state that keeps Ctrl+S from saving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SaveBlocker {
    TagName,
    LayerName,
    SliceName,
    GroupName,
    TilesetName,
    BrushName,
    KeysDialog,
    CanvasDialog,
    PluginReview,
    Menu,
    Scripts,
    Export,
    New,
    Pointer,
}

/// Every blocker, in the order the key routing meets them.
#[cfg(test)]
pub(crate) const ALL_BLOCKERS: [SaveBlocker; 14] = [
    SaveBlocker::TagName,
    SaveBlocker::LayerName,
    SaveBlocker::SliceName,
    SaveBlocker::GroupName,
    SaveBlocker::TilesetName,
    SaveBlocker::BrushName,
    SaveBlocker::KeysDialog,
    SaveBlocker::CanvasDialog,
    SaveBlocker::PluginReview,
    SaveBlocker::Menu,
    SaveBlocker::Scripts,
    SaveBlocker::Export,
    SaveBlocker::New,
    SaveBlocker::Pointer,
];

impl SaveBlocker {
    /// What to do so a save can happen, after "Not saved: ".
    pub(crate) fn reason(self) -> &'static str {
        match self {
            SaveBlocker::TagName => "finish naming the tag first (Enter)",
            SaveBlocker::LayerName => "finish naming the layer first (Enter)",
            SaveBlocker::SliceName => "finish naming the slice first (Enter)",
            SaveBlocker::GroupName => "finish naming the group first (Enter)",
            SaveBlocker::TilesetName => "finish naming the tileset first (Enter)",
            SaveBlocker::BrushName => "finish naming the brush first (Enter)",
            SaveBlocker::KeysDialog => "close Keyboard shortcuts first (Esc)",
            SaveBlocker::CanvasDialog => "close Canvas size first (Esc)",
            SaveBlocker::PluginReview => "answer the plugin review first (Esc denies it)",
            SaveBlocker::Scripts => "close the Scripts panel first (Esc)",
            SaveBlocker::Menu => "close the menu first (Esc)",
            SaveBlocker::Export => "close the Export dialog first (Esc)",
            SaveBlocker::New => "close the New sprite dialog first (Esc)",
            SaveBlocker::Pointer => "let go of the mouse button first",
        }
    }

    /// Its name in the diagnostics.
    pub(crate) fn tag(self) -> &'static str {
        match self {
            SaveBlocker::TagName => "tag-name",
            SaveBlocker::LayerName => "layer-name",
            SaveBlocker::SliceName => "slice-name",
            SaveBlocker::GroupName => "group-name",
            SaveBlocker::TilesetName => "tileset-name",
            SaveBlocker::BrushName => "brush-name",
            SaveBlocker::KeysDialog => "keys",
            SaveBlocker::CanvasDialog => "canvas",
            SaveBlocker::PluginReview => "plugin-review",
            SaveBlocker::Scripts => "scripts",
            SaveBlocker::Menu => "menu",
            SaveBlocker::Export => "export",
            SaveBlocker::New => "new",
            SaveBlocker::Pointer => "pointer",
        }
    }
}

impl App {
    /// What keeps a save from happening now, in the order the key routing meets it.
    pub(crate) fn save_blocker(&self) -> Option<SaveBlocker> {
        self.naming_blocker().or_else(|| self.dialog_blocker())
    }

    fn naming_blocker(&self) -> Option<SaveBlocker> {
        [
            (self.timeline.naming.is_some(), SaveBlocker::TagName),
            (self.layer_naming.is_some(), SaveBlocker::LayerName),
            (self.slice_naming.is_some(), SaveBlocker::SliceName),
            (self.palette_ui.naming.is_some(), SaveBlocker::GroupName),
            (self.tile_ui.naming.is_some(), SaveBlocker::TilesetName),
            (self.brush_ui.naming.is_some(), SaveBlocker::BrushName),
        ]
        .into_iter()
        .find_map(|(on, blocker)| on.then_some(blocker))
    }

    /// The dialog, panel or menu with the keyboard, from the one list the key routing reads
    /// (`modal.rs`), then a held mouse button.
    fn dialog_blocker(&self) -> Option<SaveBlocker> {
        self.modal()
            .map(Modal::blocker)
            .or_else(|| self.pointer_busy().then_some(SaveBlocker::Pointer))
    }

    /// True for a key the keymap gives to Save or Save as, as `letter_key` reads it. With Ctrl,
    /// always. Without, only when a menu or dialog that takes no text has the keyboard: there
    /// the letter would otherwise be dropped without a word. In a name or a text field that
    /// letter is typed, as any letter is there.
    fn is_save_key(&self, key: Key, mods: Mods) -> bool {
        let Key::Char(letter) = key else {
            return false;
        };
        let plain_ok = !mods.alt && self.modal().is_some_and(Modal::takes_no_text);
        if !mods.ctrl && !plain_ok {
            return false;
        }
        let pressed = Binding {
            letter,
            ctrl: mods.ctrl,
            shift: mods.shift,
        };
        matches!(
            self.keymap.action_for(pressed),
            Some(Action::Save | Action::SaveAs)
        )
    }

    /// Takes a Save key that something would keep from saving, and says so. True if it did.
    /// The Keyboard shortcuts dialog waiting for a key gets it: that is the key being chosen.
    pub(crate) fn save_key_held_back(&mut self, key: Key, mods: Mods, repeat: bool) -> bool {
        if !self.is_save_key(key, mods)
            || self
                .keys_dialog
                .as_ref()
                .is_some_and(|d| d.waiting.is_some())
        {
            return false;
        }
        let Some(blocker) = self.save_blocker() else {
            return false;
        };
        if !repeat {
            self.save_blocked(blocker.reason().to_string());
        }
        true
    }

    /// Save or Save as, from a key or the File menu: blocked with a reason, or done.
    pub(crate) fn request_save(&mut self, window: Window, choose_path: bool) {
        match self.save_blocker() {
            Some(blocker) => self.save_blocked(blocker.reason().to_string()),
            None => {
                self.start_save(window, choose_path);
            }
        }
    }

    /// "Not saved" goes once what was in the way has gone. Moved pixels that could not be put
    /// down stay said while they are still not put down, whatever else comes and goes.
    pub(crate) fn recheck_save_blocked(&mut self) {
        let Some(why) = &self.save.blocked else {
            return;
        };
        let still = match why == PIXELS_NOT_PLACED {
            true => self.editor.is_floating(),
            false => self.save_blocker().is_some_and(|b| b.reason() == why),
        };
        if !still {
            self.save_unblocked();
        }
    }
}

#[cfg(test)]
#[path = "save_keys_tests.rs"]
mod tests;
