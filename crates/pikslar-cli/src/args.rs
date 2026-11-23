//! What was asked for on the command line. Parsing only: nothing here touches a file.

use crate::failure::Failure;
use pikslar_export::{Frames, JsonShape};
use std::ffi::OsString;
use std::path::PathBuf;

use pikslar_export::Kind;

/// The kinds of file `export` can make: the app's image kinds, and a Tiled map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Sheet,
    Sequence,
    Gif,
    Apng,
    /// A Tiled map of one frame's tilemap layers, with an atlas PNG for each tileset.
    Tmj,
}

impl Format {
    /// The image kind the export library makes, or none for a Tiled map.
    pub fn kind(self) -> Option<Kind> {
        Some(match self {
            Format::Png => Kind::Png,
            Format::Sheet => Kind::Sheet,
            Format::Sequence => Kind::Sequence,
            Format::Gif => Kind::Gif,
            Format::Apng => Kind::Apng,
            Format::Tmj => return None,
        })
    }

    /// The ending its file has.
    pub fn extension(self) -> &'static str {
        self.kind().map_or("tmj", Kind::extension)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ExportArgs {
    pub project: PathBuf,
    pub format: Format,
    pub out: Option<PathBuf>,
    pub frames: Option<Frames>,
    /// Names of the layers to draw. None given means the visible layers.
    pub layers: Vec<String>,
    pub scale: Option<usize>,
    /// Indexed PNGs, each with a palette of its own colours, in palette order when every colour
    /// it uses is a swatch (see `pikslar_export::Palette`).
    pub indexed: bool,
    /// How many times a GIF or APNG plays, 0 for forever. None given means forever.
    pub plays: Option<u32>,
    /// A sheet's frames a row. None given means a row for each tag.
    pub columns: Option<usize>,
    /// Clear pixels between a sheet's frames. None given means none.
    pub padding: Option<usize>,
    /// How a sheet's description lists its frames. None given means an array.
    pub json: Option<JsonShape>,
    pub overwrite: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Help(Option<String>),
    Version,
    Info {
        project: PathBuf,
        json: bool,
    },
    Export(ExportArgs),
    Palette(crate::palette_args::PaletteArgs),
    Batch(crate::script_args::BatchArgs),
    /// A JSON lines session that may open and save only inside `dir`.
    Session {
        dir: PathBuf,
    },
}

/// Reads the arguments after the program's name.
pub fn parse(args: Vec<OsString>) -> Result<Command, Failure> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Command::Help(None));
    };
    match text(first)?.as_str() {
        "help" | "--help" | "-h" => Ok(Command::Help(args.next().map(text).transpose()?)),
        "--version" | "-V" => Ok(Command::Version),
        "info" => info(args),
        "export" => export(args),
        "palette" => crate::palette_args::parse(args),
        "batch" => crate::script_args::batch(args),
        "session" => crate::script_args::session(args),
        other => Err(Failure::Usage(format!("Unknown command \"{other}\""))),
    }
}

pub(crate) fn text(arg: OsString) -> Result<String, Failure> {
    arg.into_string()
        .map_err(|_| Failure::Usage("An argument is not valid text".into()))
}

fn info(args: impl Iterator<Item = OsString>) -> Result<Command, Failure> {
    let (mut project, mut json) = (None, false);
    for arg in args {
        match arg.to_str() {
            Some("--json") => json = true,
            Some("--help" | "-h") => return Ok(Command::Help(Some("info".into()))),
            Some(flag) if flag.starts_with("--") => return Err(unknown(flag)),
            _ => set_once(&mut project, PathBuf::from(arg), "project")?,
        }
    }
    let project = project.ok_or_else(|| Failure::Usage("info needs a project file".into()))?;
    Ok(Command::Info { project, json })
}

fn export(mut args: impl Iterator<Item = OsString>) -> Result<Command, Failure> {
    let mut project = None;
    let mut kind = None;
    let mut out = ExportArgs {
        project: PathBuf::new(),
        format: Format::Png,
        out: None,
        frames: None,
        layers: Vec::new(),
        scale: None,
        indexed: false,
        plays: None,
        columns: None,
        padding: None,
        json: None,
        overwrite: false,
    };
    while let Some(arg) = args.next() {
        let Some(flag) = arg.to_str().filter(|a| a.starts_with('-')) else {
            set_once(&mut project, PathBuf::from(arg), "project")?;
            continue;
        };
        match flag {
            "--help" | "-h" => return Ok(Command::Help(Some("export".into()))),
            "--overwrite" => out.overwrite = true,
            "--indexed" => out.indexed = true,
            "--format" => set_once(&mut kind, format(&value(&mut args, flag)?)?, "--format")?,
            "--out" | "-o" => {
                let path = PathBuf::from(value_os(&mut args, flag)?);
                set_once(&mut out.out, path, "--out")?;
            }
            "--frame" => set_frames(&mut out.frames, Frames::One(number(&mut args, flag)?))?,
            "--frames" => set_frames(&mut out.frames, range(&value(&mut args, flag)?)?)?,
            "--tag" => set_frames(&mut out.frames, Frames::Tag(value(&mut args, flag)?))?,
            "--layer" => out.layers.push(value(&mut args, flag)?),
            "--scale" => set_once(&mut out.scale, scale(&value(&mut args, flag)?)?, "--scale")?,
            "--loop" => set_once(&mut out.plays, plays(&value(&mut args, flag)?)?, "--loop")?,
            "--columns" => set_once(&mut out.columns, columns(&value(&mut args, flag)?)?, flag)?,
            "--padding" => set_once(&mut out.padding, padding(&value(&mut args, flag)?)?, flag)?,
            "--json" => set_once(&mut out.json, shape(&value(&mut args, flag)?)?, flag)?,
            other => return Err(unknown(other)),
        }
    }
    out.project = project.ok_or_else(|| Failure::Usage("export needs a project file".into()))?;
    out.format = kind.unwrap_or(Format::Png);
    for_its_format(&out)?;
    Ok(Command::Export(out))
}

/// Options that only one kind of file has.
fn for_its_format(out: &ExportArgs) -> Result<(), Failure> {
    let animated = matches!(out.format, Format::Gif | Format::Apng);
    if out.plays.is_some() && !animated {
        return Err(Failure::Usage("--loop is for --format gif and apng".into()));
    }
    let laid_out = [
        (out.columns.is_some(), "--columns"),
        (out.padding.is_some(), "--padding"),
        (out.json.is_some(), "--json"),
    ];
    if let Some((_, flag)) = laid_out.iter().find(|(given, _)| *given) {
        if out.format != Format::Sheet {
            return Err(Failure::Usage(format!("{flag} is for --format sheet")));
        }
    }
    if out.format == Format::Tmj {
        return for_a_map(out);
    }
    match (out.indexed, out.format) {
        (true, Format::Gif) => Err(Failure::Usage(
            "A GIF is always indexed. --indexed is for png, sheet and sequence".into(),
        )),
        (true, Format::Apng) => Err(Failure::Usage(
            "An APNG keeps every colour and alpha as they are. --indexed is for png, sheet and \
             sequence"
                .into(),
        )),
        _ => Ok(()),
    }
}

/// A Tiled map is one frame of every tilemap layer at its own size.
fn for_a_map(out: &ExportArgs) -> Result<(), Failure> {
    if matches!(
        out.frames,
        Some(Frames::Range(..) | Frames::Tag(_) | Frames::All)
    ) {
        return Err(Failure::Usage(
            "A TMJ holds one frame: choose it with --frame".into(),
        ));
    }
    let image_only = [
        (!out.layers.is_empty(), "--layer"),
        (out.scale.is_some(), "--scale"),
        (out.indexed, "--indexed"),
    ];
    match image_only.iter().find(|(given, _)| *given) {
        Some((_, flag)) => Err(Failure::Usage(format!(
            "{flag} is not for --format tmj, which exports every tilemap layer, shown or \
             hidden, at its own size"
        ))),
        None => Ok(()),
    }
}

/// How many times a GIF plays: 0 for forever, or 1 to 65536.
fn plays(text: &str) -> Result<u32, Failure> {
    match text.trim().parse::<u32>() {
        Ok(n) if n <= pikslar_format::gif_write::MAX_PLAYS => Ok(n),
        _ => Err(Failure::Usage(format!(
            "\"{text}\" is not a number of plays. --loop takes 0 (forever) or 1 to \
             65536"
        ))),
    }
}

/// A sheet's frames a row: a whole number from 1. Whether the sheet fits is checked later.
fn columns(text: &str) -> Result<usize, Failure> {
    match text.trim().parse::<usize>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(Failure::Usage(format!(
            "\"{text}\" is not a number of columns. --columns takes a whole number from 1"
        ))),
    }
}

/// Clear pixels between a sheet's frames: a whole number from 0.
fn padding(text: &str) -> Result<usize, Failure> {
    text.trim().parse::<usize>().map_err(|_| {
        Failure::Usage(format!(
            "\"{text}\" is not a padding. --padding takes a whole number of pixels from 0"
        ))
    })
}

fn shape(name: &str) -> Result<JsonShape, Failure> {
    match name {
        "array" => Ok(JsonShape::Array),
        "hash" => Ok(JsonShape::Hash),
        other => Err(Failure::Usage(format!(
            "Unknown --json \"{other}\". A sheet's description is an array (the default) or a              hash"
        ))),
    }
}

pub(crate) fn unknown(flag: &str) -> Failure {
    Failure::Usage(format!("Unknown option \"{flag}\""))
}

pub(crate) fn set_once<T>(slot: &mut Option<T>, value: T, what: &str) -> Result<(), Failure> {
    if slot.replace(value).is_some() {
        return Err(Failure::Usage(format!("Only one {what} can be given")));
    }
    Ok(())
}

fn set_frames(slot: &mut Option<Frames>, frames: Frames) -> Result<(), Failure> {
    if slot.replace(frames).is_some() {
        return Err(Failure::Usage(
            "Choose frames once: --frame, --frames or --tag".into(),
        ));
    }
    Ok(())
}

pub(crate) fn value_os(
    args: &mut impl Iterator<Item = OsString>,
    flag: &str,
) -> Result<OsString, Failure> {
    args.next()
        .ok_or_else(|| Failure::Usage(format!("{flag} needs a value")))
}

pub(crate) fn value(
    args: &mut impl Iterator<Item = OsString>,
    flag: &str,
) -> Result<String, Failure> {
    text(value_os(args, flag)?)
}

fn format(name: &str) -> Result<Format, Failure> {
    match name {
        "png" => Ok(Format::Png),
        "sheet" => Ok(Format::Sheet),
        "sequence" => Ok(Format::Sequence),
        "gif" => Ok(Format::Gif),
        "apng" => Ok(Format::Apng),
        "tmj" => Ok(Format::Tmj),
        other => Err(Failure::Usage(format!(
            "Unknown format \"{other}\". Formats: png, sheet, sequence, gif, apng, tmj"
        ))),
    }
}

/// Frames are counted from 1 on the command line, as in the program, and from 0 inside.
fn frame_number(text: &str) -> Result<usize, Failure> {
    match text.trim().parse::<usize>() {
        Ok(n) if n >= 1 => Ok(n - 1),
        _ => Err(Failure::Usage(format!(
            "\"{text}\" is not a frame number. Frames are counted from 1"
        ))),
    }
}

/// A whole number from 1. How large it may be depends on the project, and is checked there.
fn scale(text: &str) -> Result<usize, Failure> {
    match text.trim().parse::<usize>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(Failure::Usage(format!(
            "\"{text}\" is not a scale. A scale is a whole number from 1, such as 2"
        ))),
    }
}

fn number(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<usize, Failure> {
    frame_number(&value(args, flag)?)
}

fn range(text: &str) -> Result<Frames, Failure> {
    let (from, to) = text.split_once('-').ok_or_else(|| {
        Failure::Usage(format!("\"{text}\" is not a range of frames, such as 2-5"))
    })?;
    Ok(Frames::Range(frame_number(from)?, frame_number(to)?))
}
