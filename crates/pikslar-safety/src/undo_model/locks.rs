//! Locking a layer: saved in the file, never undone, and every drawing on it refused.

use super::{bad, Model};
use crate::go;
use pikslar_core::{DrawCmd, FloatingCmd, LayerCmd, LimitError, Tool};

impl Model {
    /// Locks or unlocks the active layer. While locked, a stroke on it must be refused and
    /// change nothing. Like visibility, every remembered state takes the lock as it is.
    pub(super) fn toggle_lock(&mut self) -> Result<(), String> {
        // Locking puts floating pixels down; done first, so that step is recorded as one.
        self.edit("putting floating pixels down", |m| {
            go(&mut m.editor, FloatingCmd::Anchor)
        })?;
        let index = self.editor.layer;
        let (id, locked) = {
            let layer = &self.editor.doc().layers()[index];
            (layer.id(), !layer.locked)
        };
        let layer = self.editor.layer_id(index).expect("layer");
        let (pushes, revision) = (self.editor.history().pushes(), self.editor.revision());
        go(&mut self.editor, LayerCmd::SetLocked { layer, locked });
        if self.editor.history().pushes() != pushes {
            return bad("locking a layer became a step".to_owned());
        }
        if self.editor.revision() == revision {
            return bad("locking a layer did not mark the file changed".to_owned());
        }
        for layer in self.timeline.iter_mut().flat_map(|s| s.doc.layers_mut()) {
            if layer.id() == id {
                layer.locked = locked;
            }
        }
        self.compare("locking a layer")?;
        if locked {
            self.refused_on_lock()?;
        }
        self.compare_picture("locking a layer")
    }

    fn refused_on_lock(&mut self) -> Result<(), String> {
        let (x, y) = self.point();
        let before = (self.editor.revision(), self.editor.history().pushes());
        let stroke = DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(x, y), (x + 3, y)],
        };
        let result = self.editor.apply(stroke);
        if result != Err(LimitError::Locked) {
            return bad(format!("a stroke on a locked layer gave {result:?}"));
        }
        if before != (self.editor.revision(), self.editor.history().pushes()) {
            return bad("a stroke on a locked layer changed the file".to_owned());
        }
        self.counts.refused += 1;
        self.compare("a stroke on a locked layer")
    }
}
