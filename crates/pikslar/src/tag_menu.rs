//! The menu a right click on a tag opens: how many times the tag plays.

use crate::app::App;
use crate::frame_menu::{MenuKind, OpenMenu};
use crate::timeline::{ID_TAG, TAG_IDS};
use pikslar_core::TagCmd;
use pikslar_ui::MenuItem;

/// The counts offered, 0 for forever, with their labels.
const REPEATS: [(u16, &str); 6] = [
    (0, "Play forever"),
    (1, "Play once"),
    (2, "Play twice"),
    (3, "Play 3 times"),
    (4, "Play 4 times"),
    (8, "Play 8 times"),
];

impl App {
    /// Opens the tag menu if `(x, y)` is on a tag of the tag row.
    pub(crate) fn open_tag_menu(&mut self, x: i32, y: i32) -> bool {
        let id = self.ui.widget_at(x, y);
        let tag = id.wrapping_sub(ID_TAG) as usize;
        if !TAG_IDS.contains(&id) || tag >= self.editor.doc().tags.len() {
            return false;
        }
        self.finish_naming();
        self.playing = false;
        self.menu = Some(OpenMenu {
            at: (x, y),
            kind: MenuKind::Tag(tag),
        });
        self.ui_dirty = true;
        true
    }

    /// Every count; the one in use cannot be chosen again.
    pub(crate) fn tag_items(&self, tag: usize) -> Vec<MenuItem<'static>> {
        let current = self.editor.doc().tags.get(tag).map(|t| t.repeat);
        REPEATS
            .iter()
            .map(|&(repeat, label)| MenuItem {
                label,
                enabled: current != Some(repeat),
            })
            .collect()
    }

    pub(crate) fn tag_menu_action(&mut self, tag: usize, n: usize) {
        let Some(&(repeat, _)) = REPEATS.get(n) else {
            return;
        };
        if tag < self.editor.doc().tags.len() {
            self.run(TagCmd::SetRepeat { tag, repeat });
        }
    }
}
