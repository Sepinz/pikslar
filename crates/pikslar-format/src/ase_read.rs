//! A read-only importer for Aseprite `.ase` and `.aseprite` files.
//!
//! The reader follows the public file specification, `docs/ase-file-specs.md` in the Aseprite
//! repository, and nothing else: no program's code was read to write it. It brings in the
//! header, frames and their durations, image layers and groups, cels (raw, zlib-packed and
//! linked), tilesets and tilemap layers with their cels, RGBA, grayscale and indexed color, both
//! kinds of palette, and tags. Whatever else a file
//! holds is left out and named in a warning. All input is untrusted: every size is checked
//! against the bytes there are before anything is allocated for it.

use crate::ase_cel::Budget;
use crate::ase_chunks::{Bytes, Depth, Kind, Sprite};
use crate::ase_names::Counted;
pub use crate::ase_names::Tally;
use crate::FormatError;
use pikslar_core::layer::{Layer, LayerKind};
use pikslar_core::tileset::TilesetId;
use pikslar_core::{
    BlendMode, Cel, ColorMode, ColorTable, Document, FrameInfo, LimitError, MAX_DIMENSION,
    MAX_DURATION_MS, MAX_FRAMES, MIN_DURATION_MS,
};
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

const HEADER_BYTES: usize = 128;
const FILE_MAGIC: u16 = 0xA5E0;
const FRAME_MAGIC: u16 = 0xF1FA;
const FRAME_HEAD_BYTES: usize = 16;
const CHUNK_HEAD_BYTES: usize = 6;
/// A frame that gives no length and a file that gives no speed show for this long.
const DEFAULT_MS: u32 = 100;
/// The most pixel bytes all the cels of one file may unpack: the same time for the same bytes.
const WORK_BUDGET: u64 = 1 << 30;
/// The most memory the drawings of one file may hold, and the cel being read with them. The
/// project's bar for a worst-case image (`pikslar-safety`'s `BOMB_PEAK_LIMIT`) is 700 MiB; this
/// leaves what the file itself, the tables and the allocator need under it.
const HELD_BUDGET: u64 = 512 << 20;

fn budget() -> Budget {
    Budget {
        work: WORK_BUDGET,
        held: HELD_BUDGET,
    }
}
const MAX_WARNINGS: usize = 256;
/// Places of the [`MAX_WARNINGS`] that only warnings about lost art may take.
const RESERVED: usize = 64;
const SWATCHES: usize = 256;

/// Something in a file that was left out or changed to fit, for the artist to be told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AseWarning {
    /// A kind of data that is not brought in.
    Skipped(&'static str),
    /// Pixels are not square.
    PixelRatio(u8, u8),
    /// A frame length was outside what a frame may be, and was moved into it.
    FrameDuration,
    /// A blend mode Pikslar does not have, shown as the closest one it has.
    LayerBlend {
        layer: String,
        mode: u16,
        shown_as: BlendMode,
    },
    /// A group's own opacity or blend mode, which a group here does not have.
    GroupSettings(String),
    /// A reference layer, brought in as an ordinary layer.
    ReferenceLayer(String),
    /// A cel's opacity, which a cel here does not have.
    CelOpacity(String),
    /// A cel's z-index, which a cel here does not have.
    CelZIndex(String),
    /// A linked cel whose frame has no drawing to link with, left empty.
    UnresolvedLink(String),
    /// The palette is changed in a later frame; the last change wins for the whole sprite.
    PaletteChanges,
    /// A cel whose pixels failed their checksum (or lost it), left empty.
    Damaged { layer: String, frame: usize },
    /// An indexed pixel names a color the palette does not have.
    IndexPastPalette,
    /// An indexed file with no palette at all.
    NoPalette,
    /// A tag outside the frames, or past the most tags, left out.
    TagDropped(String),
    /// A ping-pong reverse tag, which is played as a ping-pong.
    TagDirection(String),
    /// A tag set to play more times than Pikslar plays one, shortened to the most.
    TagRepeat(String),
    /// Many things of one kind told as one: how many, the first of them, and a frame number
    /// (from 1) for the kinds that name one.
    Counted {
        kind: Tally,
        count: usize,
        first: String,
        frame: usize,
    },
    /// A tilemap layer whose tileset the file does not hold (or keeps in another file), left out.
    TilemapLeftOut(String),
    /// A tileset whose tile pictures could not be unpacked; its tiles are blank.
    TilesetDamaged(String),
    /// Cells that name a tile the tileset does not have, left empty.
    UnknownTile(String),
    /// A diagonal flip on tiles that are not square, which cannot be kept; the flips are. Aseprite
    /// draws such a cell as the turned top-left square of the tile and leaves the rest clear, so
    /// these cells look different here.
    DiagonalOnOblong(String),
    /// There were more warnings than are kept.
    TooMany,
}

impl fmt::Display for AseWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.fmt_basic(f) {
            Some(done) => done,
            None => self.fmt_more(f).unwrap_or(Ok(())),
        }
    }
}

impl AseWarning {
    /// The words of the first kinds of warning; `None` for those [`AseWarning::fmt_more`] words.
    fn fmt_basic(&self, f: &mut fmt::Formatter<'_>) -> Option<fmt::Result> {
        Some(match self {
            AseWarning::Skipped(what) => write!(f, "Left out: {what}"),
            AseWarning::PixelRatio(w, h) => write!(f, "Pixels are {w}:{h}, shown as square"),
            AseWarning::FrameDuration => write!(
                f,
                "Some frame times were outside {MIN_DURATION_MS} to {MAX_DURATION_MS} ms and were set to the nearest of those"
            ),
            AseWarning::LayerBlend {
                layer,
                mode,
                shown_as,
            } => write!(
                f,
                "Layer \"{layer}\": blend mode {mode} is shown as {}",
                shown_as.name()
            ),
            AseWarning::GroupSettings(g) => {
                write!(f, "Group \"{g}\": its opacity and blend mode are not kept")
            }
            AseWarning::CelOpacity(l) => write!(f, "Layer \"{l}\": cel opacity is not kept"),
            AseWarning::CelZIndex(l) => write!(f, "Layer \"{l}\": cel z-index is not kept"),
            AseWarning::UnresolvedLink(l) => {
                write!(f, "Layer \"{l}\": a linked cel has nothing to link with")
            }
            AseWarning::PaletteChanges => f.write_str("The palette changes; the last one is kept"),
            AseWarning::Damaged { layer, frame } => write!(
                f,
                "Layer \"{layer}\", frame {}: the drawing is damaged and was left empty",
                frame + 1
            ),
            AseWarning::IndexPastPalette => {
                f.write_str("Some pixels use a color the palette does not have")
            }
            AseWarning::NoPalette => f.write_str("The file has no palette"),
            AseWarning::TagDropped(_)
            | AseWarning::TagDirection(_)
            | AseWarning::TagRepeat(_)
            | AseWarning::Counted { .. }
            | AseWarning::TilemapLeftOut(_)
            | AseWarning::TilesetDamaged(_)
            | AseWarning::UnknownTile(_)
            | AseWarning::DiagonalOnOblong(_)
            | AseWarning::ReferenceLayer(_) => return None,
            AseWarning::TooMany => f.write_str("There are more warnings than are shown"),
        })
    }
}

impl AseWarning {
    /// The words of the warnings about tags, counted names, reference layers and tilemaps.
    fn fmt_more(&self, f: &mut fmt::Formatter<'_>) -> Option<fmt::Result> {
        Some(match self {
            AseWarning::ReferenceLayer(l) => {
                write!(f, "Layer \"{l}\" is a reference layer, brought in as a layer")
            }
            AseWarning::TagDropped(t) => write!(f, "Tag \"{t}\" was left out"),
            AseWarning::TagDirection(t) => write!(f, "Tag \"{t}\" plays as a ping-pong"),
            AseWarning::TagRepeat(t) => write!(f, "Tag \"{t}\" plays fewer times than it asks"),
            AseWarning::Counted {
                kind,
                count,
                first,
                frame,
            } => f.write_str(&kind.say(*count, first, *frame)),
            AseWarning::TilemapLeftOut(l) => write!(
                f,
                "Layer \"{l}\": its tileset is not in the file, so the layer was left out"
            ),
            AseWarning::TilesetDamaged(t) => write!(
                f,
                "Tileset \"{t}\": the tiles are damaged and were left blank"
            ),
            AseWarning::UnknownTile(l) => write!(
                f,
                "Layer \"{l}\": some cells name a tile the tileset does not have and were left empty"
            ),
            AseWarning::DiagonalOnOblong(l) => write!(
                f,
                "Layer \"{l}\": tiles that are not square cannot be turned on the diagonal; those turns were left out, so those cells look different from Aseprite"
            ),
            AseWarning::Skipped(_)
            | AseWarning::PixelRatio(..)
            | AseWarning::FrameDuration
            | AseWarning::LayerBlend { .. }
            | AseWarning::GroupSettings(_)
            | AseWarning::CelOpacity(_)
            | AseWarning::CelZIndex(_)
            | AseWarning::UnresolvedLink(_)
            | AseWarning::PaletteChanges
            | AseWarning::Damaged { .. }
            | AseWarning::IndexPastPalette
            | AseWarning::NoPalette
            | AseWarning::TooMany => return None,
        })
    }
}

/// The warnings of one read, each told once. Notes about names, slices and the like fill no more
/// than [`MAX_WARNINGS`] less [`RESERVED`] places, so that a warning about lost art
/// ([`AseWarning::Damaged`], [`AseWarning::UnresolvedLink`]) always has a place; many things of one
/// kind are counted and told as one. What was told is remembered in sets, so that a file that
/// repeats a warning millions of times costs a lookup each and not a scan of the others.
#[derive(Default)]
pub(crate) struct Warnings {
    list: Vec<AseWarning>,
    /// Kept as `String`, the type of `told`: one kind of set for both.
    left_out: HashSet<String>,
    told: HashSet<String>,
    counted: Vec<Counted>,
    too_many: bool,
}

impl Warnings {
    pub fn add(&mut self, warning: AseWarning) {
        let new = match &warning {
            AseWarning::Skipped(what) => !self.left_out.contains(*what),
            other => !self.told.contains(&format!("{other:?}")),
        };
        if !new {
            return;
        }
        let art = matches!(
            warning,
            AseWarning::Damaged { .. }
                | AseWarning::UnresolvedLink(_)
                | AseWarning::TilemapLeftOut(_)
                | AseWarning::TilesetDamaged(_)
        );
        let room = if art {
            MAX_WARNINGS
        } else {
            MAX_WARNINGS - RESERVED
        };
        if self.list.len() >= room {
            self.too_many = true;
            return;
        }
        match &warning {
            AseWarning::Skipped(what) => self.left_out.insert(what.to_string()),
            other => self.told.insert(format!("{other:?}")),
        };
        self.list.push(warning);
    }

    /// Counts one more of `kind`; the first one counted is the one the warning names.
    pub fn tally(&mut self, kind: Tally, name: &str, frame: usize) {
        match self.counted.iter_mut().find(|c| c.kind == kind) {
            Some(c) => c.count += 1,
            None => self.counted.push(Counted {
                kind,
                count: 1,
                first: name.to_owned(),
                frame,
            }),
        }
    }

    pub fn into_vec(self) -> Vec<AseWarning> {
        let mut all = self.list;
        all.extend(self.counted.into_iter().map(|c| AseWarning::Counted {
            kind: c.kind,
            count: c.count,
            first: c.first,
            frame: c.frame,
        }));
        if self.too_many {
            all.push(AseWarning::TooMany);
        }
        all
    }
}

#[cfg(test)]
#[path = "ase_opacity_tests.rs"]
mod opacity_tests;

#[cfg(test)]
#[path = "ase_slice_tests.rs"]
mod slice_tests;

#[cfg(test)]
#[path = "ase_fix_tests.rs"]
mod fix_tests;

#[cfg(test)]
#[path = "ase_roundtrip_tests.rs"]
mod roundtrip_tests;

/// Reads an Aseprite file into a document, with a warning for each thing left out.
///
/// Nothing is written and nothing is kept of `bytes`. A file that is damaged, or past what a
/// document may hold (8192 pixels across, 4096 frames, 256 layers), is refused.
pub fn import_ase(bytes: &[u8]) -> Result<(Document, Vec<AseWarning>), FormatError> {
    import_ase_until(bytes, &|| false)
}

/// As [`import_ase`], asking `stop` before each frame, every 1,024 chunks within one, every
/// mebibyte of a picture being unpacked ([`crate::inflate::STOP_EVERY_BYTES`]), after each
/// picture is unpacked, before the frames are put together, and every mebibyte of pixels looked
/// at for the blending the document needs (each tileset and image once). Once it answers true
/// the read ends with [`FormatError::Stopped`], and what was read so far is let go. A stop
/// waits for no more than a mebibyte of pixels, or one picture made from pixels already
/// unpacked.
pub fn import_ase_until(
    bytes: &[u8],
    stop: &dyn Fn() -> bool,
) -> Result<(Document, Vec<AseWarning>), FormatError> {
    let mut sprite = read_header(bytes, stop)?;
    let mut rest = bytes.get(HEADER_BYTES..).unwrap_or_default();
    for frame in 0..sprite.frames {
        if stop() {
            return Err(FormatError::Stopped);
        }
        rest = read_frame(&mut sprite, (frame, stop), rest)?;
    }
    if stop() {
        return Err(FormatError::Stopped);
    }
    let doc = assemble(&mut sprite)?;
    Ok((doc, std::mem::take(&mut sprite.warnings).into_vec()))
}

fn read_header<'s>(bytes: &[u8], stop: &'s dyn Fn() -> bool) -> Result<Sprite<'s>, FormatError> {
    let mut r = Bytes(bytes);
    if bytes.len() < HEADER_BYTES {
        return Err(FormatError::Corrupt(
            "the file is too short to be an Aseprite file",
        ));
    }
    r.skip(4)?;
    if r.u16()? != FILE_MAGIC {
        return Err(FormatError::Corrupt("this is not an Aseprite file"));
    }
    let frames = usize::from(r.u16()?);
    let (width, height) = (usize::from(r.u16()?), usize::from(r.u16()?));
    let depth = match r.u16()? {
        32 => Depth::Rgba,
        16 => Depth::Gray,
        8 => Depth::Indexed,
        _ => return Err(FormatError::Corrupt("an unknown color depth")),
    };
    let flags = r.u32()?;
    let speed = u32::from(r.u16()?);
    r.skip(8)?;
    let transparent = r.u8()?;
    r.skip(3 + 2)?;
    let ratio = (r.u8()?, r.u8()?);
    check_size(width, height, frames, bytes.len())?;
    let mut sprite = Sprite::new(width, height, depth, frames, budget());
    sprite.stop = stop;
    (sprite.flags, sprite.speed, sprite.transparent) = (flags, speed, transparent);
    if ratio.0 != ratio.1 && ratio.0 != 0 && ratio.1 != 0 {
        sprite
            .warnings
            .add(AseWarning::PixelRatio(ratio.0, ratio.1));
    }
    Ok(sprite)
}

/// Refuses sizes a document cannot hold, and more frames than the file has room for.
fn check_size(width: usize, height: usize, frames: usize, len: usize) -> Result<(), FormatError> {
    if width == 0 || height == 0 || frames == 0 {
        return Err(FormatError::Corrupt("the canvas or the frame count is 0"));
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(FormatError::TooLarge);
    }
    if frames > MAX_FRAMES {
        return Err(FormatError::Unsupported("more than 4096 frames"));
    }
    if frames > (len - HEADER_BYTES) / FRAME_HEAD_BYTES {
        return Err(FormatError::Corrupt(
            "the file has fewer frames than it says",
        ));
    }
    Ok(())
}

/// How many chunks of a frame are read between asking whether to stop.
const STOP_EVERY: u32 = 1024;

/// Reads the frame at the start of `input` and returns what follows it.
fn read_frame<'a>(
    sprite: &mut Sprite<'_>,
    (frame, stop): (usize, &dyn Fn() -> bool),
    input: &'a [u8],
) -> Result<&'a [u8], FormatError> {
    let mut head = Bytes(input);
    let size = head.u32()? as usize;
    if head.u16()? != FRAME_MAGIC {
        return Err(FormatError::Corrupt(
            "a frame does not start where it should",
        ));
    }
    let (old, duration) = (head.u16()?, head.u16()?);
    head.skip(2)?;
    let new = head.u32()?;
    if size < FRAME_HEAD_BYTES {
        return Err(FormatError::Corrupt(
            "a frame is smaller than its own header",
        ));
    }
    let (body, rest) = input
        .split_at_checked(size)
        .ok_or(FormatError::Corrupt("the file ends inside a frame"))?;
    note_duration(sprite, duration);
    let count = if new != 0 { new } else { u32::from(old) };
    let mut chunks = Bytes(body.get(FRAME_HEAD_BYTES..).unwrap_or_default());
    for n in 0..count {
        if n % STOP_EVERY == STOP_EVERY - 1 && stop() {
            return Err(FormatError::Stopped);
        }
        let size = chunks.u32()? as usize;
        let kind = chunks.u16()?;
        let data = size
            .checked_sub(CHUNK_HEAD_BYTES)
            .ok_or(FormatError::Corrupt(
                "a chunk is smaller than its own header",
            ))?;
        sprite.chunk(frame, kind, chunks.take(data)?)?;
    }
    Ok(rest)
}

fn note_duration(sprite: &mut Sprite<'_>, duration: u16) {
    let ms = match (duration, sprite.speed) {
        (0, 0) => DEFAULT_MS,
        (0, speed) => speed,
        (ms, _) => u32::from(ms),
    };
    let kept = ms.clamp(MIN_DURATION_MS, MAX_DURATION_MS);
    if kept != ms {
        sprite.warnings.add(AseWarning::FrameDuration);
    }
    sprite.durations.push(kept);
}

impl Sprite<'_> {
    /// Reads one chunk of a frame, by its type.
    fn chunk(&mut self, frame: usize, kind: u16, data: &[u8]) -> Result<(), FormatError> {
        match kind {
            0x0004 => self.old_palette_chunk(frame, data, false),
            0x0011 => self.old_palette_chunk(frame, data, true),
            0x2004 => self.layer_chunk(data),
            0x2005 => self.cel_chunk(frame, data),
            0x2018 => self.tags_chunk(data),
            0x2019 => self.palette_chunk(frame, data),
            0x2022 => {
                self.slice_chunk(data);
                Ok(())
            }
            0x2023 => self.tileset_chunk(data),
            other => {
                self.warnings.add(AseWarning::Skipped(skipped(other)));
                Ok(())
            }
        }
    }
}

/// What a chunk type that is not read is called.
fn skipped(kind: u16) -> &'static str {
    match kind {
        0x2006 => "cel extra data",
        0x2007 => "the color profile",
        0x2008 => "external files",
        0x2016 => "masks",
        0x2017 => "paths",
        0x2020 => "user data",
        _ => "chunks of an unknown type",
    }
}

/// The file's layers as Pikslar lists them, from the bottom. A group comes after what it
/// holds, where the file puts it before; skipped layers are left out.
fn emit_order(layers: &[crate::ase_chunks::AseLayer]) -> Vec<usize> {
    let mut order = Vec::with_capacity(layers.len());
    let mut open: Vec<usize> = Vec::new();
    for (index, layer) in layers.iter().enumerate() {
        while open.len() > usize::from(layer.level) {
            order.extend(open.pop());
        }
        match layer.kind {
            Kind::Group => open.push(index),
            Kind::Image | Kind::Tilemap => order.push(index),
            Kind::Skipped => {}
        }
    }
    order.extend(open.into_iter().rev());
    order
}

fn limit(error: LimitError) -> FormatError {
    match error {
        LimitError::Dimensions => FormatError::TooLarge,
        _ => FormatError::Corrupt("the file does not fit a document"),
    }
}

fn assemble(sprite: &mut Sprite<'_>) -> Result<Document, FormatError> {
    let tilesets = sprite.settle_tilemap_layers();
    let order = emit_order(&sprite.layers);
    if order.is_empty() && !sprite.layers.is_empty() {
        // Only tilemap layers, or others that are left out: a valid file, not a damaged one.
        return Err(FormatError::Unsupported(
            "a file whose layers are all tilemaps or of other kinds that are not brought in",
        ));
    }
    if order.is_empty() {
        return Err(FormatError::Corrupt("the file has no layers to bring in"));
    }
    sprite.check_palette();
    let mode = match sprite.depth {
        Depth::Indexed => ColorMode::Palette,
        _ => ColorMode::Rgba,
    };
    let mut slots: Vec<_> = std::mem::take(&mut sprite.layers)
        .into_iter()
        .map(Some)
        .collect();
    let mut layers = Vec::with_capacity(order.len());
    let mut holds = Vec::with_capacity(order.len());
    for index in order {
        let layer = slots
            .get_mut(index)
            .and_then(Option::take)
            .ok_or(FormatError::Corrupt("a layer is listed twice"))?;
        holds.push(layer.holds.clone());
        layers.push(pikslar_layer(layer, mode, sprite.frames, &tilesets));
    }
    let frames = sprite
        .durations
        .iter()
        .map(|&duration_ms| FrameInfo::new(duration_ms))
        .collect();
    let mut doc = Document::from_parts(
        sprite.width,
        sprite.height,
        Arc::new(table(sprite)),
        layers,
        frames,
    )
    .map_err(limit)?;
    for (index, ids) in holds
        .iter()
        .enumerate()
        .filter(|(_, h)| h.iter().any(|&id| id != 0))
    {
        doc.set_holds(index, ids).map_err(limit)?;
    }
    sprite.add_tilesets(&mut doc, mode, &tilesets)?;
    doc.check_groups()
        .map_err(|_| FormatError::Corrupt("layer groups are out of order"))?;
    doc.check_tilemaps()
        .map_err(|_| FormatError::Corrupt("a tilemap does not match its tileset"))?;
    doc.tags = std::mem::take(&mut sprite.tags);
    for slice in std::mem::take(&mut sprite.slices) {
        doc.add_slice(slice).map_err(limit)?;
    }
    doc.blending = crate::ase_blending::blending_for(&doc, sprite.stop)?;
    Ok(doc)
}

fn pikslar_layer(
    from: crate::ase_chunks::AseLayer,
    mode: ColorMode,
    frames: usize,
    tilesets: &[(u32, TilesetId)],
) -> Layer {
    let group = from.kind == Kind::Group;
    let mut layer = Layer::new(from.name, if group { ColorMode::Rgba } else { mode });
    layer.visible = from.visible;
    layer.locked = from.locked;
    layer.folded = from.folded;
    layer.depth = from.level;
    layer.opacity = from.opacity;
    layer.blend = from.blend;
    if group {
        layer.kind = LayerKind::Group;
    }
    if let Some(&(_, id)) = tilesets.iter().find(|(file, _)| *file == from.tileset) {
        if from.kind == Kind::Tilemap {
            layer.kind = LayerKind::Tilemap(id);
        }
    }
    let blank = Cel::blank();
    let mut cels: Vec<Cel> = from
        .cels
        .into_iter()
        .map(|cel| cel.unwrap_or_else(|| blank.clone()))
        .collect();
    cels.resize(frames, blank);
    layer.with_cels(cels)
}

/// The color table: clear, then the palette, with at least 256 places when pixels are indexed
/// so that every index a pixel holds has a place.
fn table(sprite: &Sprite<'_>) -> ColorTable {
    let mut palette = sprite.final_palette();
    if sprite.depth == Depth::Indexed && palette.len() < 256 {
        palette.resize(256, 0);
    }
    let mut colors = vec![0];
    colors.extend(palette);
    let swatches: Vec<u32> = (1..colors.len().min(SWATCHES + 1))
        .filter(|&id| colors.get(id).is_some_and(|&c| c != 0))
        .map(|id| id as u32)
        .collect();
    ColorTable::from_parts(&colors, &swatches)
}
