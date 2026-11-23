//! Keeps a command's numbers within what the editor's arithmetic is built for.
//!
//! The pointer can only give sensible positions, but a command is plain data and one day a script
//! may send anything. A position far outside the canvas is brought back to the edge of what can
//! matter, so no stroke walks for billions of cells and no sum can overflow.

use super::{CelCmd, Command, DrawCmd, FloatingCmd, SelectionCmd, TileCmd};
use crate::document::{LimitError, MAX_DIMENSION};
use crate::editor::Editor;
use crate::mask::Mask;

/// Far enough that everything reachable by hand or by a brush is unaffected.
pub(super) const REACH: i32 = MAX_DIMENSION as i32 * 4;

fn near(value: i32) -> i32 {
    value.clamp(-REACH, REACH)
}

fn near_both(x: &mut i32, y: &mut i32) {
    *x = near(*x);
    *y = near(*y);
}

fn check_shape(shape: &Mask) -> Result<(), LimitError> {
    let (x, y, w, h) = shape.bounds();
    let inside = |at: i32, size: i32| {
        let end = at.saturating_add(size);
        near(at) == at && near(end) == end
    };
    if inside(x, w) && inside(y, h) {
        Ok(())
    } else {
        Err(LimitError::Dimensions)
    }
}

impl Editor {
    // Inlined into each kind's `apply`, where only that kind's arm is left.
    #[inline(always)]
    pub(super) fn keep_in_reach(&self, command: &mut Command) -> Result<(), LimitError> {
        match command {
            Command::Draw(command) => keep_draw_in_reach(command),
            Command::Tile(TileCmd::Begin { x, y, .. } | TileCmd::To { x, y }) => near_both(x, y),
            Command::Tile(TileCmd::Stroke { path, .. }) => {
                for (x, y) in path {
                    near_both(x, y);
                }
            }
            Command::Selection(
                SelectionCmd::Select { shape, .. } | SelectionCmd::Temporary { shape },
            ) => check_shape(shape)?,
            Command::Floating(command) => self.keep_floating_in_reach(command)?,
            Command::Cel(CelCmd::Move { dx, dy, .. } | CelCmd::MoveOne { dx, dy, .. }) => {
                // No move is longer than the whole range, so the sums below cannot overflow.
                let most = 2 * crate::layer::OFFSET_LIMIT;
                *dx = (*dx).clamp(-most, most);
                *dy = (*dy).clamp(-most, most);
            }
            _ => {}
        }
        Ok(())
    }

    fn keep_floating_in_reach(&self, command: &mut FloatingCmd) -> Result<(), LimitError> {
        match command {
            FloatingCmd::Move { dx, dy, .. } => {
                let (x, y) = self.selection.as_ref().map_or((0, 0), |s| {
                    let (x, y, _, _) = s.bounds();
                    (x, y)
                });
                *dx = near(x.saturating_add(*dx)) - x;
                *dy = near(y.saturating_add(*dy)) - y;
            }
            FloatingCmd::Scale { x, y, w, h } => {
                near_both(x, y);
                near_both(w, h);
            }
            FloatingCmd::Paste { clip, x, y } => {
                if !clip.is_well_formed() {
                    return Err(LimitError::Dimensions);
                }
                near_both(x, y);
            }
            FloatingCmd::ReplacePasted { clip } if !clip.is_well_formed() => {
                return Err(LimitError::Dimensions);
            }
            _ => {}
        }
        Ok(())
    }
}

fn keep_draw_in_reach(command: &mut DrawCmd) {
    match command {
        DrawCmd::Begin { x, y, .. } | DrawCmd::To { x, y } | DrawCmd::EraseCell { x, y } => {
            near_both(x, y);
        }
        DrawCmd::Stroke { path, .. } => {
            for (x, y) in path {
                near_both(x, y);
            }
        }
        DrawCmd::Commit | DrawCmd::Cancel | DrawCmd::DeleteSelected => {}
    }
}
