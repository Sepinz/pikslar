//! The dialogs, panels and menus that take the keyboard while they are open, in the order the
//! key routing gives it to them. `key_down` sends keys to [`App::modal`], `panel_open` is
//! whether there is one, and Save names it as what is in the way (`save_keys.rs`): all three
//! read this one list, so a dialog added here can never swallow Ctrl+S without a word, and one
//! not added here never gets the keyboard.

use crate::app::App;
use crate::save_keys::SaveBlocker;

/// What has the keyboard, of the dialogs, panels and menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Modal {
    KeysDialog,
    CanvasDialog,
    PluginReview,
    /// A menu, over the Scripts panel too.
    Menu,
    Scripts,
    Export,
    New,
}

impl Modal {
    /// Every one, in the order the key routing meets them.
    pub(crate) const ALL: [Modal; 7] = [
        Modal::KeysDialog,
        Modal::CanvasDialog,
        Modal::PluginReview,
        Modal::Menu,
        Modal::Scripts,
        Modal::Export,
        Modal::New,
    ];

    /// True for one with no text field: a letter pressed there is a command, never text.
    pub(crate) fn takes_no_text(self) -> bool {
        matches!(
            self,
            Modal::KeysDialog | Modal::PluginReview | Modal::Menu | Modal::Export | Modal::New
        )
    }

    /// How Save names it.
    pub(crate) fn blocker(self) -> SaveBlocker {
        match self {
            Modal::KeysDialog => SaveBlocker::KeysDialog,
            Modal::CanvasDialog => SaveBlocker::CanvasDialog,
            Modal::PluginReview => SaveBlocker::PluginReview,
            Modal::Menu => SaveBlocker::Menu,
            Modal::Scripts => SaveBlocker::Scripts,
            Modal::Export => SaveBlocker::Export,
            Modal::New => SaveBlocker::New,
        }
    }
}

impl App {
    fn is_open(&self, modal: Modal) -> bool {
        match modal {
            Modal::KeysDialog => self.keys_dialog.is_some(),
            Modal::CanvasDialog => self.canvas_dialog.is_some(),
            Modal::PluginReview => self.plugins.review.is_some(),
            Modal::Menu => self.menu.is_some(),
            Modal::Scripts => self.scripts.open,
            Modal::Export => self.export_dialog.is_some(),
            Modal::New => self.new_dialog.is_some(),
        }
    }

    /// What has the keyboard now, if a dialog, panel or menu does.
    pub(crate) fn modal(&self) -> Option<Modal> {
        Modal::ALL.into_iter().find(|m| self.is_open(*m))
    }
}
