//! The brush library on disk: `brushes.pkb` in the settings folder.
//!
//! Nothing is read until the library is first needed (the brush menu opening, or a brush being
//! saved), so launch is unchanged. A file that is partly damaged is kept once as
//! `brushes.damaged.pkb` before anything is written, a file from a newer version is never
//! written over, and a folder that cannot be written only costs the library, never a project.

use pikslar_core::custom_brush::{Brush, BrushPaint, Handle};
use pikslar_core::orient::Orientation;
use pikslar_core::rgba;
use pikslar_format::brush_lib::{
    self, load_library, save_library, LibraryBrush, Paint, MAX_BRUSHES, MAX_NAME_BYTES,
};
use pikslar_format::FormatError;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

pub(crate) const FILE_NAME: &str = "brushes.pkb";
/// Most characters a brush's name keeps when it is typed, as a layer's. Sixty-four characters
/// are at most 256 bytes, the file's limit.
pub(crate) const MAX_BRUSH_NAME: usize = 64;
const DAMAGED_NAME: &str = "brushes.damaged.pkb";
/// How many damaged files are kept aside: `brushes.damaged.pkb`, `brushes.damaged.2.pkb`, ...
const MAX_ASIDE: usize = 5;

/// Length and modified time of the file when it was read; `None` for a missing file.
type Stamp = Option<(u64, Option<SystemTime>)>;

fn stamp_of(path: &Path) -> Stamp {
    let meta = fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

/// The name of the n-th (1-based) place a damaged file can be kept.
fn aside_name(n: usize) -> String {
    if n == 1 {
        DAMAGED_NAME.to_string()
    } else {
        format!("brushes.damaged.{n}.pkb")
    }
}

/// What the library holds once it has been read.
struct Loaded {
    brushes: Vec<LibraryBrush>,
    /// Why nothing may be written, if so.
    locked: Option<String>,
    /// The file as it was when `brushes` was read from it or last written to it.
    stamp: Stamp,
    /// Changes whenever `brushes` may have, and is never the same in two lists.
    version: u64,
}

/// A number no list of brushes has had before in this run.
fn next_version() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Where the library is, and what was read from it. Reads nothing until [`ensure_read`].
///
/// [`ensure_read`]: BrushStore::ensure_read
#[derive(Default)]
pub(crate) struct BrushStore {
    path: Option<PathBuf>,
    loaded: Option<Loaded>,
}

/// What saving a brush did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Saved {
    Added(String),
    /// The same brush was already there.
    AlreadyThere,
}

impl BrushStore {
    /// A library at `path`, or none: tests and `--no-settings` runs keep no library.
    pub fn at(path: Option<PathBuf>) -> Self {
        BrushStore { path, loaded: None }
    }

    #[cfg(test)]
    /// Whether the file has been looked at.
    pub fn is_read(&self) -> bool {
        self.loaded.is_some()
    }

    #[cfg(test)]
    pub fn is_enabled(&self) -> bool {
        self.path.is_some()
    }

    /// The brushes, empty until the library has been read.
    pub fn brushes(&self) -> &[LibraryBrush] {
        self.loaded.as_ref().map_or(&[], |l| &l.brushes)
    }

    /// A number that changes whenever [`brushes`](Self::brushes) may have: 0 until the library
    /// is read, and never the same for two lists, even of two stores. Pictures of the brushes
    /// are made again only when it changes.
    pub fn version(&self) -> u64 {
        self.loaded.as_ref().map_or(0, |l| l.version)
    }

    /// Reads the library the first time; later calls do nothing. Returns what is worth telling
    /// the artist: brushes lost, a file kept aside, or a library that will not be written.
    pub fn ensure_read(&mut self) -> Option<String> {
        if self.loaded.is_some() {
            return None;
        }
        let (loaded, message) = match &self.path {
            None => (Loaded::empty(None), None),
            Some(path) => {
                let before = stamp_of(path);
                let (mut loaded, message) = read_file(path);
                // Moved aside, the file is gone; otherwise it is what was looked at.
                loaded.stamp = if path.exists() { before } else { None };
                (loaded, message)
            }
        };
        self.loaded = Some(loaded);
        message
    }

    /// Puts a brush in the library and writes it. A name already used is made unique.
    pub fn add(&mut self, mut brush: LibraryBrush) -> Result<Saved, String> {
        let (path, loaded) = self.writable()?;
        if loaded.brushes.contains(&brush) {
            return Ok(Saved::AlreadyThere);
        }
        if loaded.brushes.len() >= MAX_BRUSHES {
            return Err(format!("The library holds {MAX_BRUSHES} brushes"));
        }
        brush.name = unique_name(&brush.name, &loaded.brushes, None);
        let mut next = loaded.brushes.clone();
        let name = brush.name.clone();
        next.push(brush);
        loaded.commit(&path, next)?;
        Ok(Saved::Added(name))
    }

    /// Takes the brush at `index` out of the library and writes it. Returns its name. If the
    /// file changed meanwhile, the same brush is looked for in the new list.
    pub fn remove(&mut self, index: usize) -> Result<String, String> {
        let (path, at, loaded) = self.find_writable(index)?;
        let mut next = loaded.brushes.clone();
        let gone = next.remove(at);
        loaded.commit(&path, next)?;
        Ok(gone.name)
    }

    /// Gives the brush at `index` the name `typed`, cleaned as a layer's name is (see
    /// [`clean_name`]) and numbered if another brush has it, and writes the library. Returns
    /// the new name, or `None` when there is nothing to write: an empty name or the same one.
    /// Like saving and removing, this is no step of undo: the library is not the document.
    pub fn rename(&mut self, index: usize, typed: &str) -> Result<Option<String>, String> {
        let name = clean_name(typed);
        if name.is_empty() {
            return Ok(None);
        }
        let (path, at, loaded) = self.find_writable(index)?;
        let name = unique_name(&name, &loaded.brushes, Some(at));
        if loaded.brushes[at].name == name {
            return Ok(None);
        }
        let mut next = loaded.brushes.clone();
        next[at].name.clone_from(&name);
        loaded.commit(&path, next)?;
        Ok(Some(name))
    }

    /// The file, where the brush shown at `index` is now, and the library, or why it cannot be
    /// written. If the file changed meanwhile, the same brush is looked for in the new list.
    fn find_writable(&mut self, index: usize) -> Result<(PathBuf, usize, &mut Loaded), String> {
        let target = self
            .loaded
            .as_ref()
            .and_then(|l| l.brushes.get(index))
            .cloned();
        let (path, loaded) = self.writable()?;
        let gone = || "That brush is no longer in the library".to_string();
        let target = target.ok_or_else(gone)?;
        let at = loaded
            .brushes
            .iter()
            .position(|b| *b == target)
            .ok_or_else(gone)?;
        Ok((path, at, loaded))
    }

    /// The file and the library, or why neither may be written. A file that changed since it
    /// was read is read again first, so what is written builds on what is there.
    fn writable(&mut self) -> Result<(PathBuf, &mut Loaded), String> {
        let Some(path) = self.path.clone() else {
            return Err("Brushes are not kept between runs while settings are off".to_string());
        };
        let Some(loaded) = self.loaded.as_mut() else {
            return Err("The brush library has not been read".to_string());
        };
        if let Some(why) = &loaded.locked {
            return Err(why.clone());
        }
        loaded.refresh(&path)?;
        Ok((path, loaded))
    }
}

impl Loaded {
    fn empty(locked: Option<String>) -> Self {
        Loaded {
            brushes: Vec::new(),
            locked,
            stamp: None,
            version: next_version(),
        }
    }

    /// Reads the file again if its length or time differ from what was read. Another
    /// Pikslar may have saved, or a newer build may have upgraded it.
    fn refresh(&mut self, path: &Path) -> Result<(), String> {
        let now = stamp_of(path);
        if now == self.stamp {
            return Ok(());
        }
        match load_library(path) {
            Ok(lib) if lib.report.newer_version => {
                let why = "The brush library was changed by a newer Pikslar and is only read";
                self.locked = Some(why.to_string());
                Err(why.to_string())
            }
            Ok(lib) if lib.report.is_clean() => {
                self.brushes = lib.brushes;
                self.stamp = now;
                self.version = next_version();
                Ok(())
            }
            Err(FormatError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                self.brushes.clear();
                self.stamp = now;
                self.version = next_version();
                Ok(())
            }
            _ => Err(
                "The brush library was changed outside this run and could not be \
                      read cleanly, so it is left alone"
                    .to_string(),
            ),
        }
    }

    /// Writes `next` and, once it is on disk, makes it the library.
    fn commit(&mut self, path: &Path, next: Vec<LibraryBrush>) -> Result<(), String> {
        write(path, &next)?;
        self.brushes = next;
        self.stamp = stamp_of(path);
        self.version = next_version();
        Ok(())
    }
}

fn write(path: &Path, brushes: &[LibraryBrush]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("The brush library was not saved: {e}"))?;
    }
    save_library(path, brushes).map_err(|e| match e {
        FormatError::FileTooLarge => "The library is full of pixels: remove a brush".to_string(),
        e => format!("The brush library was not saved: {e}"),
    })
}

/// Reads the file. A missing one is an empty library.
fn read_file(path: &Path) -> (Loaded, Option<String>) {
    match load_library(path) {
        Ok(lib) => adopt(path, lib),
        Err(FormatError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
            (Loaded::empty(None), None)
        }
        Err(FormatError::Io(e)) => {
            let why = format!("The brush library could not be read ({e}), so it is left alone");
            (Loaded::empty(Some(why.clone())), Some(why))
        }
        Err(e) => set_aside(path, &e),
    }
}

fn adopt(path: &Path, lib: brush_lib::Library) -> (Loaded, Option<String>) {
    let report = &lib.report;
    if report.newer_version {
        let why = "The brush library is from a newer Pikslar and is only read".to_string();
        let loaded = Loaded {
            brushes: lib.brushes,
            locked: Some(why.clone()),
            stamp: None,
            version: next_version(),
        };
        return (loaded, Some(why));
    }
    if report.is_clean() {
        return (Loaded::empty(None).with(lib.brushes), None);
    }
    let said = if report.lost > 0 {
        format!("{} brush(es) in the library could not be read", report.lost)
    } else {
        "The brush library is larger than the limits and was cut".to_string()
    };
    match keep_copy(path) {
        Ok(name) => (
            Loaded::empty(None).with(lib.brushes),
            Some(format!("{said}. The file was kept as {name}")),
        ),
        Err(e) => {
            let why = format!("{said}, and it could not be kept aside ({e}), so it is left alone");
            let loaded = Loaded {
                brushes: lib.brushes,
                locked: Some(why.clone()),
                stamp: None,
                version: next_version(),
            };
            (loaded, Some(why))
        }
    }
}

/// A file that is no library at all moves aside, so the first save cannot write over it.
fn set_aside(path: &Path, e: &FormatError) -> (Loaded, Option<String>) {
    let moved = free_aside(path).and_then(|(name, kept)| fs::rename(path, kept).map(|()| name));
    match moved {
        Ok(name) => (
            Loaded::empty(None),
            Some(format!(
                "The brush library could not be read ({e}). It was kept as {name}"
            )),
        ),
        Err(io) => {
            let why =
                format!("The brush library could not be read ({e}) or kept aside ({io}), so it is left alone");
            (Loaded::empty(Some(why.clone())), Some(why))
        }
    }
}

impl Loaded {
    fn with(self, brushes: Vec<LibraryBrush>) -> Loaded {
        Loaded { brushes, ..self }
    }
}

/// The first of the aside names that is not taken, with its path. Nothing is ever deleted.
fn free_aside(path: &Path) -> io::Result<(String, PathBuf)> {
    (1..=MAX_ASIDE)
        .map(aside_name)
        .map(|name| {
            let kept = path.with_file_name(&name);
            (name, kept)
        })
        .find(|(_, kept)| fs::symlink_metadata(kept).is_err())
        .ok_or_else(|| {
            io::Error::other(format!(
                "all {MAX_ASIDE} places for damaged copies are taken, remove some"
            ))
        })
}

/// Keeps a copy of the damaged file and returns the name it is under: an earlier copy of the
/// same bytes is reused, otherwise the next free name is written. Files are compared by
/// length and then in small pieces, never read whole.
fn keep_copy(path: &Path) -> io::Result<String> {
    for n in 1..=MAX_ASIDE {
        let name = aside_name(n);
        if same_bytes(path, &path.with_file_name(&name))? {
            return Ok(name);
        }
    }
    let (name, kept) = free_aside(path)?;
    fs::copy(path, kept)?;
    Ok(name)
}

/// Whether both files exist and hold the same bytes.
fn same_bytes(a: &Path, b: &Path) -> io::Result<bool> {
    let (Ok(ma), Ok(mb)) = (fs::metadata(a), fs::metadata(b)) else {
        return Ok(false);
    };
    if ma.len() != mb.len() {
        return Ok(false);
    }
    let mut fa = io::BufReader::new(fs::File::open(a)?);
    let mut fb = io::BufReader::new(fs::File::open(b)?);
    let (mut ba, mut bb) = ([0u8; 8192], [0u8; 8192]);
    loop {
        let n = fa.read(&mut ba)?;
        if n == 0 {
            return Ok(true);
        }
        fb.read_exact(&mut bb[..n])?;
        if ba[..n] != bb[..n] {
            return Ok(false);
        }
    }
}

/// `name`, or `name 2`, `name 3`... when a brush of `taken` other than the one at `skip`
/// already has it, within both name limits.
fn unique_name(name: &str, taken: &[LibraryBrush], skip: Option<usize>) -> String {
    let free = |n: &str| {
        taken
            .iter()
            .enumerate()
            .all(|(i, b)| Some(i) == skip || b.name != n)
    };
    if free(name) {
        return name.to_string();
    }
    // A library holds at most 64 brushes, so the number takes at most three characters.
    let stem = cut_chars(cut_name(name, MAX_NAME_BYTES - 8), MAX_BRUSH_NAME - 4);
    let stem = stem.trim_end();
    (2..)
        .map(|n| format!("{stem} {n}"))
        .find(|n| free(n))
        .unwrap_or_default()
}

/// A typed name as the library keeps it, by the rule every name follows (`pikslar_core::names`,
/// as a layer's name): hidden characters left out, trimmed, and at most [`MAX_BRUSH_NAME`]
/// characters, which is within the file's byte limit.
pub(crate) fn clean_name(typed: &str) -> String {
    let kept = pikslar_core::names::without_hidden(typed);
    let cut = cut_chars(kept.trim(), MAX_BRUSH_NAME).trim_end();
    cut_name(cut, MAX_NAME_BYTES).to_string()
}

/// At most `most` characters of `name`.
fn cut_chars(name: &str, most: usize) -> &str {
    name.char_indices()
        .nth(most)
        .map_or(name, |(end, _)| &name[..end])
}

/// At most `most` bytes of `name`, cut on a character boundary.
pub(crate) fn cut_name(name: &str, most: usize) -> &str {
    let mut end = name.len().min(most);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// The brush as the file holds it. Colours are the cell format's bytes, which is red, green,
/// blue, alpha in memory order. Palette ids stay behind: they belong to one document.
pub(crate) fn to_library(brush: &Brush) -> LibraryBrush {
    let (width, height) = brush.source_size();
    LibraryBrush {
        name: cut_name(brush.name(), MAX_NAME_BYTES).to_string(),
        width: width as u16,
        height: height as u16,
        paint: match brush.paint() {
            BrushPaint::OwnColors => Paint::OwnColors,
            BrushPaint::Shape => Paint::Shape,
        },
        orientation: brush.orientation().index(),
        handle: match brush.handle() {
            Handle::Centre => brush_lib::Handle::Centre,
            Handle::Corner(c) => brush_lib::Handle::Corner(c),
        },
        rgba: brush
            .colors()
            .iter()
            .flat_map(|c| c.to_le_bytes())
            .collect(),
    }
}

/// The brush a library entry describes, or `None` if it is empty or does not fit the model.
pub(crate) fn from_library(entry: &LibraryBrush) -> Option<Brush> {
    let colors: Vec<u32> = entry
        .rgba
        .chunks_exact(4)
        .map(|p| rgba(p[0], p[1], p[2], p[3]))
        .collect();
    let paint = match entry.paint {
        Paint::OwnColors => BrushPaint::OwnColors,
        Paint::Shape => BrushPaint::Shape,
    };
    let brush = Brush::from_parts(
        &entry.name,
        entry.width.into(),
        entry.height.into(),
        &colors,
        None,
        paint,
    )
    .ok()?;
    let handle = match entry.handle {
        brush_lib::Handle::Centre => Handle::Centre,
        brush_lib::Handle::Corner(c) => Handle::Corner(c.min(3)),
    };
    let turned = brush.turned(Orientation::from_index(entry.orientation)?);
    Some(turned.with_handle(handle))
}

#[cfg(test)]
#[path = "brush_store_tests.rs"]
mod tests;
