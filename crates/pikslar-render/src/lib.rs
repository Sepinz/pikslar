//! CPU rendering: layers into a cached image, and that image onto the screen.

mod bands;
/// The arithmetic lives in the core, so that merging layers uses exactly what the screen uses.
pub use pikslar_core::blend;
pub mod compose;
pub mod drag_preview;
mod flatten;
#[cfg(test)]
mod frozen_tests;
#[cfg(test)]
mod groups_tests;
#[cfg(test)]
mod links_tests;
pub mod marks;
#[cfg(test)]
mod merge_tests;
pub mod onion;
mod pixel;
#[cfg(test)]
mod reference_tests;
#[cfg(test)]
mod tilemap_oracle;
#[cfg(test)]
mod tilemap_tests;
pub mod view;

pub use bands::{flatten_bands, BandSource, BAND_ROWS};
pub use compose::{shown_region, Compositor, BANDS_PER_THREAD, MAX_THREADS, PARALLEL_WORK};
pub use drag_preview::{DragPreview, CEL_EPOCH, PREVIEW_MIN_CELLS};
pub use flatten::{flatten, flatten_document};
pub use marks::{draw_ants, draw_frame, draw_path, screen_rect};
pub use onion::{OnionColors, OnionStyle};
pub use view::{
    blit_canvas, blit_true_size, current_backdrop, document_on_screen, set_backdrop, Backdrop,
    Rect, Surface, View,
};
