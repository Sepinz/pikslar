//! Slices: named rectangles of the canvas, for sprite sheets that cut one picture into parts.
//! A slice may have a pivot and a centre for nine-slice scaling, both measured from its top left
//! corner. A slice may reach past the canvas, as cels may, and moves with the pixels when the
//! canvas is resized.

use crate::document::{Document, LimitError, MAX_DIMENSION};

pub const MAX_SLICES: usize = 256;
pub const MAX_SLICE_NAME: usize = 32;
/// How far from the canvas corner a slice's edges and pivot may be.
pub const SLICE_REACH: i32 = 2 * MAX_DIMENSION as i32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliceRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slice {
    pub name: String,
    /// The color its outline is drawn in, as RGBA.
    pub color: u32,
    /// Where it is on the canvas.
    pub bounds: SliceRect,
    /// A point from the slice's top left corner, such as where a sprite stands.
    pub pivot: Option<(i32, i32)>,
    /// The part that stretches when the slice is scaled, from its top left corner.
    pub center: Option<SliceRect>,
}

fn within(v: i64) -> bool {
    (-i64::from(SLICE_REACH)..=i64::from(SLICE_REACH)).contains(&v)
}

impl SliceRect {
    fn reachable(&self) -> bool {
        let (x, y) = (i64::from(self.x), i64::from(self.y));
        within(x)
            && within(y)
            && within(x + i64::from(self.width))
            && within(y + i64::from(self.height))
    }
}

impl Slice {
    /// The slice as it may be kept: the name cut to [`MAX_SLICE_NAME`] characters. Refused for
    /// an empty rectangle or a centre outside the slice, and past [`SLICE_REACH`].
    pub fn checked(mut self) -> Result<Slice, LimitError> {
        let b = self.bounds;
        if b.width == 0 || b.height == 0 {
            return Err(LimitError::Dimensions);
        }
        if let Some(c) = self.center {
            let inside = c.x >= 0
                && c.y >= 0
                && i64::from(c.x) + i64::from(c.width) <= i64::from(b.width)
                && i64::from(c.y) + i64::from(c.height) <= i64::from(b.height);
            if c.width == 0 || c.height == 0 || !inside {
                return Err(LimitError::Dimensions);
            }
        }
        let pivot_in = self
            .pivot
            .is_none_or(|(x, y)| within(i64::from(x)) && within(i64::from(y)));
        if !b.reachable() || !pivot_in {
            return Err(LimitError::CanvasReach);
        }
        if self.name.chars().count() > MAX_SLICE_NAME {
            self.name = self.name.chars().take(MAX_SLICE_NAME).collect();
        }
        Ok(self)
    }
}

impl Document {
    /// Adds a slice at the end of the list, returning its index.
    pub fn add_slice(&mut self, slice: Slice) -> Result<usize, LimitError> {
        if self.slices.len() >= MAX_SLICES {
            return Err(LimitError::TooManySlices);
        }
        self.slices.push(slice.checked()?);
        Ok(self.slices.len() - 1)
    }

    /// Checks that every slice can move by (`dx`, `dy`), changing nothing.
    pub(crate) fn slices_can_move(&self, dx: i32, dy: i32) -> Result<(), LimitError> {
        let moved = |s: &Slice| SliceRect {
            x: s.bounds.x.saturating_add(dx),
            y: s.bounds.y.saturating_add(dy),
            ..s.bounds
        };
        match self.slices.iter().all(|s| moved(s).reachable()) {
            true => Ok(()),
            false => Err(LimitError::CanvasReach),
        }
    }

    /// Moves every slice by (`dx`, `dy`), once [`Document::slices_can_move`] allows it.
    pub(crate) fn move_slices(&mut self, dx: i32, dy: i32) {
        for slice in &mut self.slices {
            slice.bounds.x = slice.bounds.x.saturating_add(dx);
            slice.bounds.y = slice.bounds.y.saturating_add(dy);
        }
    }
}
