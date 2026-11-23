//! The words of every [`LimitError`], one row of one table, so a variant added without words
//! is a compile error and not an empty message. The same table makes the list of all of them
//! the tests walk.

use crate::document::LimitError;

macro_rules! limit_texts {
    ($($variant:ident => $text:expr,)*) => {
        impl LimitError {
            /// What the person is told for this limit.
            pub fn text(&self) -> &'static str {
                match self {
                    $(LimitError::$variant => $text,)*
                }
            }

            /// Every limit error, as the table of words holds them.
            #[cfg(test)]
            pub(crate) const ALL: &'static [LimitError] = &[$(LimitError::$variant,)*];
        }
    };
}

limit_texts! {
    Dimensions => "Canvas size must be between 1 and 8192",
    TooManyFrames => "Frame limit reached",
    TooManyLayers => "Layer limit reached",
    LastLayer => "A sprite needs at least one layer",
    LastFrame => "A sprite needs at least one frame",
    TableFull => "This image has more than 65,536 colors",
    NoSuchItem => "That layer or frame no longer exists",
    NoMemory => "Not enough memory",
    OutOfReach => "The drawing reaches too far past the canvas to turn",
    CelPlacement => "Cels land only on layers of the same color mode",
    LinkModes => "Links join layers of the same color mode",
    LinkKind => "Only paint layers can share a drawing",
    SharedLocked => "This drawing is shared with a locked layer",
    CanvasReach => "That would leave a drawing too far from the canvas",
    Locked => "This layer is locked",
    NothingBelow => "There is no layer below to merge into",
    HiddenMerge => "Show both layers to merge them",
    Reference => "A reference layer is not drawn on",
    Group => "A group has no pixels of its own: choose a layer in it",
    TooDeep => "Groups go at most four deep",
    MergeAcrossGroups => "Merge two layers in the same group",
    GroupOrder => "A group moves with the layers in it",
    FrameRate => "A frame rate is from 1 to 100 frames a second",
    TooManySlices => "Slice limit reached",
    TweenTooShort => "Pick at least three frames",
    TweenUnheld => "Hold one drawing across these frames first",
    BrushTooLarge => "Brushes are at most 256 by 256 pixels",
    NothingToCapture => "Select some pixels to make a brush",
    TooManyGroups => "Palette group limit reached",
    PaletteFile => "That palette file could not be read",
    PaletteNoColors => "There are no colours to load",
    TooManyTilesets => "Tileset limit reached",
    TooManyTiles => "Tile limit reached",
    TilesetInUse => "This tileset is used by a layer",
    LinkTileset => "Links join tilemap layers of the same tileset",
    Tilemap => "Use the Tiles or Pixels mode on a tilemap layer",
    SquareTiles => "Turning tiles needs square tiles",
    NoTileHere => "Place tiles here first",
    TileSize => "A tile is 1 to 256 pixels each way",
    TerrainSlot => "That terrain has no such slot",
    TooManyTerrains => "A tileset has at most 64 terrains",
    TilemapMode => "A tilemap layer has the colour mode of its tileset",
    TilemapMerge => "A tilemap layer is not merged down",
    TilemapCels => "Tilemap cels land only on layers of the same tileset",
    NotTilemap => "This is not a tilemap layer",
    NoTileChosen => "Choose a tile of this layer's tileset first",
    StampTooLarge => "A stamp is at most 64 by 64 tiles",
    UnequalHold => "These held frames sit in different places; release the hold first",
    ReferenceTooLarge => "References are at most 4,096 by 4,096 pixels, and 64 MiB together",
    RampSteps => "A ramp runs between two different swatches, with 1 to 254 colours between",
    PaletteSize => "Load as colours needs as many colours as the palette has swatches",
    TileMemory => "Tiles take at most 416 MiB together in a project file: remove or reuse some tiles",
    Busy => "Finish the stroke or place the floating pixels first",
}

impl std::fmt::Display for LimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.text())
    }
}

#[cfg(test)]
#[path = "limit_text_tests.rs"]
mod tests;
