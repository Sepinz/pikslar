//! Palette files in the app: Import (Append, Replace or Load as colors) and Export, each through
//! a file dialog. A file that is refused is refused whole, with the line named, and the palette
//! is left as it was. Export reads the document and changes nothing.

use crate::app::App;
use crate::files::{read_palette, write_palette};
use crate::session::{confirm_corrected, with_required_extension};
use pikslar_core::{LoadHow, PaletteCmd};
use pikslar_format::palette_file::PaletteFormat;
use pikslar_platform::{open_file_dialog, save_file_dialog, show_error, Window};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const PALETTE_FILTER: &str = "Palette files\0*.gpl;*.pal;*.hex\0GIMP palette\0*.gpl\0\
                              JASC palette\0*.pal\0Hex colors\0*.hex\0";

fn name_of(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// What the dialog's choice means: a name that is not a palette file's gets `.gpl`.
pub(crate) fn palette_target(chosen: &Path) -> PathBuf {
    let known = chosen
        .extension()
        .and_then(|e| e.to_str())
        .and_then(PaletteFormat::from_extension)
        .is_some();
    match known {
        true => chosen.to_path_buf(),
        false => with_required_extension(chosen.to_path_buf(), "gpl"),
    }
}

impl App {
    /// Asks for a palette file and puts its colors in the palette as `how` says.
    pub(crate) fn import_palette(&mut self, how: LoadHow, window: Window) {
        self.finish_gesture();
        let Some(path) = open_file_dialog(window, PALETTE_FILTER) else {
            return;
        };
        match self.import_palette_from(&path, how) {
            Ok(said) => self.notify(said),
            Err(message) => show_error(window, &message),
        }
    }

    /// Reads `path` and loads it: one step, none if it changes nothing. Returns what to tell the
    /// person, or why nothing changed (with the line, for a file that could not be read).
    pub(crate) fn import_palette_from(
        &mut self,
        path: &Path,
        how: LoadHow,
    ) -> Result<String, String> {
        self.finish_gesture();
        let name = name_of(path);
        let read = read_palette(path).map_err(|e| format!("{name}: {e}"))?;
        let colors: Arc<[u32]> = read.palette.packed().into();
        let count = colors.len();
        let outcome = self
            .editor
            .apply(PaletteCmd::Load { colors, how })
            .map_err(|e| format!("{name}: {e}"))?;
        self.ui_dirty = true;
        if !outcome.changed {
            return Ok(format!("The palette already matches {name}"));
        }
        Ok(imported_message(count, &name, &read.notes))
    }

    /// Asks where, then writes the swatches in order as a palette file.
    pub(crate) fn export_palette(&mut self, window: Window) {
        self.finish_gesture();
        let start = self.path.as_ref().map(|p| p.with_extension("gpl"));
        let Some(chosen) = save_file_dialog(window, PALETTE_FILTER, "gpl", start.as_deref()) else {
            return;
        };
        let target = palette_target(&chosen);
        let Some(path) = confirm_corrected(window, &chosen, target) else {
            return;
        };
        match self.export_palette_to(&path) {
            Ok(said) => self.notify(said),
            Err(message) => show_error(window, &message),
        }
    }

    /// Writes the swatches to `path`. The format is the extension's.
    pub(crate) fn export_palette_to(&mut self, path: &Path) -> Result<String, String> {
        let name = name_of(path);
        let written = write_palette(path, &self.editor.doc().colors)
            .map_err(|e| format!("{name} was not written: {e}"))?;
        let mut said = format!("Exported {} colors to {name}", written.count);
        if written.partly_clear > 0 {
            said.push_str(&format!(
                ". {} partly clear, written as their color alone",
                written.partly_clear
            ));
        }
        Ok(said)
    }
}

/// What an import says: how many colours came from where, then every note the reader made.
pub(crate) fn imported_message(count: usize, name: &str, notes: &[String]) -> String {
    let mut said = format!("Imported {count} colors from {name}");
    for note in notes {
        said = format!("{said}. {note}");
    }
    said
}
