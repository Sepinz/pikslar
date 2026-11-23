//! The menu a right click on the duration opens: time by milliseconds or by frame rate.

use crate::app::App;
use crate::frame_menu::{MenuKind, OpenMenu};
use crate::panel_timeline::ID_READOUT;
use pikslar_core::FrameCmd;
use pikslar_ui::MenuItem;

/// The ways to time the document, with their labels.
const RATES: [(Option<u8>, &str); 5] = [
    (None, "Time in milliseconds"),
    (Some(12), "12 fps"),
    (Some(24), "24 fps"),
    (Some(30), "30 fps"),
    (Some(60), "60 fps"),
];

impl App {
    /// Opens the rate menu if `(x, y)` is on the duration readout.
    pub(crate) fn open_rate_menu(&mut self, x: i32, y: i32) -> bool {
        if self.ui.widget_at(x, y) != ID_READOUT {
            return false;
        }
        self.finish_naming();
        self.playing = false;
        self.menu = Some(OpenMenu {
            at: (x, y),
            kind: MenuKind::Rate,
        });
        self.ui_dirty = true;
        true
    }

    /// Every way; the one in use cannot be chosen again.
    pub(crate) fn rate_items(&self) -> Vec<MenuItem<'static>> {
        let current = self.editor.doc().frame_rate;
        RATES
            .iter()
            .map(|&(fps, label)| MenuItem {
                label,
                enabled: current != fps,
            })
            .collect()
    }

    pub(crate) fn rate_menu_action(&mut self, n: usize) {
        if let Some(&(fps, _)) = RATES.get(n) {
            self.run(FrameCmd::SetRate { fps });
        }
    }
}
