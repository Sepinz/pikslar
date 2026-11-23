//! The menu of blend modes, opened from the active layer's blend button.
//!
//! It offers the modes the document's blending draws: Pikslar's nine, or with
//! Aseprite-compatible blending Aseprite's nineteen. A layer whose mode the document does not
//! draw as itself keeps it, shown at the end of the menu as the mode in use.

use crate::app::App;
use crate::frame_menu::{MenuKind, OpenMenu};
use pikslar_core::{BlendMode, LayerCmd};
use pikslar_ui::MenuItem;

impl App {
    /// Opens the blend menu at `at`, for the active layer.
    pub(crate) fn open_blend_menu(&mut self, at: (i32, i32)) {
        self.menu = Some(OpenMenu {
            at,
            kind: MenuKind::Blend,
        });
        self.ui_dirty = true;
    }

    /// The modes the menu lists, in order: the document's, then the one in use if it is not
    /// among them.
    fn blend_choices(&self) -> Vec<BlendMode> {
        let doc = self.editor.doc();
        let current = doc.layers()[self.editor.layer].blend;
        let mut modes = doc.blending.modes().to_vec();
        if !modes.contains(&current) {
            modes.push(current);
        }
        modes
    }

    /// The document's modes; the one in use cannot be chosen again.
    pub(crate) fn blend_items(&self) -> Vec<MenuItem<'static>> {
        let current = self.editor.doc().layers()[self.editor.layer].blend;
        self.blend_choices()
            .into_iter()
            .map(|mode| MenuItem {
                label: mode.name(),
                enabled: mode != current,
            })
            .collect()
    }

    pub(crate) fn blend_menu_action(&mut self, n: usize) {
        let (Some(blend), Some(layer)) = (
            self.blend_choices().get(n).copied(),
            self.editor.layer_id(self.editor.layer),
        ) else {
            return;
        };
        self.run(LayerCmd::SetBlend { layer, blend });
    }
}

#[cfg(test)]
mod tests {
    use crate::test_app::app;
    use pikslar_core::command::CanvasCmd;
    use pikslar_core::{BlendMode, Blending};

    #[test]
    fn every_mode_is_offered_and_choosing_one_is_one_step() {
        let mut a = app(1);
        a.open_blend_menu((100, 100));
        let items = a.blend_items();
        assert_eq!(items.len(), 9);
        assert!(!items[0].enabled, "Normal is in use");
        assert_eq!(items[2].label, "Screen");
        let pushes = a.editor.history().pushes();
        a.blend_menu_action(8);
        let layer = &a.editor.doc().layers()[a.editor.layer];
        assert_eq!(layer.blend, BlendMode::Difference);
        assert_eq!(a.editor.history().pushes(), pushes + 1);
        a.blend_menu_action(99);
        assert_eq!(
            a.editor.history().pushes(),
            pushes + 1,
            "no such mode, no step"
        );
    }

    #[test]
    fn aseprite_blending_offers_aseprites_modes_and_pikslars_keeps_one_in_use() {
        let mut a = app(1);
        a.run(CanvasCmd::SetBlending {
            blending: Blending::Aseprite,
        });
        let items = a.blend_items();
        assert_eq!(items.len(), 19);
        assert_eq!(items[18].label, "Divide");
        a.blend_menu_action(14);
        let blend = |a: &crate::app::App| a.editor.doc().layers()[a.editor.layer].blend;
        assert_eq!(blend(&a), BlendMode::Hue);
        a.run(CanvasCmd::SetBlending {
            blending: Blending::Pikslar,
        });
        let items = a.blend_items();
        assert_eq!(items.len(), 10, "Pikslar's nine and the mode in use");
        assert_eq!((items[9].label, items[9].enabled), ("Hue", false));
        a.blend_menu_action(1);
        assert_eq!(blend(&a), BlendMode::Multiply);
        assert_eq!(
            a.blend_items().len(),
            9,
            "once changed, it is not offered again"
        );
    }
}
