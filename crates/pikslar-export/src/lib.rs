//! Export: which pixels leave the program, laid out how, and written where.
//!
//! Every pixel comes from the canvas's own composition, a band
//! of rows at a time, so no export holds a whole image. Nothing here opens a window or touches the
//! GPU, so the command line program can use all of it.

mod apng;
mod apng_plan;
pub mod atlas;
mod frames;
mod gif;
mod gif_plan;
mod job;
pub mod json;
mod outputs;
mod palette;
mod picture;
mod sequence;
mod sheet;
mod sheet_layout;
mod sheet_slices;
mod stream;
pub mod targets;
pub mod tmj;
pub mod tmj_json;
mod tmj_tree;

#[cfg(test)]
#[path = "../../pikslar-format/src/apng_read.rs"]
mod apng_read;
#[cfg(test)]
mod apng_tests;
#[cfg(test)]
mod gif_area_tests;
#[cfg(test)]
#[path = "../../pikslar-format/src/gif_read.rs"]
mod gif_read;
#[cfg(test)]
mod gif_tests;
#[cfg(test)]
mod golden_tests;
#[cfg(test)]
pub(crate) mod indexed_tests;
#[cfg(test)]
mod links_tests;
#[cfg(test)]
mod picture_tests;
#[cfg(test)]
mod rate_tests;
#[cfg(test)]
mod repeat_tests;
#[cfg(test)]
mod sheet_options_tests;
#[cfg(test)]
mod sheet_slice_tests;
#[cfg(test)]
mod tmj_tests;
#[cfg(test)]
mod tween_tests;

pub use apng::{apng_output, MOST_PLAYS};
pub use frames::{animation_order, play_order, resolve, Frames};
pub use gif::{gif_delays, gif_delays_for, gif_output, gif_palette, GifPalette, SHORTEST_DELAY};
pub use job::{plan, Job, Kind, Planned, SHORTEST_MS};
pub use outputs::{
    frame_output, sequence_outputs, sheet_outputs, write_outputs, write_outputs_locked, Output,
};
pub use palette::{palette_of, Palette};
pub use picture::{check_size, layers_named, Layers, Picture};
pub use sequence::{member_name, sequence_plan, stale_members, SequencePlan};
pub use sheet::{
    direction_name, export_sheet, may_write_description, names_for, sheet_files, sheet_plan,
    SheetFiles, SheetPlan,
};
pub use sheet_layout::{arrange, JsonShape, Layout, Sheet, SheetOptions};
pub use stream::{write_frame_png, write_sheet_png};
pub use tmj::{atlas_path, tmj_map, tmj_outputs, TmjPlanned};

use pikslar_core::{Document, LimitError, MAX_DIMENSION};
use pikslar_format::atomic_stream::{write_set_streamed, Streamed};
use pikslar_format::FormatError;
use std::path::Path;
use std::{fmt, io};

/// Why an export did not happen.
#[derive(Debug)]
pub enum ExportError {
    /// The file could not be written, or its bytes could not be made.
    Format(FormatError),
    /// What was asked for is larger than an image may be.
    Limit(LimitError),
    /// A frame was asked for that the document does not have.
    NoSuchFrame { frame: usize, frames: usize },
    /// No tag has this name.
    NoSuchTag(String),
    /// A range that ends before it starts.
    Backwards { from: usize, to: usize },
    /// No layer has this name.
    NoSuchLayer(String),
    /// More than one layer has this name, so it does not say which.
    SameName { name: String, layers: usize },
    /// A layer was asked for by a position the document does not have.
    NoLayerAt { layer: usize, count: usize },
    /// A choice of layers with none in it.
    NoLayers,
    /// A scale of zero.
    NoScale,
    /// At this scale a frame would be larger than an image may be. The sides are as asked.
    TooLarge {
        scale: usize,
        width: u128,
        height: u128,
    },
    /// At this scale each frame fits, but the frames do not fit in one sheet.
    SheetTooLarge { scale: usize },
    /// The frames do not fit in one sheet with the columns and padding asked for, at `scale`.
    LayoutTooLarge { scale: usize },
    /// A PNG holds one frame, and this many were chosen.
    OneFrame { count: usize },
    /// An indexed image would need more colours than a palette holds: this frame's, or, for
    /// none, those of a sheet's frames together. `hidden` of the colours found before the
    /// search stopped are clear pixels with a colour other than black stored behind them: a
    /// lower bound, since the search stops at the 257th colour.
    TooManyColours { frame: Option<usize>, hidden: usize },
    /// A pixel of `frame` at `x`, `y` (document pixels, from 0 at the top left) is neither
    /// opaque nor clear, which a GIF cannot show.
    Translucent {
        frame: usize,
        x: usize,
        y: usize,
        alpha: u8,
    },
    /// The frames of a GIF have more colours together than its one palette holds; `clear` if
    /// some pixels are clear, which takes a place.
    GifColours { clear: bool },
    /// More plays than an animation is given.
    TooManyPlays { plays: u32 },
    /// No frames to show.
    NoFrames,
    /// A TMJ was asked for and the project has no tilemap layer.
    NoTilemaps,
    /// Two tilemap layers, by name, whose tiles differ in size: a Tiled map has one tile size.
    TileSizes {
        first: (String, (usize, usize)),
        other: (String, (usize, usize)),
    },
    /// A tileset's atlas would be larger than an exported image may be.
    AtlasTooLarge {
        tileset: String,
        width: usize,
        height: usize,
    },
    /// The map would have more cells, over all its tilemap layers, than
    /// [`tmj::MAX_TMJ_CELLS`].
    MapTooLarge { cells: usize },
}

fn tmj_message(f: &mut fmt::Formatter<'_>, e: &ExportError) -> fmt::Result {
    match e {
        ExportError::NoTilemaps => {
            f.write_str("This project has no tilemap layers, so there is no map to export as TMJ")
        }
        ExportError::TileSizes {
            first: (a, (aw, ah)),
            other: (b, (bw, bh)),
        } => write!(
            f,
            "A Tiled map has one tile size, and the tilemap layers differ: \"{a}\" has tiles of \
             {aw} by {ah} and \"{b}\" of {bw} by {bh}. Give every tilemap layer tiles of one \
             size, or remove the others from a copy of the project"
        ),
        ExportError::AtlasTooLarge {
            tileset,
            width,
            height,
        } => write!(
            f,
            "The tileset \"{tileset}\" would make an atlas of {width} by {height}. An exported \
             image may be at most {MAX_DIMENSION} a side"
        ),
        ExportError::MapTooLarge { cells } => write!(
            f,
            "This map would have {cells} cells over its tilemap layers, and a TMJ export holds \
             at most 4,194,304 (2,048 by 2,048 on one layer). Use larger tiles, or export fewer \
             tilemap layers from a copy of the project"
        ),
        other => write!(f, "{other:?}"),
    }
}

fn gif_message(f: &mut fmt::Formatter<'_>, e: &ExportError) -> fmt::Result {
    match e {
        ExportError::Translucent { frame, x, y, alpha } => write!(
            f,
            "Frame {} has a partly clear pixel at x {x}, y {y} (counted from 0 at the top \
             left), of alpha {alpha} out of 255. A GIF pixel is opaque or clear, and no pixel \
             is changed. Make such pixels opaque or clear, or export PNG",
            frame + 1
        ),
        ExportError::GifColours { clear: true } => f.write_str(
            "These frames have more than 255 colours together as well as clear pixels, which \
             take one of a GIF's 256 palette places. A GIF has one palette for all its frames, \
             and no colour is changed. Choose fewer frames, or export a PNG sequence",
        ),
        ExportError::GifColours { clear: false } => f.write_str(
            "These frames have more than 256 colours together. A GIF has one palette for all \
             its frames, and no colour is changed. Choose fewer frames, or export a PNG \
             sequence",
        ),
        ExportError::TooManyPlays { plays } => write!(
            f,
            "An animation can play at most 65536 times, not {plays}. --loop 0 plays it forever"
        ),
        ExportError::NoFrames => f.write_str("No frames were chosen"),
        other => write!(f, "{other:?}"),
    }
}

fn too_many_colours(
    f: &mut fmt::Formatter<'_>,
    frame: Option<usize>,
    hidden: usize,
) -> fmt::Result {
    match frame {
        Some(frame) => write!(
            f,
            "Frame {} has more than 256 colours, so it cannot be an indexed PNG without changing \
             some, and no colour is changed. Export it without --indexed",
            frame + 1
        )?,
        None => f.write_str(
            "These frames together have more than 256 colours, so one sheet of them cannot be an \
             indexed PNG without changing some, and no colour is changed. Choose fewer frames, \
             --format sequence, or no --indexed",
        )?,
    }
    if hidden > 0 {
        write!(
            f,
            ". At least {hidden} of them are clear pixels with a colour stored behind them, \
             which an indexed PNG keeps exactly as the RGBA one does"
        )?;
    }
    Ok(())
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Format(e) => e.fmt(f),
            ExportError::Limit(e) => e.fmt(f),
            ExportError::NoSuchFrame { frame, frames } => write!(
                f,
                "There is no frame {}: the project has {frames}",
                frame + 1
            ),
            ExportError::NoSuchTag(name) => write!(f, "There is no tag named \"{name}\""),
            ExportError::Backwards { from, to } => {
                write!(f, "Frames {} to {} end before they start", from + 1, to + 1)
            }
            ExportError::NoSuchLayer(name) => write!(f, "There is no layer named \"{name}\""),
            ExportError::SameName { name, layers } => write!(
                f,
                "{layers} layers are named \"{name}\". Give each its own name to choose one"
            ),
            ExportError::NoLayerAt { layer, count } => write!(
                f,
                "There is no layer {}: the project has {count}",
                layer + 1
            ),
            ExportError::NoLayers => f.write_str("No layers were chosen"),
            ExportError::SheetTooLarge { scale } => write!(
                f,
                "At {scale}x these frames do not fit in one sheet of at most {MAX_DIMENSION} a \
                 side. Choose fewer frames, a smaller scale, or --format sequence"
            ),
            ExportError::LayoutTooLarge { scale } => {
                let smaller = if *scale > 1 { ", a smaller scale" } else { "" };
                write!(
                    f,
                    "With these columns and padding the frames do not fit in one sheet of at \
                     most {MAX_DIMENSION} a side. Choose fewer columns, less padding{smaller} \
                     or fewer frames"
                )
            }
            ExportError::OneFrame { count } => {
                write!(f, "A PNG holds one frame, and {count} were chosen")
            }
            ExportError::NoScale => f.write_str("A scale must be 1 or more"),
            ExportError::TooManyColours { frame, hidden } => too_many_colours(f, *frame, *hidden),
            ExportError::Translucent { .. }
            | ExportError::GifColours { .. }
            | ExportError::TooManyPlays { .. }
            | ExportError::NoFrames => gif_message(f, self),
            ExportError::NoTilemaps
            | ExportError::TileSizes { .. }
            | ExportError::AtlasTooLarge { .. }
            | ExportError::MapTooLarge { .. } => tmj_message(f, self),
            ExportError::TooLarge {
                scale,
                width,
                height,
            } => write!(
                f,
                "At {scale}x a frame would be {width} by {height}. An exported image may be at \
                 most {MAX_DIMENSION} a side"
            ),
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExportError::Format(e) => Some(e),
            ExportError::Limit(_)
            | ExportError::NoSuchFrame { .. }
            | ExportError::NoSuchTag(_)
            | ExportError::Backwards { .. }
            | ExportError::NoSuchLayer(_)
            | ExportError::SameName { .. }
            | ExportError::NoLayerAt { .. }
            | ExportError::NoLayers
            | ExportError::NoScale
            | ExportError::TooLarge { .. }
            | ExportError::SheetTooLarge { .. }
            | ExportError::LayoutTooLarge { .. }
            | ExportError::OneFrame { .. }
            | ExportError::TooManyColours { .. }
            | ExportError::Translucent { .. }
            | ExportError::GifColours { .. }
            | ExportError::TooManyPlays { .. }
            | ExportError::NoFrames
            | ExportError::NoTilemaps
            | ExportError::TileSizes { .. }
            | ExportError::AtlasTooLarge { .. }
            | ExportError::MapTooLarge { .. } => None,
        }
    }
}

impl From<FormatError> for ExportError {
    fn from(e: FormatError) -> Self {
        ExportError::Format(e)
    }
}

impl From<LimitError> for ExportError {
    fn from(e: LimitError) -> Self {
        ExportError::Limit(e)
    }
}

impl From<io::Error> for ExportError {
    fn from(e: io::Error) -> Self {
        ExportError::Format(FormatError::Io(e))
    }
}

/// An export error as the writer of a file sees it. A file error keeps its kind.
pub(crate) fn io_error(e: ExportError) -> io::Error {
    match e {
        ExportError::Format(FormatError::Io(e)) => e,
        other => io::Error::other(other.to_string()),
    }
}

/// Writes one file from `write`, beside its target first, then in its place.
pub(crate) fn write_streamed(
    path: &Path,
    write: &mut dyn FnMut(&mut dyn io::Write) -> io::Result<()>,
) -> Result<(), FormatError> {
    let mut files = [Streamed {
        path,
        write: Box::new(write),
    }];
    write_set_streamed(&mut files, &[], &mut |_, _| Ok(())).map_err(|e| e.cause)
}

/// One frame as PNG bytes. Nothing in the document changes.
pub fn frame_png(doc: &Document, frame: usize) -> Result<Vec<u8>, ExportError> {
    let mut bytes = Vec::new();
    write_frame_png(&Picture::visible(doc), frame, None, &mut bytes)?;
    Ok(bytes)
}

/// Writes one frame as a PNG, without holding the image.
#[inline(never)]
pub fn export_frame(path: &Path, doc: &Document, frame: usize) -> Result<(), ExportError> {
    let frames = doc.frames.len();
    if frame >= frames {
        return Err(ExportError::NoSuchFrame { frame, frames });
    }
    let picture = Picture::visible(doc);
    write_streamed(path, &mut |out| {
        write_frame_png(&picture, frame, None, out).map_err(io_error)
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::ColorMode;

    #[test]
    fn a_sheet_too_large_once_scaled_says_what_to_change() {
        assert_eq!(
            ExportError::SheetTooLarge { scale: 3 }.to_string(),
            "At 3x these frames do not fit in one sheet of at most 8192 a side. Choose fewer \
             frames, a smaller scale, or --format sequence"
        );
    }

    #[test]
    fn a_frame_past_the_end_is_an_error_and_writes_nothing() {
        let doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
        let dir = std::env::temp_dir().join(format!("pikslar-frame-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("f.png");
        let result = export_frame(&path, &doc, 1);
        assert!(matches!(
            result,
            Err(ExportError::NoSuchFrame {
                frame: 1,
                frames: 1
            })
        ));
        assert!(!path.exists());
        export_frame(&path, &doc, 0).expect("the last frame exports");
        assert!(matches!(
            frame_png(&doc, 1),
            Err(ExportError::NoSuchFrame { frame: 1, .. })
        ));
        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}
