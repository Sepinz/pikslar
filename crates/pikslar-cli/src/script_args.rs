//! `batch` and `session`: what was asked for. Parsing only: nothing here touches a file.

use crate::args::{set_once, text, unknown, value_os, Command};
use crate::failure::Failure;
use std::ffi::OsString;
use std::path::PathBuf;

/// Where a batch's operations come from.
#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    Stdin,
    File(PathBuf),
}

#[derive(Debug, PartialEq, Eq)]
pub struct BatchArgs {
    pub project: PathBuf,
    pub ops: Source,
    pub out: Option<PathBuf>,
    pub overwrite: bool,
}

/// Reads the arguments after `batch`.
pub fn batch(mut args: impl Iterator<Item = OsString>) -> Result<Command, Failure> {
    let (mut files, mut out, mut overwrite) = (Vec::new(), None, false);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--help" | "-h") => return Ok(Command::Help(Some("batch".into()))),
            Some("--overwrite") => overwrite = true,
            Some(flag @ ("--out" | "-o")) => {
                set_once(&mut out, PathBuf::from(value_os(&mut args, flag)?), "--out")?
            }
            Some("-") => files.push(None),
            Some(flag) if flag.starts_with('-') => return Err(unknown(flag)),
            _ => files.push(Some(PathBuf::from(arg))),
        }
    }
    let mut files = files.into_iter();
    let (Some(Some(project)), Some(ops), None) = (files.next(), files.next(), files.next()) else {
        return Err(Failure::Usage(
            "batch needs a project and an operations file (or - for standard input)".into(),
        ));
    };
    let ops = ops.map_or(Source::Stdin, Source::File);
    Ok(Command::Batch(BatchArgs {
        project,
        ops,
        out,
        overwrite,
    }))
}

/// Reads the arguments after `session`: `--dir <folder>`, the only place it may open or save.
pub fn session(mut args: impl Iterator<Item = OsString>) -> Result<Command, Failure> {
    let mut dir = None;
    while let Some(arg) = args.next() {
        match text(arg)?.as_str() {
            "--help" | "-h" => return Ok(Command::Help(Some("session".into()))),
            flag @ "--dir" => {
                let folder = PathBuf::from(value_os(&mut args, flag)?);
                set_once(&mut dir, folder, flag)?
            }
            other => return Err(unknown(other)),
        }
    }
    let dir = dir.ok_or_else(|| {
        Failure::Usage("session needs --dir <folder>, the folder it may open and save in".into())
    })?;
    Ok(Command::Session { dir })
}
