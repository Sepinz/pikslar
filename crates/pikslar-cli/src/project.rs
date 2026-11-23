//! Reading a project, with the same limits as the program.

use crate::failure::Failure;
use pikslar_core::Document;
use pikslar_format::upgrade::older_version;
use pikslar_format::{import_ase, piks, read_limited, write_atomic, FormatError, MAX_FILE_BYTES};
use std::io::Write;
use std::path::Path;

/// True for the names Aseprite files carry. They are only ever read, and from them only a
/// document is made: nothing here writes one.
pub fn is_aseprite(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ase") || e.eq_ignore_ascii_case("aseprite"))
}

fn warn(what: &dyn std::fmt::Display) {
    let _ = writeln!(std::io::stderr(), "pikslar-cli: warning: {what}");
}

/// Reads a project, or an Aseprite file. What the importer left out or changed, or what a
/// project could not keep as it was, is told on standard error, one line each, and is not a
/// failure.
pub fn load(path: &Path) -> Result<Document, Failure> {
    load_noted(path).map(|(doc, _)| doc)
}

/// [`load`], and whether writing the project back would keep everything it held: false if
/// opening left something out or changed it.
pub fn load_noted(path: &Path) -> Result<(Document, bool), Failure> {
    read_noted(path).map_err(|why| match why {
        Unreadable::Folder => Failure::Read(format!("{} is a folder", path.display())),
        Unreadable::Format(e) => Failure::Read(format!("{}: {e}", path.display())),
    })
}

/// Why a project could not be read, without saying where it is: a session tells its client
/// only the path the client gave.
pub enum Unreadable {
    Folder,
    Format(FormatError),
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::Folder => f.write_str("it is a folder"),
            Unreadable::Format(e) => write!(f, "{e}"),
        }
    }
}

/// [`load_noted`], failing with a reason that holds no path.
pub fn read_noted(path: &Path) -> Result<(Document, bool), Unreadable> {
    if path.is_dir() {
        return Err(Unreadable::Folder);
    }
    let bytes = read_limited(path, MAX_FILE_BYTES).map_err(Unreadable::Format)?;
    if !is_aseprite(path) {
        let (doc, notes) = piks::load_noted(&bytes).map_err(Unreadable::Format)?;
        notes.lines().iter().for_each(|line| warn(line));
        return Ok((doc, notes.can_save_over()));
    }
    let (doc, warnings) = import_ase(&bytes).map_err(Unreadable::Format)?;
    warnings.iter().for_each(|warning| warn(warning));
    Ok((doc, false))
}

/// Why the project at `path` must not be written back over itself, if it must not: an
/// Aseprite file, one an older version saved, or one that opening left something out of.
pub fn write_over_refusal(path: &Path, lossless: bool) -> Option<String> {
    if is_aseprite(path) {
        return Some("Aseprite files are only ever read, never written".into());
    }
    if let Some(version) = older_version(path) {
        return Some(format!(
            "This project was saved by an older version of Pikslar (version {version}). \
             Writing it back would make it a version 3 file that older versions cannot open"
        ));
    }
    (!lossless).then(|| {
        "Opening this project left something out or changed it, so writing it back would \
         lose it"
            .into()
    })
}

/// Writes a project beside the target and renames it into place, so a failed save never
/// damages the file there.
pub fn save(target: &Path, doc: &Document) -> Result<(), Failure> {
    let bytes = piks::try_save(doc).map_err(|e| Failure::Write(e.to_string()))?;
    write_atomic(target, &bytes).map_err(|e| Failure::Write(e.to_string()))
}
