//! The Pikslar document engine: tiles, layers, frames, strokes and history.
//!
//! Every pixel cell is a `u32`. On a palette layer it is an index into the color table,
//! on an RGBA layer it is a straight-alpha color, and 0 means transparent in both.
//! That single representation means tiles, undo, overlays and files have one code path,
//! so neither color workflow can fall behind the other.

pub mod animation;
pub mod autotile;
pub mod blend;
mod blend_hsl;
pub mod blend_mode;
pub mod blend_straight;
pub mod brush;
pub mod cel_drag;
#[cfg(test)]
mod cel_drag_tests;
pub mod color;
pub mod command;
pub mod custom_brush;
pub mod document;
pub mod editor;
pub mod floating;
pub mod frame_block;
#[cfg(test)]
mod frame_block_tests;
mod frame_damage;
mod frame_ids;
#[cfg(test)]
mod frame_ids_tests;
pub mod glyphs;
mod glyphs_latin;
mod glyphs_tiny;
pub mod grid;
pub mod groups;
#[cfg(test)]
mod groups_tests;
mod held;
pub mod history;
pub mod ids;
#[cfg(test)]
mod ids_tests;
pub mod image;
pub mod layer;
pub(crate) mod layers;
mod limit_text;
pub mod links;
pub mod lint;
pub mod mask;
mod merge;
#[cfg(test)]
mod merge_tests;
pub mod names;
#[cfg(test)]
mod offset_tests;
pub mod orient;
#[cfg(test)]
mod orient_tests;
pub mod overlay;
pub mod palette_ops;
#[cfg(test)]
mod redo_aside_tests;
pub mod reference;
#[cfg(test)]
mod reference_tests;
pub mod rhythm;
#[cfg(test)]
mod seams_tests;
pub mod selection;
#[cfg(test)]
mod shown_tests;
pub mod slice;
#[cfg(test)]
mod slice_tests;
pub mod sorting;
mod span_fill;
pub(crate) mod steps;
pub mod tile;
pub mod tilemap;
pub mod tileset;
pub mod timing;
pub mod tween;
#[cfg(test)]
mod undo_tests;
#[cfg(test)]
mod widest_tests;

pub use animation::{
    onion_ghosts, tidy_links, Direction, Ghost, Playhead, Span, Tag, TagId, MAX_DURATION_MS,
    MAX_TAGS, MAX_TAG_NAME, MAX_TAG_REPEAT, MIN_DURATION_MS,
};
pub use brush::{brush_origin, brush_outline, in_brush};
pub use cel_drag::{CelDrag, DragKind, Landing};
pub use color::{
    clean_group_name, hue_key, hue_order, light_order, lightness_key, rgba, ColorTable, GroupKind,
    PaletteGroup, SortBy, TableFull, MAX_GROUPS, MAX_GROUP_NAME,
};
pub use command::{
    CelCmd, Command, Corners, DrawCmd, FloatingCmd, FrameCmd, LayerCmd, LayerId, Outcome,
    PaletteCmd, SelectionCmd, SliceCmd, StepCmd, TagCmd, TerrainCmd, TerrainShape, TerrainTool,
    TileCmd, TileSource, TileTool,
};
pub use document::{
    BlendMode, Blending, ColorMode, Document, ForeignSection, FrameInfo, LimitError, MAX_DIMENSION,
    MAX_FRAMES,
};
pub use editor::{Damage, Editor, OlderStep, RewindRefusal, StepView, TerrainPreview, Tool};
pub use floating::{stretched, Clip, FloatingSnapshot, FloatingView};
pub use grid::Grid;
pub use history::{Edit, Snapshot};
pub use history::{History, RewindMarks, SavedCopy};
pub use ids::{DocumentId, FrameKey, MAX_FRAME_ID};
pub use image::{Image, TileKey, MAX_TILE_REACH};
pub use layer::{Cel, Layer, LayerKind, PersistedLayerId, Turn, OFFSET_LIMIT};
pub use lint::{lint, lint_all, lint_cel, lint_layer, Finding, LintKind, LINT_MARGIN};
pub use mask::{scale_edges, Edge, Mask};
pub use overlay::Overlay;
pub use palette_ops::load::LoadHow;
pub use palette_ops::remap::RemapKind;
pub use selection::SelectOp;
pub use slice::{Slice, SliceRect, MAX_SLICES, MAX_SLICE_NAME, SLICE_REACH};
pub use tile::{Tile, TILE, TILE_AREA, TILE_SHIFT};
pub use timing::{cumulative, tick_ms, ticks, MAX_FRAME_RATE};
pub use tween::{tween_offsets, Ease};
