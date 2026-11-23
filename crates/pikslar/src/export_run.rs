//! Exporting from the app, through the same plan as the command line (decision D1): it reads
//! the document as shown, floating pixels included where they are, from a copy, and changes
//! nothing in the project. No step, no anchoring, no change to the selection or the saved state.

use crate::app::App;
use crate::diag::{note, Event};
use crate::export_dialog::{filter, ExportChoice};
use crate::export_map::{MAP_ENDING, MAP_FILTER};
use crate::session::{confirm_corrected, with_required_extension};
use pikslar_export::targets::{check_name, check_target, Refused};
use pikslar_export::{
    may_write_description, plan, stale_members, write_outputs_locked, ExportError, Kind, Layers,
    Output, Picture,
};
use pikslar_platform::{ask_replace_file, save_file_dialog, show_error, Key, Mods, Window};
use std::path::{Path, PathBuf};

impl App {
    /// While the Export panel is open the keyboard belongs to it. Tab and the arrows move focus,
    /// Space acts on what has it, and so does Enter, which otherwise exports. Escape closes, and
    /// 1 to 5 choose the kind of file as the buttons are numbered left to right.
    pub(crate) fn export_key(
        &mut self,
        choice: ExportChoice,
        key: Key,
        mods: Mods,
        window: Window,
    ) {
        self.ui_dirty = true;
        if key != Key::Escape && (self.focus_key(key, mods) || self.focus_space_key(key)) {
            return;
        }
        match key {
            Key::Enter => self.export_chosen(window),
            Key::Escape => self.close_export_dialog(),
            Key::Char(digit @ b'1'..=b'5') => {
                let kind = pikslar_export::Kind::ALL[usize::from(digit - b'1')];
                self.export_dialog = Some(choice.with_kind(kind));
                self.full_canvas |= kind != choice.kind || choice.map;
            }
            // 6 is TMJ, which is there while the document has a tilemap layer.
            Key::Char(b'6') if self.has_tilemaps() => {
                self.export_dialog = Some(choice.with_map());
                self.full_canvas |= !choice.map;
            }
            _ => {}
        }
    }

    /// Opens the export panel with the last choices, or a sheet's if `sheet`.
    pub fn open_export_dialog(&mut self, sheet: bool) {
        let doc = self.editor.doc();
        let mut choice = self.export_choice.fitted(&doc.tags, doc.frames.len());
        if sheet {
            choice = choice.with_kind(Kind::Sheet);
        }
        self.export_dialog = Some(choice);
        self.ui.focus.clear();
        self.ui_dirty = true;
    }

    pub fn close_export_dialog(&mut self) {
        self.ui.focus.clear();
        self.export_dialog = None;
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    /// Asks where, then exports what the panel chose. The choice is kept for the next export.
    pub fn export_chosen(&mut self, window: Window) {
        let Some(choice) = self.export_dialog else {
            return;
        };
        self.export_choice = choice.settled();
        self.close_export_dialog();
        let (ending, filter) = match choice.map {
            true => (MAP_ENDING, MAP_FILTER),
            false => (choice.kind.extension(), filter(choice.kind)),
        };
        let suggestion = self.path.as_ref().map(|p| p.with_extension(ending));
        let Some(chosen) = save_file_dialog(window, filter, ending, suggestion.as_deref()) else {
            return;
        };
        let target = with_required_extension(chosen.clone(), ending);
        let Some(path) = confirm_corrected(window, &chosen, target) else {
            return;
        };
        let (started, name) = (std::time::Instant::now(), path.clone());
        let ask = &mut |name: &str| ask_replace_file(window, name);
        let result = match choice.map {
            true => self.export_map_as(path, ask),
            false => self.export_as(&choice, path, ask),
        };
        match result {
            Ok(Some(done)) => {
                note(Event::new("export", "ok").took(started).file(&name));
                self.notify(done);
            }
            Ok(None) => note(Event::new("export", "cancel").file(&name)),
            Err(message) => {
                note(Event::new("export", "fail").took(started).file(&name));
                show_error(window, &message);
            }
        }
    }

    /// Exports `choice` to `path`. `ask` is asked before replacing files the save dialog did not
    /// ask about; a no writes nothing and returns None. Otherwise returns what to tell the
    /// person, or why nothing was written.
    pub(crate) fn export_as(
        &mut self,
        choice: &ExportChoice,
        path: PathBuf,
        ask: &mut dyn FnMut(&str) -> bool,
    ) -> Result<Option<String>, String> {
        check_name(&path).map_err(|e| e.to_string())?;
        let project = self.path.clone();
        let (frame, layer) = (self.editor.frame, self.editor.layer);
        let doc = self.editor.shown_document();
        let job = choice.job(frame, &doc.tags).ok_or_else(|| {
            "The tag chosen is no longer in the project. Choose the frames again".to_string()
        })?;
        let layers = match choice.this_layer {
            true => Layers::Only(vec![layer]),
            false => Layers::Visible,
        };
        let picture = Picture::new(&doc, &layers, choice.scale).map_err(app_message)?;
        let picture = picture.with_indexed(choice.indexed());
        let planned = plan(&job, &picture, path.clone()).map_err(app_message)?;
        let mut outputs = planned.outputs;
        // The set is locked by the file it ends with, even when that file is kept out.
        let set = outputs
            .last()
            .map_or_else(|| path.clone(), |o| o.path.clone());
        let Some(kept) = settle(&mut outputs, (&path, job.kind), project.as_deref(), ask)? else {
            return Ok(None);
        };
        write_outputs_locked(&mut outputs, &set, &mut |_, _| Ok(())).map_err(|e| e.to_string())?;
        let mut done = format!("Exported {}", path.display());
        if kept {
            done.push_str(", keeping the .json already there");
        }
        if let Some(note) = planned.note {
            done = format!("{done}. {note}");
        }
        Ok(Some(done))
    }
}

/// Checks every target before anything is written, and leaves out a description someone else
/// wrote, which is kept. Returns whether one was, or None if the person chose not to replace
/// files the save dialog did not ask about.
pub(crate) fn settle(
    outputs: &mut Vec<Output<'_>>,
    (path, kind): (&Path, Kind),
    project: Option<&Path>,
    ask: &mut dyn FnMut(&str) -> bool,
) -> Result<Option<bool>, String> {
    if kind == Kind::Sequence {
        refuse_stale(path, outputs.len() - 1)?;
    }
    let keep = outputs
        .last()
        .is_some_and(|last| last.ours_only && !may_write_description(&last.path));
    if keep {
        outputs.pop();
    }
    // Everything that would refuse is found before the person is asked anything.
    for output in outputs.iter() {
        check_target(&output.path, project, true).map_err(|e: Refused| e.to_string())?;
    }
    // A description Pikslar wrote is part of the export the dialog asked about.
    let unasked: Vec<&Path> = outputs
        .iter()
        .filter(|o| !o.ours_only)
        .map(|o| o.path.as_path())
        .filter(|p| *p != path && p.exists())
        .collect();
    if let Some(first) = unasked.first() {
        let name = first
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let more = unasked.len() - 1;
        let name = if more > 0 {
            format!("{name} and {more} other file(s)")
        } else {
            name
        };
        if !ask(&name) {
            return Ok(None);
        }
    }
    Ok(Some(keep))
}

/// Files from an earlier, longer sequence would read as part of this one.
fn refuse_stale(path: &Path, members: usize) -> Result<(), String> {
    let stale = stale_members(path, members).map_err(|e| e.to_string())?;
    let Some(first) = stale.first() else {
        return Ok(());
    };
    Err(format!(
        "{} and {} other file(s) numbered past this export's {members} are left from an earlier \
         one, and would look like part of it. Delete them or choose another name",
        first.display(),
        stale.len() - 1
    ))
}

/// The library's message, in the app's words where it names a command line option.
pub(crate) fn app_message(e: ExportError) -> String {
    match e {
        ExportError::OneFrame { .. } => "A PNG holds one frame. Choose This frame".into(),
        other => other
            .to_string()
            .replace("without --indexed", "without Indexed")
            .replace("no --indexed", "not Indexed")
            .replace("--format sequence", "Frames"),
    }
}
