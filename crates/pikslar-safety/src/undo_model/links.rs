//! Links across layers in the random runs: linking cels of two layers, moving one cel of a
//! link, converting, merging and removing layers in links, and drawing on a drawing shared with
//! a locked layer. Every change must be one step or nothing, and the links keep their rules.

use super::{bad, Model};
use crate::go;
use pikslar_core::{
    CelCmd, CelDrag, ColorMode, Command, DragKind, DrawCmd, Landing, LayerCmd, LimitError, Tool,
};

impl Model {
    /// One change that links across layers bring, chosen at random.
    pub(super) fn link_step(&mut self) -> Result<(), String> {
        match self.rng.below(7) {
            0 | 1 => {
                let before = self.editor.history().pushes();
                self.edit("link across layers", Self::link_across)?;
                self.counts.links += u64::from(self.editor.history().pushes() != before);
                Ok(())
            }
            2 => self.edit("move one cel", Self::move_one),
            3 => self.edit("convert a linked layer", |m| {
                m.on_linked_layer(Self::convert)
            }),
            4 => self.edit("merge a linked layer", |m| m.on_linked_layer(Self::merge)),
            5 => self.edit("remove a linked layer", |m| m.on_linked_layer(Self::remove)),
            _ => self.draw_with_a_member_locked(),
        }
    }

    /// Links one cel, or a small block, to another layer, as an Alt drag does.
    fn link_across(&mut self) {
        let (layers, frames) = (
            self.editor.doc().layers().len(),
            self.editor.doc().frames.len(),
        );
        let from = (self.rng.below(layers), self.rng.below(frames));
        let to = (self.rng.below(layers), self.rng.below(frames));
        let wide = self.rng.below(2);
        let drag = CelDrag {
            layers: (from.0, from.0),
            frames: (from.1, (from.1 + wide).min(frames - 1)),
            to_layer: to.0,
            to_frame: to.1,
            kind: DragKind::Link,
            landing: match self.rng.below(4) {
                0 => Landing::Insert,
                _ => Landing::Overwrite,
            },
        };
        // Refused for modes, kinds, locks or room; then nothing changes.
        let _ = self.editor.apply(CelCmd::Drag(drag));
    }

    /// Moves the active cel alone, out of step with the rest of its link.
    fn move_one(&mut self) {
        let layer = self.editor.layer_id(self.editor.layer).expect("layer");
        let frame = self.editor.frame;
        for _ in 0..1 + self.rng.below(3) {
            let (dx, dy) = (self.rng.between(-3, 3), self.rng.between(-3, 3));
            let _ = self.editor.apply(CelCmd::MoveOne {
                layer,
                frame,
                dx,
                dy,
            });
        }
    }

    /// Runs a change on a layer with a cel linked to another layer, if there is one.
    fn on_linked_layer(&mut self, change: fn(&Model, usize) -> Command) {
        let doc = self.editor.doc();
        let table = doc.link_table();
        let across: Vec<usize> = table
            .all()
            .iter()
            .filter(|m| table.reaches_across(m.id))
            .map(|m| usize::from(m.layer))
            .collect();
        if across.is_empty() {
            return;
        }
        let index = across[self.rng.below(across.len())];
        let command = change(self, index);
        let _ = self.editor.apply(command);
    }

    fn convert(&self, index: usize) -> Command {
        let layer = self.editor.layer_id(index).expect("layer");
        let mode = match self.editor.doc().layers()[index].mode {
            ColorMode::Rgba => ColorMode::Palette,
            ColorMode::Palette => ColorMode::Rgba,
        };
        LayerCmd::SetMode { layer, mode }.into()
    }

    fn merge(&self, index: usize) -> Command {
        let layer = self.editor.layer_id(index).expect("layer");
        LayerCmd::MergeDown { layer }.into()
    }

    fn remove(&self, index: usize) -> Command {
        let layer = self.editor.layer_id(index).expect("layer");
        LayerCmd::Remove { layer }.into()
    }

    /// Locks another layer of the active cel's link, draws on the cel, which must be refused
    /// and change nothing, then unlocks it. Locks are never undone, so no state changes.
    fn draw_with_a_member_locked(&mut self) -> Result<(), String> {
        let (layer, frame) = (self.editor.layer, self.editor.frame);
        let doc = self.editor.doc();
        let other = doc
            .link_members(layer, frame)
            .into_iter()
            .find(|&(l, _)| l != layer && !doc.locked_through(l));
        let unlocked = self.editor.paint_refusal(layer).is_none();
        let Some((other, _)) = other.filter(|_| unlocked) else {
            return Ok(());
        };
        let id = self.editor.layer_id(other).expect("layer");
        go(
            &mut self.editor,
            LayerCmd::SetLocked {
                layer: id,
                locked: true,
            },
        );
        let before = (self.editor.revision(), self.editor.history().pushes());
        let (x, y) = self.point();
        let stroke = DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(x, y), (x + 2, y + 1)],
        };
        let result = self.editor.apply(stroke);
        let after = (self.editor.revision(), self.editor.history().pushes());
        go(
            &mut self.editor,
            LayerCmd::SetLocked {
                layer: id,
                locked: false,
            },
        );
        if result != Err(LimitError::SharedLocked) {
            return bad(format!(
                "drawing shared with a locked layer gave {result:?}"
            ));
        }
        if before != after {
            return bad("drawing shared with a locked layer changed the file".to_owned());
        }
        self.counts.refused += 1;
        self.compare("drawing shared with a locked layer")
    }
}
