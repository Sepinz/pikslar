//! Values dragged a little at a time, and cels moved past the canvas or turned.

use super::Model;
use crate::go;
use pikslar_core::{CelCmd, Command, FrameCmd, LayerCmd, PaletteCmd, StepCmd, TagCmd, Turn};

/// How to set a dragged value back as it was when the drag began.
enum Back {
    Set(Command),
    /// Move the dragged cel back to this place.
    CelAt(i32, i32),
}

/// A way to pick a new value for what is dragged.
type Change = fn(&mut Model) -> Command;

/// How a drag ends.
enum End {
    /// Set back to where it began, so that the drag comes to nothing.
    Back,
    Cancelled,
    Released,
}

impl Model {
    /// Drags one value through a few changes. A drag that goes back to where it began, or is
    /// cancelled, must leave history as it was, redo included.
    pub(super) fn drag(&mut self) {
        let Some((change, back)) = self.dragged_value() else {
            return;
        };
        for _ in 0..1 + self.rng.below(4) {
            let command = change(self);
            go(&mut self.editor, command);
        }
        let end = match self.rng.below(3) {
            0 => End::Back,
            1 => End::Cancelled,
            _ => End::Released,
        };
        match end {
            End::Back => {
                let command = match back {
                    Back::Set(command) => command,
                    Back::CelAt(x, y) => self.cel_move_to(x, y),
                };
                go(&mut self.editor, command);
            }
            End::Cancelled => go(&mut self.editor, StepCmd::AbortJoin),
            End::Released => {}
        }
    }

    /// A way to change a value at random, and the command that sets it back as it is now.
    fn dragged_value(&mut self) -> Option<(Change, Back)> {
        let doc = self.editor.doc();
        let (index, frame) = (self.editor.layer, self.editor.frame);
        let layer = self.editor.layer_id(index)?;
        let set = |command: Command| Back::Set(command);
        Some(match self.rng.below(5) {
            0 => (Self::new_opacity, {
                let opacity = doc.layers()[index].opacity;
                set(LayerCmd::SetOpacity { layer, opacity }.into())
            }),
            1 => (Self::new_timing, {
                let durations = vec![doc.frames[frame].duration_ms];
                set(FrameCmd::Retime {
                    from: frame,
                    durations,
                }
                .into())
            }),
            2 => {
                let id = *doc.colors.swatches().first()?;
                let color = doc.colors.color(id);
                (
                    Self::new_color,
                    set(PaletteCmd::Recolor { id, color }.into()),
                )
            }
            3 => {
                let name = doc.tags.first()?.name.clone();
                (Self::new_name, set(TagCmd::SetName { tag: 0, name }.into()))
            }
            _ => {
                let (x, y) = doc.layers()[index].cel(frame).position();
                (Self::new_place, Back::CelAt(x.into(), y.into()))
            }
        })
    }

    fn new_opacity(&mut self) -> Command {
        let layer = self.editor.layer_id(self.editor.layer).expect("layer");
        let opacity = self.rng.next_u64() as u8;
        LayerCmd::SetOpacity { layer, opacity }.into()
    }

    fn new_timing(&mut self) -> Command {
        let durations = vec![10 + self.rng.below(900) as u32];
        let from = self.editor.frame;
        FrameCmd::Retime { from, durations }.into()
    }

    fn new_color(&mut self) -> Command {
        let id = self
            .editor
            .doc()
            .colors
            .swatches()
            .first()
            .copied()
            .unwrap_or(1);
        let color = self.rng.next_u64() as u32 | 0xff;
        PaletteCmd::Recolor { id, color }.into()
    }

    fn new_name(&mut self) -> Command {
        let name = format!("n{}", self.rng.below(4));
        TagCmd::SetName { tag: 0, name }.into()
    }

    /// Moves the cel far enough that its pixels go past the canvas, and back.
    fn new_place(&mut self) -> Command {
        let reach = 3 * super::SIDE as i32;
        let to = (
            self.rng.between(-reach, reach),
            self.rng.between(-reach, reach),
        );
        self.cel_move_to(to.0, to.1)
    }

    /// The move from where the dragged cel sits now to a place.
    fn cel_move_to(&mut self, x: i32, y: i32) -> Command {
        let (index, frame) = (self.editor.layer, self.editor.frame);
        let layer = self.editor.layer_id(index).expect("layer");
        let (at_x, at_y) = self.editor.doc().layers()[index].cel(frame).position();
        CelCmd::Move {
            layer,
            frame,
            dx: x - i32::from(at_x),
            dy: y - i32::from(at_y),
        }
        .into()
    }

    /// Turns the drawing of the current cel, which may be shared with other frames.
    pub(super) fn turn(&mut self) {
        let layer = self.editor.layer_id(self.editor.layer).expect("layer");
        let turn = match self.rng.below(4) {
            0 => Turn::FlipHorizontal,
            1 => Turn::FlipVertical,
            2 => Turn::Clockwise,
            _ => Turn::CounterClockwise,
        };
        let frame = self.editor.frame;
        go(&mut self.editor, CelCmd::Turn { layer, frame, turn });
    }
}
