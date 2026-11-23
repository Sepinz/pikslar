//! Many commands kept as one undo step, or none at all: what a script run needs.
//!
//! The commands run, through [`Editor::apply`] as always, on an editor of their own holding a
//! copy of the document, which shares every drawing until one changes. If they all succeed, the
//! document takes on the copy in one structural step, so undo, redo and the memory history
//! keeps work as for any other step. If any fails, the copy is dropped: the document, its
//! history, redo and revision are as they were.

use crate::command::{LayerId, StepCmd};
use crate::document::{Document, LimitError};
use crate::editor::{Damage, Editor};
use crate::layer::Layer;

/// How the artist looks at a layer: never part of an undo step.
type View = (u32, bool, bool, bool);

fn view_of(layer: &Layer) -> View {
    (layer.id(), layer.visible, layer.locked, layer.folded)
}

impl Editor {
    /// Runs `body` on an editor of its own, and keeps everything it did as one undo step if it
    /// returns `Ok`, or nothing at all if it returns an error. A body that changes nothing
    /// records nothing.
    ///
    /// The body's editor starts with this one's document, active layer, frame and color, no
    /// selection and no floating pixels. Refused with [`LimitError::Busy`], changing nothing,
    /// while a stroke (of pixels or map cells) is in progress or pixels are floating, here or
    /// when the body returns.
    ///
    /// Visibility, locks and folding are never undone, as for the commands; changing them
    /// still counts as a change to the file.
    pub fn transaction<T, E: From<LimitError>>(
        &mut self,
        body: impl FnOnce(&mut Editor) -> Result<T, E>,
    ) -> Result<T, E> {
        if self.is_busy() {
            return Err(LimitError::Busy.into());
        }
        // Color ids the table holds as the run starts stay retired, whatever undo does later,
        // as the run could read any of them. Only runs raise this floor, so the interface on
        // its own numbers as before. Ids of colors the run adds are retired by the caller that
        // knows which of them it handed out (`pikslar-api`'s `run_with`): a run that paints
        // with many new colors it is never given the ids of leaves no gap behind.
        self.doc.colors.retire_held();
        let mut scratch = self.scratch();
        let ran = body(&mut scratch);
        // Ids the run gave are used up whether or not it is kept, so a script that held on to
        // one can never find it naming another layer.
        self.doc.take_on_floor(&scratch.doc);
        let value = ran?;
        if scratch.is_busy() {
            return Err(LimitError::Busy.into());
        }
        scratch.apply(StepCmd::EndDrag)?;
        self.take_on(scratch)?;
        Ok(value)
    }

    /// A stroke of any kind, pixels or map cells, or floating pixels. A value being dragged
    /// (a joined step) is not: its step is closed first, as before any other command.
    pub(crate) fn is_busy(&self) -> bool {
        self.is_stroking() || self.floating.is_some()
    }

    /// The run's editor. Nothing undoes there, so its history keeps one step at most: what the
    /// commands log is dropped as they go, and a long run holds only the document.
    fn scratch(&self) -> Editor {
        let mut scratch = Editor::with_undo_budget(self.doc.clone(), 0);
        scratch.memory_check = self.memory_check;
        scratch.widest_table = self.widest_table;
        scratch.layer = self.layer;
        scratch.frame = self.frame;
        scratch.color = self.color;
        scratch
    }

    /// Takes on the scratch editor's document as one step.
    fn take_on(&mut self, scratch: Editor) -> Result<(), LimitError> {
        let active: Option<LayerId> = self.layer_id(self.layer);
        let widest = scratch.widest_table;
        let new = scratch.doc;
        let views: Vec<View> = new.layers.iter().map(view_of).collect();
        let resized = (new.width, new.height) != (self.doc.width, self.doc.height);
        let change = |doc: &mut Document| {
            take_on_document(doc, new);
            Ok(())
        };
        // Frames the run renewed are kept even when they play as before (copy a frame, remove
        // the original): a handle the run gave out must go on naming its frame.
        self.keyed_frames = true;
        let took = match resized {
            true => self.transact_canvas(change),
            false => self.transact(change),
        };
        self.keyed_frames = false;
        took?;
        // Layers the step added keep a table no log entry names.
        self.widest_table = self.widest_table.max(widest);
        self.take_on_views(&views);
        if let Some(index) = active.and_then(|id| self.index_of(id).ok()) {
            self.layer = index;
        }
        Ok(())
    }

    /// Shows, locks and folds layers as the scratch editor left them, outside any step.
    fn take_on_views(&mut self, views: &[View]) {
        let mut changed = false;
        for layer in self.doc.layers.iter_mut() {
            let wanted = views.iter().find(|view| view.0 == layer.id());
            if let Some(&(_, visible, locked, folded)) = wanted {
                changed |= (layer.visible, layer.locked, layer.folded) != (visible, locked, folded);
                (layer.visible, layer.locked, layer.folded) = (visible, locked, folded);
            }
        }
        if changed {
            self.touched();
            self.damage = Damage::All;
        }
    }
}

/// Makes `doc`, which is logging a step, what `new` is, a copy of it changed by commands.
/// Layers the copy shares keep their session ids, so they are changed in place; the others are
/// removed or added whole.
pub(super) fn take_on_document(doc: &mut Document, new: Document) {
    let Document {
        width,
        height,
        colors,
        layers,
        frames,
        tags,
        frame_rate,
        slices,
        tilesets,
        foreign: _,
        blending,
        // Taken on before, whatever the run did.
        layer_floor: _,
        frame_floor: _,
        // Which file this is, not what the run changed.
        document_id: _,
    } = new;
    let wanted = layers.into_vec();
    for index in (0..doc.layers.len()).rev() {
        let id = doc.layers[index].id();
        if !wanted.iter().any(|l| l.id() == id) {
            doc.layers.remove(index);
        }
    }
    for (index, layer) in wanted.into_iter().enumerate() {
        match doc.layers.iter().position(|l| l.id() == layer.id()) {
            Some(at) => {
                if at != index {
                    doc.layers.move_to(at, index);
                }
                doc.layers[index].take_on(layer);
            }
            None => doc.layers.insert(index, layer),
        }
    }
    (doc.width, doc.height) = (width, height);
    doc.colors = colors;
    doc.frames = frames;
    doc.tags = tags;
    doc.frame_rate = frame_rate;
    doc.slices = slices;
    doc.tilesets = tilesets;
    doc.blending = blending;
}
