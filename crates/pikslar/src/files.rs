//! Turning files into documents and documents into files.

use pikslar_core::{ColorMode, ColorTable, Document, Grid, LimitError};
use pikslar_format::atomic::SaveStep;
use pikslar_format::palette_file::table::export_swatches;
use pikslar_format::palette_file::{self, PaletteError, PaletteFormat, PaletteRead};
use pikslar_format::png::{self, Image};
use pikslar_format::publish::{write_atomic_with, Placed};
use pikslar_format::upgrade::{self, UpgradeError};
use pikslar_format::write_atomic;
use pikslar_format::{
    import_ase_until, piks, read_limited, AseWarning, FormatError, MAX_FILE_BYTES,
};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Images with at most this many colors arrive as palette art.
const PALETTE_IMPORT_LIMIT: usize = 256;

#[derive(Debug)]
pub enum FileError {
    Format(FormatError),
    Limit(LimitError),
    /// What was about to be saved would not open again, so nothing was replaced.
    Unverified,
    Upgrade(UpgradeError),
    /// Export refused what was asked, for a reason only export has.
    Export(pikslar_export::ExportError),
    /// A palette file was refused; its message names the line.
    Palette(PaletteError),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::Format(e) => e.fmt(f),
            FileError::Limit(e) => e.fmt(f),
            FileError::Unverified => f.write_str(
                "The project could not be checked before saving, so the file was left as it was",
            ),
            FileError::Upgrade(e) => e.fmt(f),
            FileError::Export(e) => e.fmt(f),
            FileError::Palette(e) => e.fmt(f),
        }
    }
}

impl From<FormatError> for FileError {
    fn from(e: FormatError) -> Self {
        FileError::Format(e)
    }
}

impl From<LimitError> for FileError {
    fn from(e: LimitError) -> Self {
        FileError::Limit(e)
    }
}

impl From<pikslar_export::ExportError> for FileError {
    fn from(e: pikslar_export::ExportError) -> Self {
        match e {
            pikslar_export::ExportError::Format(e) => FileError::Format(e),
            pikslar_export::ExportError::Limit(e) => FileError::Limit(e),
            other => FileError::Export(other),
        }
    }
}

impl From<PaletteError> for FileError {
    fn from(e: PaletteError) -> Self {
        FileError::Palette(e)
    }
}

impl From<std::io::Error> for FileError {
    fn from(e: std::io::Error) -> Self {
        FileError::Format(FormatError::Io(e))
    }
}

/// Builds a one-layer document from an image, choosing the color mode that suits it.
pub fn document_from_image(image: &Image) -> Result<Document, FileError> {
    let mut ids: HashMap<u32, u32> = HashMap::new();
    let mut table = ColorTable::new();
    for &px in image.px.iter().filter(|&&p| p != 0) {
        if !ids.contains_key(&px) {
            if ids.len() == PALETTE_IMPORT_LIMIT {
                let mut doc = Document::new(image.width, image.height, ColorMode::Rgba)?;
                doc.set_cel(0, 0, Grid::from_cells(image.width, image.height, &image.px))?;
                return Ok(doc);
            }
            ids.insert(px, table.add_swatch(px).map_err(LimitError::from)?);
        }
    }
    let cells: Vec<u32> = image
        .px
        .iter()
        .map(|p| ids.get(p).copied().unwrap_or(0))
        .collect();
    let mut doc = Document::new(image.width, image.height, ColorMode::Palette)?;
    doc.set_cel(0, 0, Grid::from_cells(image.width, image.height, &cells))?;
    doc.colors = Arc::new(table);
    Ok(doc)
}

fn has_extension(path: &Path, wanted: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(wanted))
}

/// True for the names Aseprite files carry. They are only ever read.
pub fn is_aseprite(path: &Path) -> bool {
    has_extension(path, "ase") || has_extension(path, "aseprite")
}

/// Far above what the largest allowed image can need, even stored without compression.
const MAX_PNG_BYTES: u64 = 1 << 30;

/// A file turned into a document, and anything the person should hear about it.
pub struct Opened {
    pub doc: Document,
    pub notes: piks::LoadNotes,
    /// What an Aseprite file held that was left out or changed. Empty for every other file.
    pub warnings: Vec<AseWarning>,
    /// The print of a project's bytes, for Rewind, when it was asked for.
    pub print: Option<u128>,
    /// The file's write time and size just before it was read.
    pub stamp: Option<crate::recovery::FileStamp>,
}

impl Opened {
    /// The one line to show after opening, if there is anything to tell.
    pub fn notice(&self) -> Option<String> {
        self.notes
            .message()
            .or_else(|| aseprite_notice(&self.warnings))
    }
}

/// What to say about an Aseprite file: the one thing left out, or how many.
pub fn aseprite_notice(warnings: &[AseWarning]) -> Option<String> {
    match warnings {
        [] => None,
        [one] => Some(format!("Opened from Aseprite: {one}")),
        many => Some(format!(
            "Opened from Aseprite: {} things were left out or changed",
            many.len()
        )),
    }
}

/// Where Save may write without asking. Only a project that loaded completely qualifies, so
/// saving can never put project data into an image or lose what this version could not read.
pub fn save_path_for(path: PathBuf, notes: &piks::LoadNotes) -> Option<PathBuf> {
    (has_extension(&path, "piks") && notes.can_save_over()).then_some(path)
}

/// The file that holds something this version could not keep, and so must not be replaced.
pub fn protected_path(path: &Path, notes: &piks::LoadNotes) -> Option<PathBuf> {
    (has_extension(path, "piks") && !notes.can_save_over()).then(|| path.to_path_buf())
}

/// True if both name one file, whatever spelling or case was used to reach it.
pub fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

pub fn open(path: &Path) -> Result<Opened, FileError> {
    open_until(path, &|| false)
}

/// How much of a file is read between two questions whether to go on.
const READ_CHUNK: u64 = 1 << 20;

/// As [`read_limited`], asking `stop` before each chunk so a stopped read lets go of the file.
fn read_stoppable(path: &Path, limit: u64, stop: &dyn Fn() -> bool) -> Result<Vec<u8>, FileError> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len > limit {
        return Err(FormatError::FileTooLarge.into());
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len as usize)
        .map_err(|_| FormatError::FileTooLarge)?;
    let source = &file;
    // The limit also guards against a file that grows while it is being read.
    while (bytes.len() as u64) < limit {
        if stop() {
            return Err(FormatError::Stopped.into());
        }
        let want = READ_CHUNK.min(limit - bytes.len() as u64);
        if source.take(want).read_to_end(&mut bytes)? == 0 {
            break;
        }
    }
    Ok(bytes)
}

/// As [`open`], asking `stop` along the way. Once it answers true the read ends with
/// [`FormatError::Stopped`]: it is asked before the file is touched, between chunks of the
/// read, before each frame of an Aseprite file, each mebibyte of a picture being decoded (as
/// [`png::decode_until`] and [`import_ase_until`] ask), and once a file is decoded.
pub fn open_until(path: &Path, stop: &dyn Fn() -> bool) -> Result<Opened, FileError> {
    open_printed(path, stop, false)
}

/// As [`open_until`], also taking the print of a project's bytes when `print` is true.
pub fn open_printed(
    path: &Path,
    stop: &dyn Fn() -> bool,
    print: bool,
) -> Result<Opened, FileError> {
    let stopped = || -> Result<(), FileError> {
        match stop() {
            true => Err(FormatError::Stopped.into()),
            false => Ok(()),
        }
    };
    // A read that was stopped before it began never touches the file.
    stopped()?;
    // Before the read: a change made during it then shows as a change.
    let stamp = crate::recovery::file_stamp(path);
    let limit = if has_extension(path, "png") {
        MAX_PNG_BYTES
    } else {
        MAX_FILE_BYTES
    };
    let bytes = read_stoppable(path, limit, stop)?;
    stopped()?;
    let plain = |doc, notes| Opened {
        doc,
        notes,
        warnings: Vec::new(),
        print: None,
        stamp,
    };
    if has_extension(path, "png") {
        let image = png::decode_until(&bytes, stop)?;
        stopped()?;
        return Ok(plain(
            document_from_image(&image)?,
            piks::LoadNotes::default(),
        ));
    }
    if is_aseprite(path) {
        // Only ever read: it opens as a new project, so Save asks for a .piks name.
        let (doc, warnings) = import_ase_until(&bytes, stop)?;
        return Ok(Opened {
            warnings,
            ..plain(doc, piks::LoadNotes::default())
        });
    }
    let (doc, notes) = piks::load_noted(&bytes)?;
    stopped()?;
    let print = print.then(|| pikslar_rewind::hash128(&bytes));
    Ok(Opened {
        print,
        ..plain(doc, notes)
    })
}

/// Reads a recovery copy, with any floating pixels it holds.
pub fn open_recovery(path: &Path) -> Result<piks::Recovered, FileError> {
    Ok(piks::load_recovery(&read_limited(path, MAX_FILE_BYTES)?)?)
}

/// The bytes to save, refused unless they open again with nothing left out and would save
/// exactly the same, so a writer bug cannot reach the disk.
pub(crate) fn checked_bytes(doc: &Document) -> Result<Vec<u8>, FileError> {
    let bytes = piks::try_save(doc)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(FormatError::FileTooLarge.into());
    }
    let (reopened, notes) = piks::load_noted(&bytes).map_err(|_| FileError::Unverified)?;
    if !notes.can_save_over() || piks::save(&reopened) != bytes {
        return Err(FileError::Unverified);
    }
    Ok(bytes)
}

/// Returns the bytes written.
#[cfg(test)]
pub fn save(path: &Path, doc: &Document) -> Result<Vec<u8>, FileError> {
    save_traced(path, doc).map_err(|f| f.error)
}

/// A save that failed, and the step it failed at, for the diagnostics.
#[derive(Debug)]
pub struct SaveFailure {
    pub error: FileError,
    /// `check`, `encode`, `verify`, `read-only`, `create`, `write`, `flush`, `read-back`,
    /// `publish`, or for an upgrade `upgrade` and `upgrade-backup`.
    pub phase: &'static str,
}

impl From<FileError> for SaveFailure {
    fn from(error: FileError) -> Self {
        let phase = crate::diag::save_failure(&error).0;
        SaveFailure { error, phase }
    }
}

/// The step a write failed at, from the last one it passed.
pub(crate) fn failed_step(passed: Option<SaveStep>) -> &'static str {
    match passed {
        None => "create",
        Some(SaveStep::Created | SaveStep::HalfWritten) => "write",
        Some(SaveStep::Written) => "flush",
        Some(SaveStep::Synced) => "read-back",
        Some(SaveStep::Verified) => "publish",
    }
}

/// As [`save_checked`] with nothing expected of the file, for tests.
#[cfg(test)]
pub fn save_traced(path: &Path, doc: &Document) -> Result<Vec<u8>, SaveFailure> {
    save_checked(path, doc, None).map(|(bytes, _)| bytes)
}

/// Saves, saying which step failed, and puts the file in place the system's way
/// (`system_publish.rs`), refusing to replace one that no longer looks as `expect` says
/// (`outside.rs`). A read-only target is refused before any step. Gives the bytes written and
/// how they were put in place.
pub(crate) fn save_checked(
    path: &Path,
    doc: &Document,
    expect: Option<crate::outside::Seen>,
) -> Result<(Vec<u8>, Placed), SaveFailure> {
    let bytes = checked_bytes(doc)?;
    let mut passed = None;
    let mut step = |step, _: &Path| {
        passed = Some(step);
        Ok(())
    };
    let publish = &mut crate::system_publish::SystemPublish { expect };
    let written = write_atomic_with(path, &bytes, &mut step, publish);
    match written {
        Ok(placed) => Ok((bytes, placed)),
        Err(e) => {
            let error = FileError::from(e);
            let phase = match crate::diag::save_failure(&error).0 {
                "read-only" => "read-only",
                _ => failed_step(passed),
            };
            Err(SaveFailure { error, phase })
        }
    }
}

/// Saves over a project of an older `version`, keeping the original beside it, and puts the
/// new one in place as any save does: the system's way, refusing a file that no longer looks
/// as `expect` says (`outside.rs`), checked first so no version backup is made of it.
pub fn save_upgrading(
    path: &Path,
    version: u32,
    doc: &Document,
    expect: Option<crate::outside::Seen>,
) -> Result<(Vec<u8>, Placed), SaveFailure> {
    let upgrade_failed = |e| SaveFailure::from(FileError::Upgrade(e));
    if expect.is_some_and(|seen| crate::outside::still_same(path, &seen).is_none()) {
        return Err(upgrade_failed(UpgradeError::Changed));
    }
    let bytes = checked_bytes(doc)?;
    let publish = &mut crate::system_publish::SystemPublish { expect };
    let placed = upgrade::upgrade_with(path, version, &bytes, &mut |_| Ok(()), publish)
        .map_err(upgrade_failed)?;
    Ok((bytes, placed))
}

/// What to ask before saving over a project an older version made.
pub fn upgrade_question(path: &Path, version: u32, opened_here: bool, in_the_way: bool) -> String {
    let name = upgrade::file_name(path);
    let backup = upgrade::file_name(&upgrade::backup_path(path, version));
    let older = format!("{name} was saved by an older version of Pikslar.");
    let replaces = if opened_here {
        String::new()
    } else {
        " Saving here replaces that project.".to_string()
    };
    if in_the_way {
        return format!(
            "{older}{replaces} {backup} already exists and holds something else, so the \
             original has nowhere to be kept.\n\nSave a copy under a new name instead?"
        );
    }
    format!(
        "{older}{replaces} Saving it in the new format means older versions cannot open it. \
         The original is kept as {backup}.\n\nYes saves in the new format. No saves a copy \
         under a new name instead."
    )
}

/// What writing a palette file did.
pub struct PaletteWritten {
    pub count: usize,
    /// Swatches with some transparency, written as their colour alone.
    pub partly_clear: usize,
}

/// Reads a palette file (the format is taken from its first line). Nothing is changed.
pub fn read_palette(path: &Path) -> Result<PaletteRead, FileError> {
    let bytes = read_limited(path, palette_file::MAX_FILE_BYTES as u64)?;
    Ok(palette_file::read_any(&bytes)?)
}

/// Writes the swatches, in the order shown, as the format the file's extension names (GIMP
/// when it names none). The file is replaced whole or not at all.
pub fn write_palette(path: &Path, table: &ColorTable) -> Result<PaletteWritten, FileError> {
    let exported = export_swatches(table);
    let format = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(PaletteFormat::from_extension)
        .unwrap_or(PaletteFormat::Gpl);
    let bytes = palette_file::write(&exported.palette, format)?;
    write_atomic(path, &bytes)?;
    Ok(PaletteWritten {
        count: exported.palette.colors.len(),
        partly_clear: exported.partly_clear,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::DrawCmd;
    use pikslar_core::Editor;
    use pikslar_core::{rgba, BlendMode, Tool};
    use pikslar_render::{flatten, Compositor};

    const RED: u32 = rgba(255, 0, 0, 255);

    fn image(px: Vec<u32>) -> Image {
        Image {
            width: px.len(),
            height: 1,
            px,
        }
    }

    #[test]
    fn the_upgrade_question_names_the_backup_and_what_older_versions_lose() {
        let path = Path::new("art/hero.piks");
        let asked = upgrade_question(path, 2, true, false);
        assert!(asked.starts_with("hero.piks was saved by an older version"));
        assert!(asked.contains("older versions cannot open it"));
        assert!(asked.contains("kept as hero.piks.v2.bak"));
        assert!(!asked.contains("replaces"));
        let elsewhere = upgrade_question(path, 1, false, false);
        assert!(elsewhere.contains("replaces that project") && elsewhere.contains(".v1.bak"));
        let blocked = upgrade_question(path, 2, true, true);
        assert!(blocked.contains("already exists") && blocked.ends_with("instead?"));
    }

    fn is_stopped(result: Result<Opened, FileError>) -> bool {
        matches!(result, Err(FileError::Format(FormatError::Stopped)))
    }

    #[test]
    fn a_read_stopped_before_it_begins_never_touches_the_file() {
        let missing = std::env::temp_dir().join("pikslar-never-read.piks");
        assert!(is_stopped(open_until(&missing, &|| true)));
        assert!(
            matches!(open(&missing), Err(FileError::Format(FormatError::Io(_)))),
            "a read that goes on does look for it"
        );
    }

    #[test]
    fn a_stop_in_the_middle_of_reading_a_project_ends_the_read() {
        let dir = std::env::temp_dir().join(format!("pikslar-chunks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("big.piks");
        std::fs::write(&path, vec![0u8; 3 * READ_CHUNK as usize]).expect("written");
        let asked = std::cell::Cell::new(0);
        let stop = || {
            asked.set(asked.get() + 1);
            asked.get() >= 3
        };
        // Asked before the file, before the first chunk, and refused at the second.
        assert!(is_stopped(open_until(&path, &stop)));
        assert_eq!(asked.get(), 3);
        // Left alone, the same bytes are read through and only then found not to be a project.
        assert!(matches!(
            open_until(&path, &|| false),
            Err(FileError::Format(_))
        ));
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn a_stop_while_a_picture_is_decoded_ends_the_read() {
        let dir = std::env::temp_dir().join(format!("pikslar-png-stop-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("big.png");
        let px = (0..1024 * 1024u32).map(|i| 0xff00_0000 | i).collect();
        let picture = Image {
            width: 1024,
            height: 1024,
            px,
        };
        std::fs::write(&path, png::encode(&picture).expect("encodes")).expect("written");
        let asked = std::cell::Cell::new(0);
        let count = || {
            asked.set(asked.get() + 1);
            false
        };
        assert!(open_until(&path, &count).is_ok());
        let all = asked.get();
        // The file itself takes a question before it, one per chunk and one after: the rest
        // are asked while the picture is decoded.
        assert!(all > 8, "asked {all} times");
        let made = std::cell::Cell::new(0);
        let stop = || {
            made.set(made.get() + 1);
            made.get() > all - 3
        };
        assert!(is_stopped(open_until(&path, &stop)));
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn a_save_that_would_not_reopen_identically_is_refused() {
        let doc = painted(ColorMode::Palette, RED).doc().clone();
        let bytes = checked_bytes(&doc).expect("a sound project");
        assert_eq!(piks::save(&piks::load(&bytes).expect("opens")), bytes);
    }

    #[test]
    fn a_project_with_tilesets_is_saved_and_opens_with_them() {
        let mut doc = painted(ColorMode::Palette, RED).doc().clone();
        doc.add_tileset("Walls", ColorMode::Palette, (8, 8))
            .expect("room");
        let dir = std::env::temp_dir().join(format!("pikslar-tilesets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("tiles.piks");
        checked_bytes(&doc).expect("a sound project");
        save(&path, &doc).expect("saved");
        let back = piks::load(&std::fs::read(&path).expect("written")).expect("opens");
        assert_eq!(*back.tilesets, *doc.tilesets);
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn small_palettes_import_as_palette_art_with_swatches() {
        let doc = document_from_image(&image(vec![RED, 0, RED])).expect("valid");
        assert_eq!(doc.layers()[0].mode, ColorMode::Palette);
        assert_eq!(doc.colors.swatches().len(), 1);
        assert_eq!(doc.colors.color(doc.layers()[0].cel(0).get(2, 0)), RED);
    }

    #[test]
    fn many_colors_import_as_rgba() {
        let px: Vec<u32> = (0..300u32).map(|i| 0xff00_0000 | i).collect();
        let doc = document_from_image(&image(px.clone())).expect("valid");
        assert_eq!(doc.layers()[0].mode, ColorMode::Rgba);
        assert_eq!(doc.layers()[0].cel(0).to_cells(doc.width, doc.height), px);
    }

    fn painted(mode: ColorMode, color: u32) -> Editor {
        let mut e = Editor::new(Document::new(4, 1, mode).expect("valid"));
        e.color = color;
        e.apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x: 1,
            y: 0,
        })
        .expect("begin");
        e.apply(DrawCmd::Commit).expect("commit");
        e
    }

    #[test]
    fn single_layer_export_keeps_exact_translucent_colors() {
        let translucent = rgba(201, 57, 13, 3);
        for mode in [ColorMode::Palette, ColorMode::Rgba] {
            let mut e = painted(mode, translucent);
            let px = flatten(&mut e, 0, &mut Compositor::new(4, 1));
            assert_eq!(px[1], translucent, "{mode:?}");
        }
    }

    #[test]
    fn saved_projects_and_exported_images_open_again() {
        let dir = std::env::temp_dir().join(format!("pikslar-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let (project, image) = (dir.join("a.piks"), dir.join("a.PNG"));
        let e = painted(ColorMode::Palette, RED);
        save(&project, e.doc()).expect("save");
        pikslar_export::export_frame(&image, e.doc(), e.frame).expect("export");
        let reopened = open(&project).expect("open project").doc;
        assert_eq!(
            reopened.colors.color(reopened.layers()[0].cel(0).get(1, 0)),
            RED
        );
        let imported = open(&image).expect("open image").doc;
        assert_eq!(
            imported.colors.color(imported.layers()[0].cel(0).get(1, 0)),
            RED
        );
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn stacked_layers_are_merged_on_export() {
        let mut e = painted(ColorMode::Rgba, RED);
        e.apply(pikslar_core::LayerCmd::Add {
            at: 1,
            mode: ColorMode::Palette,
        })
        .expect("room");
        e.layer = 1;
        e.apply(pikslar_core::LayerCmd::SetBlend {
            layer: e.layer_id(1).expect("layer"),
            blend: BlendMode::Multiply,
        })
        .expect("set");
        e.color = rgba(0, 255, 255, 255);
        e.apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x: 1,
            y: 0,
        })
        .expect("begin");
        e.apply(DrawCmd::Commit).expect("commit");
        let px = flatten(&mut e, 0, &mut Compositor::new(4, 1));
        assert_eq!(px[1], rgba(0, 0, 0, 255));
    }

    #[test]
    fn export_ignores_the_stroke_cursor_position() {
        let mut e = painted(ColorMode::Rgba, RED);
        e.apply(pikslar_core::FrameCmd::Add { at: 1, copy: false })
            .expect("room");
        e.frame = 1;
        let px = flatten(&mut e, 0, &mut Compositor::new(4, 1));
        assert_eq!(px[1], RED);
        assert_eq!(e.frame, 1);
    }
}
