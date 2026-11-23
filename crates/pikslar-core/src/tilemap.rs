//! Tilemap layers: the cells that place tileset tiles on the canvas (M10).
//!
//! Naming: `tile.rs` and `TileKey` are 16 x 16 storage tiles. Code here uses `TileId`,
//! `TilesetId`, `MapCell` and `TileStamp` for tileset tiles. Phase 10.2 adds the layer kind,
//! the cell and the rules that keep a document's maps sound.

mod cache;
mod capture;
mod cell;
mod check;
mod cut;
mod damage;
mod doc;
#[cfg(test)]
mod oracle;
mod places;
mod read;
mod sample;
mod stamp;
mod state;
pub(crate) mod stroke;
mod write_through;

pub use cache::{TileCache, MAX_CACHED_PIXELS};
pub use cell::{MapCell, PINNED};
pub use check::TilemapFault;
pub use damage::TileEdit;
pub(crate) use sample::{map_bounds, sample_window};
pub(crate) use stamp::Prepared;
pub use stamp::{orientation_of, TileStamp, MAX_STAMP_SIDE};
pub use state::{PendingCell, TileState};
pub(crate) use write_through::write_through;

#[cfg(test)]
mod audit_cases;
#[cfg(test)]
mod audit_tests;
#[cfg(test)]
mod cut_tests;
#[cfg(test)]
mod fixture;
#[cfg(test)]
mod layer_tests;
#[cfg(test)]
mod pending_tests;
#[cfg(test)]
mod place_timing;
#[cfg(test)]
mod rule_tests;
#[cfg(test)]
mod stroke_tests;
#[cfg(test)]
mod terrain_fixture;
#[cfg(test)]
mod terrain_property_tests;
#[cfg(test)]
mod terrain_stroke_tests;
#[cfg(test)]
mod write_through_timing;
