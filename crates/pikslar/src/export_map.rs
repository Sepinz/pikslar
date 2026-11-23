//! TMJ from the export panel: a Tiled map of the frame shown, with an atlas PNG for each tileset
//! beside it, through `pikslar-export`'s plan. The panel names the layers a map leaves out.

use crate::app::App;
use crate::export_run::{app_message, settle};
use pikslar_core::LayerKind;
use pikslar_export::targets::check_name;
use pikslar_export::{tmj_outputs, write_outputs_locked, Kind};
use std::path::PathBuf;

/// The TMJ button, after the kinds.
pub(crate) const MAP_LABEL: &str = "TMJ";
pub(crate) const MAP_HINT: &str = "A Tiled map of the frame shown, with an atlas a tileset  6";
pub(crate) const MAP_ENDING: &str = "tmj";
pub(crate) const MAP_FILTER: &str = "Tiled map\0*.tmj\0";

impl App {
    /// Whether the document has a tilemap layer, so a map can be exported.
    pub(crate) fn has_tilemaps(&self) -> bool {
        self.editor.doc().layers().iter().any(|l| l.is_tilemap())
    }

    /// The paint and reference layers, by name, which a map has no place for.
    pub(crate) fn map_left_out(&self) -> Vec<String> {
        self.editor
            .doc()
            .layers()
            .iter()
            .rev()
            .filter(|l| matches!(l.kind, LayerKind::Paint | LayerKind::Reference))
            .map(|l| l.name.clone())
            .collect()
    }

    /// Exports the map of the frame shown to `path`, its atlases beside it. `ask` is asked
    /// before replacing atlases the save dialog did not ask about. Returns what to tell the
    /// person, None if they chose not to replace, or why nothing was written.
    pub(crate) fn export_map_as(
        &mut self,
        path: PathBuf,
        ask: &mut dyn FnMut(&str) -> bool,
    ) -> Result<Option<String>, String> {
        check_name(&path).map_err(|e| e.to_string())?;
        let (project, frame) = (self.path.clone(), self.editor.frame);
        let doc = self.editor.shown_document();
        let planned = tmj_outputs(&doc, frame, path.clone()).map_err(app_message)?;
        let mut outputs = planned.outputs;
        // A map is one file with its atlases, as a still image is: no numbered members.
        if settle(&mut outputs, (&path, Kind::Png), project.as_deref(), ask)?.is_none() {
            return Ok(None);
        }
        write_outputs_locked(&mut outputs, &path, &mut |_, _| Ok(())).map_err(|e| e.to_string())?;
        let mut done = format!("Exported {}", path.display());
        for note in planned.notes {
            done = format!("{done}. {note}");
        }
        Ok(Some(done))
    }
}

#[cfg(test)]
#[path = "export_map_tests.rs"]
mod tests;
