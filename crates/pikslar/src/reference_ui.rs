//! Placing a reference picture: a PNG shown above the active layer to draw from, left out of
//! everything exported.

use crate::app::App;
use crate::files::FileError;
use pikslar_core::{Cel, Grid, LayerCmd, LimitError};
use pikslar_format::png;
use pikslar_format::{read_limited, FormatError};
use pikslar_platform::{open_file_dialog, show_error, Window};
use std::path::Path;
use std::sync::Arc;

/// Far above what the largest reference can need, even stored without compression.
const MAX_REFERENCE_FILE_BYTES: u64 = 256 << 20;

impl App {
    /// Asks for a PNG and places it as a reference above the active layer.
    pub(crate) fn place_reference(&mut self, window: Window) {
        self.finish_gesture();
        let Some(path) = open_file_dialog(window, crate::session::PNG_FILTER) else {
            return;
        };
        if let Err(e) = self.place_reference_from(&path) {
            show_error(window, &e.to_string());
        }
    }

    /// Reads `path` and places it. Its size is checked from its header before anything is
    /// decoded, so an image too large for a reference costs only the read.
    pub(crate) fn place_reference_from(&mut self, path: &Path) -> Result<(), FileError> {
        self.finish_gesture();
        let bytes = read_limited(path, MAX_REFERENCE_FILE_BYTES)?;
        let (width, height) = png::dimensions(&bytes).map_err(too_large_as_reference)?;
        self.editor.doc().check_reference(width, height)?;
        let picture = png::decode(&bytes)?;
        drop(bytes);
        let grid = Grid::from_cells(picture.width, picture.height, &picture.px);
        drop(picture);
        let image = Arc::new(Cel::new(grid).image().clone());
        let name = reference_name(path);
        let below = self.editor.layer;
        // The refusal itself is shown, not one about drawing on references.
        self.editor
            .apply(LayerCmd::PlaceReference { below, image, name })
            .map_err(FileError::Limit)?;
        self.ui_dirty = true;
        self.reset_lint();
        Ok(())
    }
}

/// A PNG past the largest image is past the largest reference too, and says so as one.
fn too_large_as_reference(e: FormatError) -> FileError {
    match e {
        FormatError::TooLarge => FileError::Limit(LimitError::ReferenceTooLarge),
        other => FileError::Format(other),
    }
}

/// The file's name without its extension, as the layer's name.
fn reference_name(path: &Path) -> String {
    path.file_stem().map_or_else(
        || "Reference".to_string(),
        |s| s.to_string_lossy().into_owned(),
    )
}
