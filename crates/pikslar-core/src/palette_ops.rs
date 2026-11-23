//! Palette operations: ramps, lossless remaps and replacing a colour (M9). Phase 9.2 made the
//! ramps; 9.3 the remaps.

pub mod load;
pub mod ramp;
mod ramp_table;
pub mod remap;
pub(crate) mod rewrite;

#[cfg(test)]
mod remap_build;
#[cfg(test)]
mod remap_property_tests;
