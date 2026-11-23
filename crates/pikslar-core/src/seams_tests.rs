//! The empty seams later phases fill in must change nothing today.

use crate::custom_brush::BrushState;
use crate::tilemap::TileState;
use crate::{ColorMode, Command, Document, Editor, LimitError, TerrainCmd, TileCmd};

#[test]
fn new_limit_errors_say_what_the_plan_says() {
    let texts = [
        (
            LimitError::BrushTooLarge,
            "Brushes are at most 256 by 256 pixels",
        ),
        (
            LimitError::NothingToCapture,
            "Select some pixels to make a brush",
        ),
        (LimitError::TilesetInUse, "This tileset is used by a layer"),
        (
            LimitError::LinkTileset,
            "Links join tilemap layers of the same tileset",
        ),
        (
            LimitError::Tilemap,
            "Use the Tiles or Pixels mode on a tilemap layer",
        ),
        (LimitError::SquareTiles, "Turning tiles needs square tiles"),
        (LimitError::NoTileHere, "Place tiles here first"),
        (LimitError::TileSize, "A tile is 1 to 256 pixels each way"),
    ];
    for (error, text) in texts {
        assert_eq!(error.to_string(), text);
    }
}

#[test]
fn every_new_limit_error_has_its_own_message() {
    let all = [
        LimitError::BrushTooLarge,
        LimitError::NothingToCapture,
        LimitError::TooManyGroups,
        LimitError::PaletteFile,
        LimitError::PaletteSize,
        LimitError::PaletteNoColors,
        LimitError::TooManyTilesets,
        LimitError::TooManyTiles,
        LimitError::TilesetInUse,
        LimitError::LinkTileset,
        LimitError::Tilemap,
        LimitError::SquareTiles,
        LimitError::NoTileHere,
        LimitError::TileSize,
        LimitError::TerrainSlot,
        LimitError::TooManyTerrains,
        LimitError::TweenUnheld,
        LimitError::TileMemory,
    ];
    let texts: std::collections::HashSet<String> = all.iter().map(ToString::to_string).collect();
    assert_eq!(texts.len(), all.len());
    assert!(texts.iter().all(|t| !t.is_empty()));
}

#[test]
fn a_new_editor_starts_with_empty_brush_and_tile_state() {
    let e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("a small canvas"));
    assert_eq!(e.brushes(), &BrushState::default());
    assert_eq!(e.tiles(), &TileState::default());
}

#[test]
fn the_empty_command_enums_convert_into_a_command_and_keep_it_small() {
    // They add variants but no data; the limit itself is asserted beside `Command`.
    let _: fn(TileCmd) -> Command = Command::from;
    let _: fn(TerrainCmd) -> Command = Command::from;
    assert!(std::mem::size_of::<Command>() <= 56);
}
