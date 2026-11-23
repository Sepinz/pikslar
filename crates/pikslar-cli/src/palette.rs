//! `palette`: loads a palette file into a project, or writes a project's swatches as one. The
//! loading is the editor's own command, so a project comes out exactly as it would in the program.

use crate::export::Done;
use crate::failure::Failure;
use crate::palette_args::PaletteArgs;
use crate::paths::{check_name, check_target, require_extension};
use crate::project;
use pikslar_core::palette_ops::load::unlisted_used;
use pikslar_core::{Document, Editor, LimitError, LoadHow, PaletteCmd};
use pikslar_format::palette_file::table::export_swatches;
use pikslar_format::palette_file::{self, PaletteFormat};
use pikslar_format::upgrade::older_version;
use pikslar_format::{read_limited, write_atomic};
use std::path::{Path, PathBuf};

pub fn run(args: &PaletteArgs) -> Result<Done, Failure> {
    match args {
        PaletteArgs::Export {
            project,
            format,
            out,
            overwrite,
        } => export(project, *format, out.as_deref(), *overwrite),
        PaletteArgs::Import {
            project,
            file,
            how,
            format,
            out,
            overwrite,
        } => import(project, file, *how, *format, (out.as_deref(), *overwrite)),
    }
}

/// The format asked for, else the ending of `--out`, else GIMP's.
fn export_format(
    format: Option<PaletteFormat>,
    out: Option<&Path>,
) -> Result<PaletteFormat, Failure> {
    let ending = out.and_then(Path::extension).and_then(|e| e.to_str());
    match (format, ending.and_then(PaletteFormat::from_extension)) {
        (Some(format), _) => Ok(format),
        (None, Some(format)) => Ok(format),
        (None, None) if out.is_some() => Err(Failure::Usage(
            "--out must end in .gpl, .pal or .hex, or give --format".into(),
        )),
        (None, None) => Ok(PaletteFormat::Gpl),
    }
}

fn export(
    project: &Path,
    format: Option<PaletteFormat>,
    out: Option<&Path>,
    overwrite: bool,
) -> Result<Done, Failure> {
    let format = export_format(format, out)?;
    let target = out.map_or_else(|| project.with_extension(format.extension()), PathBuf::from);
    check_name(&target)?;
    require_extension(&target, format.extension())?;
    let doc = project::load(project)?;
    let swatches = export_swatches(&doc.colors);
    if swatches.palette.colors.is_empty() {
        return Err(Failure::Export(
            "The project has no swatches to export".into(),
        ));
    }
    check_target(&target, project, overwrite)?;
    let bytes = palette_file::write(&swatches.palette, format)
        .map_err(|e| Failure::Export(e.to_string()))?;
    write_atomic(&target, &bytes).map_err(|e| Failure::Write(e.to_string()))?;
    let mut notes = Vec::new();
    if swatches.partly_clear > 0 {
        notes.push(format!(
            "{} swatch(es) are partly clear; palette files have no alpha, so each is written as \
             its colour alone",
            swatches.partly_clear
        ));
    }
    Ok(Done {
        written: vec![target],
        notes,
    })
}

/// The palette file's colours, and what the reader changed or ignored.
fn read_palette(
    file: &Path,
    format: Option<PaletteFormat>,
) -> Result<palette_file::PaletteRead, Failure> {
    let fail = |e: &dyn std::fmt::Display| Failure::Read(format!("{}: {e}", file.display()));
    let limit = palette_file::MAX_FILE_BYTES as u64;
    if file.is_dir() {
        return Err(Failure::Read(format!("{} is a folder", file.display())));
    }
    let bytes = read_limited(file, limit).map_err(|e| fail(&e))?;
    let ending = file.extension().and_then(|e| e.to_str());
    let format = format.or_else(|| ending.and_then(PaletteFormat::from_extension));
    match format {
        Some(format) => palette_file::read(&bytes, format),
        None => palette_file::read_any(&bytes),
    }
    .map_err(|e| fail(&e))
}

/// The refusal for a command that did not run, in words for a script's log.
fn refusal(error: LimitError, swatches: usize, colors: usize) -> Failure {
    match error {
        LimitError::PaletteSize => Failure::Export(format!(
            "Load as colours needs as many colours as the project has swatches: the project \
             has {swatches}, the file has {colors}"
        )),
        other => Failure::Export(other.to_string()),
    }
}

/// Where the project is written, checked before anything is read: `--out` if given, else the
/// project itself, which must not be an Aseprite file or one an older version saved.
fn import_target<'a>(
    project: &'a Path,
    out: Option<&'a Path>,
    overwrite: bool,
) -> Result<&'a Path, Failure> {
    if project::is_aseprite(project) {
        return Err(Failure::Usage(
            "palette import changes a Pikslar project; Aseprite files are only ever read".into(),
        ));
    }
    let Some(target) = out else {
        return match older_version(project) {
            Some(version) => Err(Failure::Export(format!(
                "This project was saved by an older version of Pikslar (version {version}).                  Writing it back would make it a version 3 file that older versions cannot                  open, with no copy of the original. Give --out to write a new file and keep                  this one"
            ))),
            None => Ok(project),
        };
    };
    check_name(target)?;
    require_extension(target, "piks")?;
    check_target(target, project, overwrite)?;
    Ok(target)
}

/// Runs the import on the project, adding to `notes`. False if it changed nothing.
fn load_colors(
    editor: &mut Editor,
    read: &palette_file::PaletteRead,
    how: LoadHow,
    notes: &mut Vec<String>,
) -> Result<bool, Failure> {
    let colors = read.palette.packed();
    let swatches = editor.doc().colors.swatches().len();
    let command = PaletteCmd::Load {
        colors: colors.as_slice().into(),
        how,
    };
    let outcome = editor
        .apply(command)
        .map_err(|e| refusal(e, swatches, colors.len()))?;
    if !outcome.changed {
        notes.push("The palette already matches the file; nothing changed".into());
    }
    Ok(outcome.changed)
}

fn import(
    project: &Path,
    file: &Path,
    how: LoadHow,
    format: Option<PaletteFormat>,
    (out, overwrite): (Option<&Path>, bool),
) -> Result<Done, Failure> {
    let target = import_target(project, out, overwrite)?;
    let (doc, lossless) = project::load_noted(project)?;
    if !lossless && out.is_none() {
        return Err(Failure::Export(
            "Opening this project left something out or changed it, so writing it back would              lose it. Give --out to write a new file and keep this one"
                .into(),
        ));
    }
    let read = read_palette(file, format)?;
    let mut notes = read.notes.clone();
    let mut editor = Editor::new(doc);
    if !load_colors(&mut editor, &read, how, &mut notes)? && out.is_none() {
        return Ok(Done {
            written: Vec::new(),
            notes,
        });
    }
    notes.extend(after_notes(how, editor.doc()));
    project::save(target, editor.doc())?;
    Ok(Done {
        written: vec![target.to_path_buf()],
        notes,
    })
}

/// What the person should know about how the picture is affected.
fn after_notes(how: LoadHow, doc: &Document) -> Option<String> {
    match how {
        LoadHow::Append => None,
        LoadHow::Replace => {
            let left = unlisted_used(doc);
            (left > 0).then(|| {
                format!(
                    "{left} colour(s) that pixels still use are no longer in the palette; those \
                     pixels are unchanged"
                )
            })
        }
        LoadHow::AsColors => Some(
            "Every swatch now has the file's colour at its place, so the picture's colours have \
             changed; no pixel moved to another swatch"
                .into(),
        ),
    }
}
