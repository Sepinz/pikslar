//! Canvas size and crop. The canvas clips what is seen and never removes a pixel, so changing
//! it moves every cel and keeps every drawing as it is.

use super::{CanvasCmd, Outcome};
use crate::document::{Document, LimitError, MAX_DIMENSION};
use crate::editor::Editor;

impl Document {
    /// Makes the canvas `width` by `height`, with the old canvas's top left corner at
    /// (`dx`, `dy`) in the new one. Every cel of every layer moves by that much, so holds stay
    /// holds and shared drawings stay shared. Refused, changing nothing, if the size is out of
    /// range or a cel would pass the offset range.
    pub(crate) fn resize_canvas(
        &mut self,
        width: usize,
        height: usize,
        dx: i32,
        dy: i32,
    ) -> Result<(), LimitError> {
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(LimitError::Dimensions);
        }
        let frames: Vec<u32> = (0..self.frames.len() as u32).collect();
        if self
            .layers
            .iter()
            .any(|layer| layer.possible_move(&frames, dx, dy) != (dx, dy))
        {
            return Err(LimitError::CanvasReach);
        }
        self.slices_can_move(dx, dy)?;
        self.move_slices(dx, dy);
        if (dx, dy) != (0, 0) {
            for layer in self.layers.iter_mut() {
                layer.move_cels(&frames, dx, dy);
            }
        }
        self.width = width;
        self.height = height;
        Ok(())
    }
}

impl Editor {
    pub(super) fn apply_canvas(&mut self, command: CanvasCmd) -> Result<Outcome, LimitError> {
        match command {
            CanvasCmd::Resize {
                width,
                height,
                dx,
                dy,
            } => self.transact_canvas(|d| d.resize_canvas(width, height, dx, dy))?,
            CanvasCmd::SetBlending { blending } => {
                let changed = self.doc.blending != blending;
                self.transact(|d| {
                    d.blending = blending;
                    Ok(())
                })?;
                return Ok(super::did(changed));
            }
        }
        Ok(Outcome::CHANGED)
    }
}

/// Where the old canvas goes in a new one: 0, 1 or 2 across and down, for the start, middle or
/// end. The middle rounds towards the start, as rotating about a centre does.
pub fn anchor_shift(old: (usize, usize), new: (usize, usize), anchor: (u8, u8)) -> (i32, i32) {
    let along = |old: usize, new: usize, at: u8| {
        let room = new as i64 - old as i64;
        let shift = match at {
            0 => 0,
            1 => room.div_euclid(2),
            _ => room,
        };
        shift as i32
    };
    (along(old.0, new.0, anchor.0), along(old.1, new.1, anchor.1))
}

/// The command that crops the canvas to a rectangle of it.
pub fn crop_to(x: i32, y: i32, width: usize, height: usize) -> CanvasCmd {
    CanvasCmd::Resize {
        width,
        height,
        dx: -x,
        dy: -y,
    }
}
