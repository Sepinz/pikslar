//! Moving cels. A move changes where a cel's image sits and never copies a pixel.

use super::{did, CelCmd, LayerId, Outcome};
use crate::cel_drag::CelDrag;
use crate::document::LimitError;
use crate::editor::Editor;
use crate::history::Edit;
use crate::layer::Turn;
use crate::steps::Dragged;

impl Editor {
    pub(super) fn apply_cel(&mut self, command: CelCmd) -> Result<Outcome, LimitError> {
        match command {
            CelCmd::Move {
                layer,
                frame,
                dx,
                dy,
            } => self.move_cel(layer, frame, (dx, dy), false).map(did),
            CelCmd::MoveOne {
                layer,
                frame,
                dx,
                dy,
            } => self.move_cel(layer, frame, (dx, dy), true).map(did),
            CelCmd::Turn { layer, frame, turn } => self.turn_cel(layer, frame, turn).map(did),
            CelCmd::Drag(drag) => self.drag_cels(drag).map(did),
            CelCmd::Tween {
                layer,
                from,
                to,
                ease,
            } => self.tween_cels(layer, from, to, ease).map(did),
        }
    }

    /// Makes a timeline drag as one step, and moves to where the block landed.
    fn drag_cels(&mut self, drag: CelDrag) -> Result<bool, LimitError> {
        self.doc.check_cel_drag(&drag)?;
        if drag.stays() {
            return Ok(false);
        }
        let revision = self.revision;
        self.transact(|doc| doc.drag_cels(&drag))?;
        // A drop that changed nothing to see leaves the current cel where it was.
        if self.revision == revision {
            return Ok(false);
        }
        self.layer = drag.to_layer;
        self.frame = drag.to_frame;
        self.damage_all();
        Ok(true)
    }

    /// Turns a cel's whole drawing for every cel of its link, on every layer, as one step.
    fn turn_cel(&mut self, layer: LayerId, frame: usize, turn: Turn) -> Result<bool, LimitError> {
        let index = self.index_of(layer)?;
        let members = self.doc.link_members(index, frame);
        if members.is_empty() {
            return Err(LimitError::NoSuchItem);
        }
        let place = self.doc.layers[index].cel(frame).position();
        let at = |&(l, f): &(usize, usize)| self.doc.layers[l].cel(f).position();
        if members.iter().any(|m| at(m) != place) {
            return Err(LimitError::UnequalHold);
        }
        let (w, h) = (self.doc.width, self.doc.height);
        let revision = self.revision;
        self.transact(|doc| {
            // Read inside, after any floating pixels went down.
            let Some(turned) = doc.layers[index].cel(frame).turned(turn, w, h)? else {
                return Ok(());
            };
            for &(l, f) in &members {
                doc.layers[l].put_cel(f, turned.clone())?;
            }
            Ok(())
        })?;
        Ok(self.revision != revision)
    }

    /// Moves a frame's cel and its hold together, as far as every one of them can go, or only
    /// the cel when `alone` is set.
    fn move_cel(
        &mut self,
        layer: LayerId,
        frame: usize,
        (dx, dy): (i32, i32),
        alone: bool,
    ) -> Result<bool, LimitError> {
        let index = self.index_of(layer)?;
        let cels = &self.doc.layers[index];
        if frame >= cels.frame_count() {
            return Err(LimitError::NoSuchItem);
        }
        // A reference is one picture at one place on every frame, so it always moves whole.
        let alone = alone && cels.kind != crate::layer::LayerKind::Reference;
        // A cel on its own is moved without a list, so a drag allocates nothing.
        let (one, held);
        let frames: &[u32] = if alone || cels.link(frame) == 0 {
            one = [frame as u32];
            &one
        } else {
            held = cels.members(frame);
            &held
        };
        let by = cels.possible_move(frames, dx, dy);
        if by == (0, 0) {
            return Ok(false);
        }
        // Every frame of a hold moves the same cels, so all of them continue one drag. Moving
        // one cel is another drag than moving its hold.
        let what = Dragged::Cel(layer, frames[0] as usize, alone);
        let revision = self.revision;
        if self.joins(what) {
            self.doc.layers[index].move_cels(frames, by.0, by.1);
            self.history.join_move(by);
            self.amended();
        } else if self.floating.is_some() {
            // Floating pixels go down first, and the move is one step with them.
            self.transact_joined(|doc| {
                doc.layers[index].move_cels(frames, by.0, by.1);
                Ok(())
            })?;
            self.mark_joined(what, revision);
        } else {
            self.record_cel_move(index, frames, by);
            self.mark_joined(what, revision);
        }
        self.damage_all();
        Ok(true)
    }

    /// Moves the cels as a step of their own, which ends any value in progress.
    fn record_cel_move(&mut self, index: usize, frames: &[u32], by: (i32, i32)) {
        self.history.finish_last(&self.doc);
        self.cancel();
        self.doc.layers[index].move_cels(frames, by.0, by.1);
        self.record_joined(Edit::Move {
            layer: index,
            frames: frames.into(),
            dx: by.0,
            dy: by.1,
        });
    }
}
