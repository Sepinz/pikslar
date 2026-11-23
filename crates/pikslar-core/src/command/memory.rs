//! Asks for what a command will need before the command changes anything.
//!
//! This is a check and not a promise: memory can still run out between the check and its use, and
//! a failed small allocation ends the process, which is what the recovery copy is for.

use super::{Command, DrawCmd, FloatingCmd, FrameCmd, LayerCmd, PaletteCmd, SelectionCmd, StepCmd};
use crate::document::LimitError;
use crate::editor::{Editor, Tool};
use crate::tile::TILE_AREA;

/// Answers whether this many bytes can be had right now.
pub type MemoryCheck = fn(usize) -> bool;

/// Below this the check would cost more than it could ever save.
const WORTH_CHECKING: usize = 1 << 20;
/// A stroke rarely touches a whole large canvas, so its copies are estimated, up to this limit.
const MOST_FOR_A_STROKE: usize = 64 << 20;
const STROKE_BYTES_PER_TILE: usize = 1 << 10;
const BYTES_PER_CEL_ENTRY: usize = 24;
const BYTES_PER_TILE_ENTRY: usize = 8;
const BYTES_PER_LAYER: usize = 128;
const BYTES_PER_FRAME: usize = 16;
const BYTES_PER_TAG: usize = 64;
const FILL_MASK_SHARE: usize = 8;

/// A real allocation that is given straight back.
pub(crate) fn system_has_room(bytes: usize) -> bool {
    if bytes < WORTH_CHECKING {
        return true;
    }
    let mut space = Vec::<u8>::new();
    let found = space.try_reserve_exact(bytes).is_ok();
    // Without this an optimizer may remove the request, and then every check would pass.
    std::hint::black_box(space.as_ptr());
    found
}

impl Editor {
    /// Replaces the check, so tests can refuse memory at the moment they choose.
    #[doc(hidden)]
    pub fn set_memory_check(&mut self, check: MemoryCheck) {
        self.memory_check = check;
    }

    // Inlined into each kind's `apply`, where only that kind's arm is left.
    #[inline(always)]
    pub(super) fn check_memory(&self, command: &Command) -> Result<(), LimitError> {
        let bytes = self.bytes_for(command);
        if bytes == 0 || (self.memory_check)(bytes) {
            Ok(())
        } else {
            Err(LimitError::NoMemory)
        }
    }

    // Inlined into each kind's `apply`, where only that kind's arm is left.
    #[inline(always)]
    fn bytes_for(&self, command: &Command) -> usize {
        match command {
            Command::Step(step) => self.step_bytes(step),
            Command::Layer(LayerCmd::SetMode { .. } | LayerCmd::MergeDown { .. }) => self
                .structure_bytes()
                .saturating_add(self.canvas_bytes().saturating_mul(self.doc.frames.len())),
            Command::Layer(LayerCmd::PlaceReference { image, .. }) => {
                self.structure_bytes().saturating_add(image.heap_size())
            }
            Command::Layer(LayerCmd::Add { .. }) | Command::Frame(FrameCmd::Add { .. }) => self
                .structure_bytes()
                .saturating_add(self.tile_table_bytes()),
            Command::Frame(FrameCmd::Release { from, to, .. }) => {
                let last = (*to).min(self.doc.frames.len().saturating_sub(1));
                let frames = last.saturating_sub(*from).saturating_add(1);
                self.structure_bytes()
                    .saturating_add(self.tile_table_bytes().saturating_mul(frames))
            }
            Command::Palette(PaletteCmd::Remap(_) | PaletteCmd::ReplaceColor { .. }) => {
                self.remap_bytes()
            }
            // Moving every cel copies no pixel; the step keeps the cel lists it replaced.
            Command::Canvas(_)
            | Command::Layer(_)
            | Command::Frame(_)
            | Command::Tag(_)
            | Command::Slice(_)
            | Command::Palette(_)
            | Command::Cel(super::CelCmd::Drag(_) | super::CelCmd::Tween { .. }) => {
                self.structure_bytes()
            }
            Command::Draw(command) => self.draw_bytes(command),
            Command::Selection(command) => self.selection_command_bytes(command),
            Command::Floating(command) => self.floating_command_bytes(command),
            Command::Tile(command) => self.tile_command_bytes(command),
            Command::Terrain(command) => self.terrain_command_bytes(command),
            // A move changes two numbers per cel. With floating pixels it first puts them down,
            // which writes no more than a stroke over them would.
            Command::Cel(super::CelCmd::Move { .. } | super::CelCmd::MoveOne { .. }) => {
                self.floating_bytes()
            }
            // A turn builds the drawing again beside the old one.
            Command::Cel(super::CelCmd::Turn { layer, frame, .. }) => self
                .index_of(*layer)
                .ok()
                .and_then(|i| self.doc.layers[i].cels().get(*frame))
                .map_or(0, |cel| cel.heap_size())
                .saturating_add(self.floating_bytes()),
        }
    }

    /// A remap builds each palette image again beside the old one, which the step keeps, so it
    /// needs room for every distinct drawing of cels and palette tilesets once more.
    fn remap_bytes(&self) -> usize {
        let images = crate::palette_ops::rewrite::palette_images(&self.doc);
        let pixels: usize = images.iter().map(|image| image.heap_size()).sum();
        self.structure_bytes().saturating_add(pixels)
    }

    /// Undoing an erase can grow an image's table back to the size of the canvas. Below the size
    /// worth checking, undo must never be the thing that asks for memory.
    fn step_bytes(&self, step: &StepCmd) -> usize {
        // A table can be wider than the canvas once a cel has moved, so the widest one counts.
        let table = self.tile_table_bytes().max(self.widest_table);
        // Also spares an ordinary canvas from walking its history on every rewind.
        if table < WORTH_CHECKING {
            return 0;
        }
        let images = match step {
            StepCmd::Undo | StepCmd::Redo => 1,
            StepCmd::ScrubTo { depth } => self.history.images_edited_on_the_way_to(*depth),
            StepCmd::EndDrag | StepCmd::AbortJoin => 0,
        };
        table.saturating_mul(images)
    }

    fn canvas_bytes(&self) -> usize {
        self.doc
            .width
            .saturating_mul(self.doc.height)
            .saturating_mul(4)
    }

    /// A cel is a table of shared tiles, so a new one costs its table and no pixels.
    fn tile_table_bytes(&self) -> usize {
        let tiles = self.doc.width.saturating_mul(self.doc.height) / TILE_AREA + 1;
        tiles.saturating_mul(BYTES_PER_TILE_ENTRY)
    }

    /// What a snapshot of the document's structure costs, plus settling any floating pixels.
    pub(super) fn structure_bytes(&self) -> usize {
        let (layers, frames) = (self.doc.layers.len(), self.doc.frames.len());
        layers
            .saturating_mul(frames)
            .saturating_mul(BYTES_PER_CEL_ENTRY)
            .saturating_add(layers.saturating_mul(BYTES_PER_LAYER))
            .saturating_add(frames.saturating_mul(BYTES_PER_FRAME))
            .saturating_add(self.doc.tags.len().saturating_mul(BYTES_PER_TAG))
            .saturating_add(self.floating_bytes())
    }

    fn stroke_bytes(&self) -> usize {
        let tiles = self.doc.width.saturating_mul(self.doc.height) / TILE_AREA + 1;
        tiles
            .saturating_mul(STROKE_BYTES_PER_TILE)
            .min(MOST_FOR_A_STROKE)
            .saturating_add(self.floating_bytes())
            .saturating_add(self.growth_bytes())
    }

    /// Growing the active cel's table to take what a write on the canvas puts there.
    pub(crate) fn growth_bytes(&self) -> usize {
        let cel = self
            .doc
            .layers
            .get(self.layer)
            .and_then(|l| l.cels().get(self.frame));
        cel.map_or(0, |cel| cel.growth_bytes(self.doc.width, self.doc.height))
    }

    /// A fill can reach every tile, each with a mask beside its pixels.
    fn fill_bytes(&self) -> usize {
        let area = match self.selected_bytes() {
            0 => self.canvas_bytes(),
            selected => selected,
        };
        area.saturating_add(area / FILL_MASK_SHARE)
            .saturating_add(self.floating_bytes())
            .saturating_add(self.growth_bytes())
    }

    pub(super) fn selected_bytes(&self) -> usize {
        let (w, h) = self.selection.as_ref().map_or((0, 0), |shape| {
            let (_, _, w, h) = shape.bounds();
            (w.max(0) as usize, h.max(0) as usize)
        });
        w.saturating_mul(h).saturating_mul(4)
    }

    fn draw_bytes(&self, command: &DrawCmd) -> usize {
        match command {
            DrawCmd::Begin {
                tool: Tool::Fill, ..
            }
            | DrawCmd::Stroke {
                tool: Tool::Fill, ..
            } => self.fill_bytes(),
            DrawCmd::Begin { tool, .. } | DrawCmd::Stroke { tool, .. } => self
                .stroke_bytes()
                .saturating_add(self.brush_stroke_bytes(*tool)),
            DrawCmd::EraseCell { .. } => self.stroke_bytes(),
            DrawCmd::DeleteSelected => self.selected_bytes(),
            DrawCmd::To { .. } | DrawCmd::Commit | DrawCmd::Cancel => 0,
        }
    }

    fn selection_command_bytes(&self, command: &SelectionCmd) -> usize {
        match command {
            SelectionCmd::All | SelectionCmd::Invert => {
                (self.canvas_bytes() / 4).saturating_add(self.floating_bytes())
            }
            SelectionCmd::Select { .. } | SelectionCmd::Temporary { .. } | SelectionCmd::None => {
                self.floating_bytes()
            }
        }
    }

    fn floating_command_bytes(&self, command: &FloatingCmd) -> usize {
        match command {
            FloatingCmd::Scale { w, h, .. } => {
                let (w, h) = self.clamped_scale(*w, *h);
                (w as usize).saturating_mul(h as usize).saturating_mul(5)
            }
            FloatingCmd::Paste { clip, .. } | FloatingCmd::ReplacePasted { clip } => clip
                .width
                .saturating_mul(clip.height)
                .saturating_mul(8)
                .saturating_add(self.floating_bytes()),
            FloatingCmd::Lift { .. }
            | FloatingCmd::FlipHorizontal
            | FloatingCmd::FlipVertical
            | FloatingCmd::Rotate { .. } => self.selected_bytes(),
            FloatingCmd::Anchor => self.floating_bytes(),
            FloatingCmd::Move { .. }
            | FloatingCmd::Revert
            | FloatingCmd::GestureBegin
            | FloatingCmd::GestureEnd
            | FloatingCmd::GestureCancel => 0,
        }
    }
}
