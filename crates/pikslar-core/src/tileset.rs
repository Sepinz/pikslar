//! Tilesets: the tiles a document keeps for its tilemap layers (M10).
//!
//! One table for the whole document, `Document::tilesets`. A tile's image is an ordinary
//! [`crate::image::Image`], so identical pixels are shared and an unedited copy costs a pointer.
//! Layers that place these tiles arrive in phase 10.2.

mod budget;
mod doc;
mod ids;
mod set;
mod table;

pub use budget::{
    stored_tile_bytes, MAX_TILE_BYTES, OLD_RECORD_BYTES, OLD_RULE_WORST_CASE, STORAGE_TILE_EXTRA,
    TILE_RECORD_BYTES,
};
pub use ids::{TileId, TilesetId, MAX_TILE_ID};
pub use set::{
    clean_name, TileEntry, Tileset, MAX_TILES, MAX_TILESETS, MAX_TILESET_NAME, MAX_TILE_SIDE,
};
pub use table::Tilesets;
