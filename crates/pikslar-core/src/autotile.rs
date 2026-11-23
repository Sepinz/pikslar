//! Automatic tilemaps: terrain sets and the rules that pick a tile from its neighbours (M11).

pub(crate) mod paint;
pub mod rules;
pub(crate) mod stroke;
pub mod terrain;
mod terrain_doc;

pub use terrain::{clean_terrain_name, TerrainKind, TerrainSet, MAX_TERRAINS, MAX_TERRAIN_NAME};
