//! `export`: works out every file to write, checks every target, then writes them as a set.
//! No file is ever whole in memory: each is written a band of rows at a time. What is written
//! is planned by the export library, as in the app, so the same choices give the same bytes.

use crate::args::{ExportArgs, Format};
use crate::failure::Failure;
use crate::paths::{check_name, check_target, require_extension};
use crate::project;
use pikslar_core::Document;
use pikslar_export::{
    layers_named, may_write_description, plan, stale_members, tmj_outputs, write_outputs,
    ExportError, Frames, Job, Kind, Layers, Layout, Output, Picture, SheetOptions,
};
use std::path::{Path, PathBuf};

fn export_failure(e: ExportError) -> Failure {
    match e {
        ExportError::Format(e) => Failure::Write(e.to_string()),
        ExportError::OneFrame { .. } => Failure::Export(format!(
            "{e}. Use --frame, --format sheet or --format sequence"
        )),
        other => Failure::Export(other.to_string()),
    }
}

/// The path asked for, or the project's name with the format's ending.
fn target(args: &ExportArgs) -> PathBuf {
    args.out
        .clone()
        .unwrap_or_else(|| args.project.with_extension(args.format.extension()))
}

/// What the arguments ask for, the defaults for what they do not.
fn job(args: &ExportArgs, kind: Kind) -> Job {
    Job {
        kind,
        frames: args.frames.clone().unwrap_or_else(|| kind.default_frames()),
        plays: args.plays,
        sheet: SheetOptions {
            layout: Layout {
                columns: args.columns,
                padding: args.padding.unwrap_or(0),
            },
            json: args.json.unwrap_or_default(),
        },
    }
}

/// Every file the export makes, not yet written, and anything the person should know.
pub fn outputs<'a>(
    args: &ExportArgs,
    kind: Kind,
    source: &'a Picture<'a>,
) -> Result<(Vec<Output<'a>>, Option<String>), Failure> {
    let path = target(args);
    check_name(&path)?;
    require_extension(&path, kind.extension())?;
    let planned = plan(&job(args, kind), source, path).map_err(export_failure)?;
    Ok((planned.outputs, planned.note))
}

/// Files from an earlier, longer sequence would read as part of this one.
fn refuse_stale(path: &Path, members: usize) -> Result<(), Failure> {
    let stale = stale_members(path, members).map_err(|e| Failure::Write(e.to_string()))?;
    let Some(first) = stale.first() else {
        return Ok(());
    };
    Err(Failure::Write(format!(
        "{} and {} other file(s) numbered past this export's {members} are left from an \
         earlier one, and would look like part of it. Delete them or choose another --out",
        first.display(),
        stale.len() - 1
    )))
}

/// Refuses the export before anything is written if any target is wrong.
fn check_all(args: &ExportArgs, files: &[Output<'_>]) -> Result<(), Failure> {
    for file in files {
        check_target(&file.path, &args.project, args.overwrite)?;
        if file.ours_only && !may_write_description(&file.path) {
            return Err(Failure::Write(format!(
                "{} was not written by Pikslar, so it is not replaced, even with --overwrite. \
                 Move it or choose another --out",
                file.path.display()
            )));
        }
    }
    if args.format == Format::Sequence {
        refuse_stale(&target(args), files.len() - 1)?;
    }
    Ok(())
}

/// The layers, scale and kind of PNG asked for, checked against the project before any file
/// is made.
pub fn picture<'a>(args: &ExportArgs, doc: &'a Document) -> Result<Picture<'a>, Failure> {
    let layers = if args.layers.is_empty() {
        Layers::Visible
    } else {
        layers_named(doc, &args.layers).map_err(export_failure)?
    };
    let picture = Picture::new(doc, &layers, args.scale.unwrap_or(1)).map_err(export_failure)?;
    Ok(picture.with_indexed(args.indexed))
}

/// What an export did: the paths written, in order, and anything the person should know.
pub struct Done {
    pub written: Vec<PathBuf>,
    pub notes: Vec<String>,
}

/// Writes the export.
pub fn run(args: &ExportArgs) -> Result<Done, Failure> {
    let doc = project::load(&args.project)?;
    let Some(kind) = args.format.kind() else {
        return map(args, &doc);
    };
    let source = picture(args, &doc)?;
    let (files, note) = outputs(args, kind, &source)?;
    write_all(args, files, note.into_iter().collect())
}

/// Checks every target, then writes the set.
fn write_all(
    args: &ExportArgs,
    mut files: Vec<Output<'_>>,
    notes: Vec<String>,
) -> Result<Done, Failure> {
    check_all(args, &files)?;
    write_outputs(&mut files, &mut |_, _| Ok(())).map_err(|e| Failure::Write(e.to_string()))?;
    Ok(Done {
        written: files.into_iter().map(|f| f.path).collect(),
        notes,
    })
}

/// A Tiled map of one frame, `--frame` or the first, and an atlas beside it for each tileset.
fn map(args: &ExportArgs, doc: &Document) -> Result<Done, Failure> {
    let path = target(args);
    check_name(&path)?;
    require_extension(&path, "tmj")?;
    let frame = match args.frames {
        Some(Frames::One(frame)) => frame,
        _ => 0,
    };
    let planned = tmj_outputs(doc, frame, path).map_err(export_failure)?;
    for file in &planned.outputs {
        check_name(&file.path)?;
    }
    write_all(args, planned.outputs, planned.notes)
}
