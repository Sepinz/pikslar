//! A script run in progress: each operation checked, then carried out by commands.

use crate::{input, query, ApiError, FrameRef, LayerRef, Op, Reply, API_VERSION};
use pikslar_core::{Command, Document, Editor, Outcome, StepCmd};
use std::sync::atomic::{AtomicBool, Ordering};

pub use blocks::MOST_READ_PIXELS;

mod blocks;
mod edit;
mod pixels;

/// The operations of one run, on an editor of its own: nothing reaches the document the run
/// was started on until the run is over and has succeeded.
pub struct Script<'a> {
    editor: &'a mut Editor,
    cancel: Option<&'a AtomicBool>,
    /// The highest color id the run was given or read: what must stay retired after it.
    pub(crate) seen_color: u32,
}

impl<'a> Script<'a> {
    pub(crate) fn new(editor: &'a mut Editor, cancel: Option<&'a AtomicBool>) -> Self {
        Script {
            editor,
            cancel,
            seen_color: 0,
        }
    }

    /// Notes a color id the run was given or read.
    pub(crate) fn saw_color(&mut self, id: u32) {
        self.seen_color = self.seen_color.max(id);
    }

    /// Carries out one operation. An error ends the run: return it, and the run is taken back.
    pub fn call(&mut self, op: Op) -> Result<Reply, ApiError> {
        if self.cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return Err(ApiError::Cancelled);
        }
        match op {
            Op::Version => Ok(Reply::Version(API_VERSION)),
            Op::Document => Ok(query::document(self.doc())),
            Op::Layer { layer } => query::layer(self.doc(), layer),
            Op::Swatches => {
                let read = query::swatches(self.doc());
                if let Reply::Swatches(list) = &read {
                    list.iter().for_each(|&(id, _)| self.saw_color(id));
                }
                Ok(read)
            }
            Op::Pixel { layer, frame, x, y } => self.pixel(layer, frame, x, y),
            Op::SetPixel { .. } | Op::ErasePixel { .. } | Op::DrawLine { .. } | Op::Fill { .. } => {
                self.draw(op)
            }
            Op::PutPixels { .. }
            | Op::FillRect { .. }
            | Op::DrawText { .. }
            | Op::Pixels { .. } => self.block(op),
            Op::AddLayer { .. }
            | Op::RemoveLayer { .. }
            | Op::RenameLayer { .. }
            | Op::MoveLayer { .. }
            | Op::DuplicateLayer { .. }
            | Op::SetOpacity { .. }
            | Op::SetVisible { .. }
            | Op::SetBlend { .. } => self.edit_layer(op),
            Op::AddFrame { .. }
            | Op::RemoveFrame { .. }
            | Op::SetDuration { .. }
            | Op::AddTag { .. }
            | Op::AddSwatch { .. }
            | Op::Recolor { .. }
            | Op::RemoveSwatch { .. } => self.edit_timeline_or_palette(op),
        }
    }

    /// Refuses drawing on a cel the editor would not let a paste draw on: no such layer or
    /// frame, a locked layer (or one in a locked group), a layer that is not a paint layer,
    /// or a drawing shared with a locked layer. Changes nothing. An engine that keeps pixels
    /// waiting before writing them checks this when it starts keeping them.
    pub fn draw_refusal(&self, layer: LayerRef, frame: FrameRef) -> Result<(), ApiError> {
        let index = input::layer_index(self.doc(), layer)?;
        let at = input::frame(self.doc(), frame)?;
        let refusal = self.editor.paint_refusal(index);
        match refusal.or_else(|| self.editor.shared_refusal(index, at)) {
            Some(why) => Err(ApiError::Refused(why)),
            None => Ok(()),
        }
    }

    /// The canvas width and height, and how many layers and frames there are, as the run's
    /// document has them now: what an engine needs to price its next operation.
    pub fn size(&self) -> (usize, usize, usize, usize) {
        let doc = self.doc();
        (doc.width, doc.height, doc.layers().len(), doc.frames.len())
    }

    /// How many cels duplicating this layer copies: its frames, for each row of the layer
    /// and, for a group, of everything in it. None when there is no such layer.
    pub fn cels_copied(&self, layer: LayerRef) -> Option<usize> {
        let doc = self.doc();
        let (low, high) = doc.block(input::layer_index(doc, layer).ok()?);
        Some((high - low + 1).saturating_mul(doc.frames.len()))
    }

    /// How many swatches the run's document has: what a read of the swatches lists.
    pub fn swatch_count(&self) -> usize {
        self.doc().colors.swatches().len()
    }

    /// How many colors the run's document's table holds, swatches among them: what a
    /// palette edit looks through.
    pub fn colors(&self) -> usize {
        self.doc().colors.len()
    }

    /// What the run's document holds, shared images and tiles counted once
    /// ([`Document::held_bytes`]), and about what measuring that costs, in cels and tiles.
    pub fn held(&self) -> usize {
        self.doc().held_bytes()
    }

    /// About what [`Self::held`] costs to measure, in cels and tiles it visits.
    pub fn held_cost(&self) -> usize {
        self.doc().held_cost()
    }

    fn doc(&self) -> &Document {
        self.editor.doc()
    }

    fn apply(&mut self, command: impl Into<Command>) -> Result<Outcome, ApiError> {
        Ok(self.editor.apply(command)?)
    }

    /// Runs a command that would join the next change of the same value, as dragging does,
    /// and ends the value, so each operation is its own change.
    fn apply_ended(&mut self, command: impl Into<Command>) -> Result<(), ApiError> {
        self.apply(command)?;
        self.apply(StepCmd::EndDrag)?;
        Ok(())
    }

    /// Makes a layer's cel at a frame the one the editor's tools work on.
    fn choose_cel(&mut self, layer: LayerRef, frame: FrameRef) -> Result<(), ApiError> {
        let index = input::layer_index(self.doc(), layer)?;
        let frame = input::frame(self.doc(), frame)?;
        (self.editor.layer, self.editor.frame) = (index, frame);
        Ok(())
    }

    /// The persisted id of the layer the last command made active.
    fn active_layer_id(&self) -> Result<Reply, ApiError> {
        let layer = self.doc().layers().get(self.editor.layer);
        let id = layer.map(|l| l.persisted_id().get());
        id.map(Reply::Created)
            .ok_or(ApiError::Refused(pikslar_core::LimitError::NoSuchItem))
    }
}
