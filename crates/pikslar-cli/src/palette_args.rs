//! `palette import` and `palette export`: what was asked for. Parsing only.

use crate::args::{set_once, text, unknown, value, value_os, Command};
use crate::failure::Failure;
use pikslar_core::LoadHow;
use pikslar_format::palette_file::PaletteFormat;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum PaletteArgs {
    /// Writes a project's swatches, in order, as a palette file.
    Export {
        project: PathBuf,
        format: Option<PaletteFormat>,
        out: Option<PathBuf>,
        overwrite: bool,
    },
    /// Loads a palette file into a project.
    Import {
        project: PathBuf,
        file: PathBuf,
        how: LoadHow,
        format: Option<PaletteFormat>,
        out: Option<PathBuf>,
        overwrite: bool,
    },
}

/// Reads the arguments after `palette`.
pub fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Command, Failure> {
    let Some(first) = args.next() else {
        return Err(Failure::Usage("palette needs import or export".into()));
    };
    match text(first)?.as_str() {
        "--help" | "-h" => Ok(Command::Help(Some("palette".into()))),
        "export" => options(args, false),
        "import" => options(args, true),
        other => Err(Failure::Usage(format!(
            "Unknown palette command \"{other}\". Use import or export"
        ))),
    }
}

fn options(mut args: impl Iterator<Item = OsString>, import: bool) -> Result<Command, Failure> {
    let (mut files, mut how, mut format) = (Vec::new(), None, None);
    let (mut out, mut overwrite) = (None, false);
    while let Some(arg) = args.next() {
        let Some(flag) = arg.to_str().filter(|a| a.starts_with('-')) else {
            files.push(PathBuf::from(arg));
            continue;
        };
        match flag {
            "--help" | "-h" => return Ok(Command::Help(Some("palette".into()))),
            "--overwrite" => overwrite = true,
            "--format" => set_once(&mut format, kind(&value(&mut args, flag)?)?, flag)?,
            "--out" | "-o" => {
                set_once(&mut out, PathBuf::from(value_os(&mut args, flag)?), "--out")?
            }
            "--how" if import => set_once(&mut how, way(&value(&mut args, flag)?)?, flag)?,
            other => return Err(unknown(other)),
        }
    }
    let wanted = if import { 2 } else { 1 };
    if files.len() != wanted {
        return Err(Failure::Usage(if import {
            "palette import needs a project and a palette file".into()
        } else {
            "palette export needs a project file".into()
        }));
    }
    let project = files.remove(0);
    Ok(Command::Palette(if import {
        PaletteArgs::Import {
            project,
            file: files.remove(0),
            how: how.unwrap_or(LoadHow::Append),
            format,
            out,
            overwrite,
        }
    } else {
        PaletteArgs::Export {
            project,
            format,
            out,
            overwrite,
        }
    }))
}

fn kind(name: &str) -> Result<PaletteFormat, Failure> {
    PaletteFormat::from_extension(name).ok_or_else(|| {
        Failure::Usage(format!(
            "Unknown palette format \"{name}\". Formats: gpl, pal, hex"
        ))
    })
}

fn way(name: &str) -> Result<LoadHow, Failure> {
    match name {
        "append" => Ok(LoadHow::Append),
        "replace" => Ok(LoadHow::Replace),
        "colors" | "colours" => Ok(LoadHow::AsColors),
        other => Err(Failure::Usage(format!(
            "Unknown --how \"{other}\". Ways: append (the default), replace, colors"
        ))),
    }
}
