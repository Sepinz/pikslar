//! Slices in the undo model: added, set whole and removed, some of them not allowed, and the
//! canvas resized under them. Each allowed change is one step; each refused one changes nothing
//! at all, which the model's usual comparison with its remembered states checks.

use super::{bad, Model, SIDE};
use pikslar_core::command::CanvasCmd;
use pikslar_core::{Command, Document, Slice, SliceCmd, SliceRect, MAX_SLICES, SLICE_REACH};

/// Every slice of the document must be one that could have been made: whole as `checked` leaves
/// it, and not too many.
pub(super) fn check_slices(doc: &Document) -> Result<(), String> {
    if doc.slices.len() > MAX_SLICES {
        return bad(format!(
            "{} slices, past the most there may be",
            doc.slices.len()
        ));
    }
    for (index, slice) in doc.slices.iter().enumerate() {
        if slice.clone().checked().as_ref() != Ok(slice) {
            return bad(format!("slice {index} is not one that may be kept"));
        }
    }
    Ok(())
}

impl Model {
    /// One change to the slices, recorded as a step when it makes one.
    pub(super) fn slice_step(&mut self) -> Result<(), String> {
        let mut verdict = Ok(());
        self.edit("slices", |m| verdict = m.slice_change())?;
        verdict
    }

    fn slice_change(&mut self) -> Result<(), String> {
        let count = self.editor.doc().slices.len();
        // Mostly a slice there is; sometimes the place after the last, where none is.
        let index = match self.rng.chance(8) {
            true => count,
            false => self.rng.below(count.max(1)),
        };
        let command: Command = match self.rng.below(11) {
            0..=2 => SliceCmd::Add {
                slice: Box::new(self.valid_slice()),
            }
            .into(),
            3 => SliceCmd::Add {
                slice: Box::new(self.invalid_slice()),
            }
            .into(),
            4 | 5 => SliceCmd::Set {
                index,
                slice: Box::new(self.valid_slice()),
            }
            .into(),
            6 => SliceCmd::Set {
                index,
                slice: Box::new(self.invalid_slice()),
            }
            .into(),
            // Set as it is: nothing changes, so it is no step.
            7 if count > 0 => SliceCmd::Set {
                index: index.min(count - 1),
                slice: Box::new(self.editor.doc().slices[index.min(count - 1)].clone()),
            }
            .into(),
            8 | 9 => SliceCmd::Remove { index }.into(),
            _ => return self.resize_under_slices(),
        };
        let pushes = self.editor.history().pushes();
        let result = self.editor.apply(command);
        match (&result, self.editor.history().pushes() != pushes) {
            (Ok(_), true) => self.counts.slices += 1,
            (Err(_), false) => self.counts.slices_refused += 1,
            _ => {}
        }
        Ok(())
    }

    /// A canvas resize or crop moves every slice with the pixels, or, if one would be carried
    /// out of reach, is refused whole.
    fn resize_under_slices(&mut self) -> Result<(), String> {
        let before = self.editor.doc().slices.clone();
        let canvas = self.another_canvas();
        let CanvasCmd::Resize { dx, dy, .. } = canvas else {
            return bad("another_canvas made something other than a resize".to_owned());
        };
        if self.editor.apply(canvas).is_err() {
            self.counts.slices_refused += u64::from(!before.is_empty());
            return Ok(());
        }
        let after = &self.editor.doc().slices;
        let moved = |s: &Slice| SliceRect {
            x: s.bounds.x + dx,
            y: s.bounds.y + dy,
            ..s.bounds
        };
        let same_but_moved = before.len() == after.len()
            && before.iter().zip(after).all(|(b, a)| {
                a.bounds == moved(b)
                    && (&a.name, a.color, a.pivot, a.center)
                        == (&b.name, b.color, b.pivot, b.center)
            });
        match same_but_moved {
            true => Ok(()),
            false => bad("a resize did not move the slices with the pixels".to_owned()),
        }
    }

    /// A slice that is allowed: on the canvas, beside it, or at the far edge of what is reachable
    /// where a resize may carry it out.
    fn valid_slice(&mut self) -> Slice {
        let (w, h) = (
            1 + self.rng.below(SIDE) as u32,
            1 + self.rng.below(SIDE) as u32,
        );
        let x = match self.rng.chance(8) {
            true => SLICE_REACH - w as i32 - self.rng.between(0, 3),
            false => self.rng.between(-4, SIDE as i32 + 3),
        };
        let y = self.rng.between(-4, SIDE as i32 + 3);
        let pivot = self.rng.chance(2).then(|| {
            (
                self.rng.between(-2, w as i32 + 2),
                self.rng.between(-2, h as i32 + 2),
            )
        });
        let center = self.rng.chance(2).then(|| {
            let (cx, cy) = (self.rng.below(w as usize), self.rng.below(h as usize));
            SliceRect {
                x: cx as i32,
                y: cy as i32,
                width: 1 + self.rng.below(w as usize - cx) as u32,
                height: 1 + self.rng.below(h as usize - cy) as u32,
            }
        });
        // Some names run past the longest, which are cut.
        let name = match self.rng.chance(6) {
            true => "a slice with a long name, longer than allowed".to_owned(),
            false => format!("S{}", self.rng.below(100)),
        };
        Slice {
            name,
            color: self.rng.next_u64() as u32,
            bounds: SliceRect {
                x,
                y,
                width: w,
                height: h,
            },
            pivot,
            center,
        }
    }

    /// A slice that is not allowed, in one of the ways it can fail.
    fn invalid_slice(&mut self) -> Slice {
        let mut slice = self.valid_slice();
        let (w, h) = (slice.bounds.width, slice.bounds.height);
        match self.rng.below(6) {
            0 => slice.bounds.width = 0,
            1 => slice.bounds.height = 0,
            2 => {
                slice.center = Some(SliceRect {
                    x: w as i32,
                    y: 0,
                    width: 1,
                    height: 1,
                })
            }
            3 => {
                slice.center = Some(SliceRect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: h,
                })
            }
            4 => slice.pivot = Some((SLICE_REACH + 1, 0)),
            _ => slice.bounds.x = SLICE_REACH,
        }
        slice
    }
}
