//! The workspace model of the UI redesign (docs/ui-redesign.md, section 3): which panels sit in
//! which dock area and slot, the four presets and one custom workspace, the pure solver that
//! turns an arrangement and a window into rectangles, and the settings lines that keep it.
//! The docks (`dock.rs`, `dock_keys.rs`, `slot_menu.rs`) draw and change it.

// Parts of the model are kept for phases 5 to 7 (the custom workspace, the layer column
// width), which use them only in tests until then.
#![allow(dead_code)]

pub(crate) mod metrics;
pub(crate) mod model;
pub(crate) mod moves;
pub(crate) mod persist;
pub(crate) mod presets;
pub(crate) mod solve;
