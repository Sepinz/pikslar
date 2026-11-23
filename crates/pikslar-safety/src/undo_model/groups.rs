//! Layer groups: grouping, ungrouping and stepping are undone like any step; folding, like
//! visibility, changes the file but never becomes one. After every step the list must keep
//! its order and every group must stay empty of pixels.

use super::{bad, Model};
use crate::go;
use pikslar_core::{Document, LayerCmd, LayerKind};

impl Model {
    /// One random group step on the active layer.
    pub(super) fn grouping(&mut self) {
        let index = self.editor.layer;
        let doc = self.editor.doc();
        let target = match doc.layers()[index].kind {
            LayerKind::Group => Some(index),
            _ => doc.parent(index),
        };
        let layer = self.editor.layer_id(index).expect("layer");
        let command = match (self.rng.below(4), target) {
            (0, _) => LayerCmd::Group { layer },
            (1, Some(group)) => LayerCmd::Ungroup {
                layer: self.editor.layer_id(group).expect("group"),
            },
            (1, None) => LayerCmd::Group { layer },
            (2, _) => LayerCmd::Step { layer, up: true },
            _ => LayerCmd::Step { layer, up: false },
        };
        go(&mut self.editor, command);
    }

    /// Folds or opens the group the active layer is or is in. Never a step: every remembered
    /// state takes the fold as it is.
    pub(super) fn toggle_fold(&mut self) -> Result<(), String> {
        let index = self.editor.layer;
        let doc = self.editor.doc();
        let Some(group) = (match doc.layers()[index].kind {
            LayerKind::Group => Some(index),
            _ => doc.parent(index),
        }) else {
            return Ok(());
        };
        let (id, folded) = {
            let layer = &doc.layers()[group];
            (layer.id(), !layer.folded)
        };
        let layer = self.editor.layer_id(group).expect("group");
        let pushes = self.editor.history().pushes();
        go(&mut self.editor, LayerCmd::SetFolded { layer, folded });
        if self.editor.history().pushes() != pushes {
            return bad("folding a group became a step".to_owned());
        }
        for layer in self.timeline.iter_mut().flat_map(|s| s.doc.layers_mut()) {
            if layer.id() == id {
                layer.folded = folded;
            }
        }
        self.compare("folding a group")
    }
}

/// The list keeps its order, nothing is too deep, and a group has no pixels.
pub(super) fn check_groups(doc: &Document) -> Result<(), String> {
    if let Err(e) = doc.check_groups() {
        return bad(format!("the layer groups are out of order: {e:?}"));
    }
    for (index, layer) in doc.layers().iter().enumerate() {
        let blank = layer.cels().iter().all(|c| c.is_blank());
        if layer.kind == LayerKind::Group && !blank {
            return bad(format!("layer {index}: a group has pixels"));
        }
    }
    Ok(())
}
