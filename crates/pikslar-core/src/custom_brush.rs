//! Custom brushes: pixels captured from the canvas and stamped along a stroke (M8).
//!
//! Brushes are view state, like the tool and the brush size: choosing, capturing and turning
//! one is never an undo step. A [`Brush`] keeps its source pixels untouched and an
//! [`Orientation`](crate::orient::Orientation) over them, so four quarter turns give back the
//! identical brush. A [`Stamp`] is a brush made ready for one stroke: cells resolved for the
//! layer's colour mode, turned, and laid out as row runs, so stamping never allocates.

mod brush;
mod capture;
mod preset;
mod resolve;
mod stamp;
mod state;
#[cfg(test)]
mod tests;

pub use brush::{Brush, BrushPaint, Handle, MAX_BRUSH_SIDE};
pub use preset::Preset;
pub use stamp::{Prepared, Stamp, StampRun};
pub use state::{BrushChoice, BrushState, MAX_RECENT_BRUSHES};
