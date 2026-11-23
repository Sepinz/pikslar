//! Checks that keep people's work safe. None of this ships in the program.

pub mod corpus;
pub mod craft;
pub mod deep_use;
pub mod fuzz;
pub mod fuzz_brush;
pub mod fuzz_limits;
pub mod fuzz_palette;
pub mod fuzz_terrain;
pub mod fuzz_tiles;
pub mod mutate;
pub mod picture;
pub mod replay;
pub mod replay_brush;
pub mod same;
pub mod tracking;
pub mod undo_model;

/// Applies a command where being refused is a legal result, as it is for random input.
pub(crate) fn go(editor: &mut pikslar_core::Editor, command: impl Into<pikslar_core::Command>) {
    let _ = editor.apply(command);
}
