//! The menu "Tween position" in the frame menu opens: how the selected frames of the active
//! layer ease from the first frame's place to the last frame's.

use crate::app::App;
use crate::frame_menu::{MenuKind, OpenMenu};
use pikslar_core::{CelCmd, Ease};
use pikslar_ui::MenuItem;

/// The eases offered, with their labels. Scripts find the items by these names.
pub(crate) const EASES: [(Ease, &str); 4] = [
    (Ease::Linear, "Linear"),
    (Ease::In, "Ease in"),
    (Ease::Out, "Ease out"),
    (Ease::InOut, "Ease in and out"),
];

impl App {
    /// Opens the tween menu where the frame menu was.
    pub(crate) fn open_tween_menu(&mut self, at: (i32, i32)) {
        self.menu = Some(OpenMenu {
            at,
            kind: MenuKind::Tween,
        });
        self.ui_dirty = true;
    }

    /// Every ease. Each can be chosen; a run that cannot be tweened says why when chosen.
    pub(crate) fn tween_items(&self) -> Vec<MenuItem<'static>> {
        EASES
            .iter()
            .map(|&(_, label)| MenuItem {
                label,
                enabled: true,
            })
            .collect()
    }

    /// Tweens the selected frames of the active layer, or refuses in the status line.
    pub(crate) fn tween_menu_action(&mut self, n: usize) {
        let Some(&(ease, _)) = EASES.get(n) else {
            return;
        };
        let Some(layer) = self.editor.layer_id(self.editor.layer) else {
            return;
        };
        let at = self.editor.frame;
        let (from, to) = self.selected_frames().unwrap_or((at, at));
        self.finish_gesture();
        self.playing = false;
        self.run(CelCmd::Tween {
            layer,
            from,
            to,
            ease,
        });
    }
}
