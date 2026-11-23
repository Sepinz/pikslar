//! The `.piks` project format.
//!
//! Little-endian. Header, color table, frames, layers, then each distinct image once,
//! tile by tile in the same packed form used in memory, then tagged sections, and finally
//! a CRC-32 of everything before it. Frames that share an image stay shared after loading.
//!
//! Version 3, in `piks_v3`, is what is written. Versions 1 and 2 are read here, and always will
//! be. Version 1 files have no sections. Readers skip sections they do not know.

pub use crate::piks_sections::LoadNotes;
use crate::piks_sections::{keep_foreign, MAX_SECTIONS};
use crate::{crc32, piks_float, FormatError};
use pikslar_core::document::{MAX_FRAMES, MAX_LAYERS};
use pikslar_core::tile::{Tile, TILE};
use pikslar_core::{
    BlendMode, Cel, ColorMode, ColorTable, Document, FloatingSnapshot, FrameInfo, Grid, Layer,
    MAX_DIMENSION, MAX_DURATION_MS, MIN_DURATION_MS, TILE_AREA,
};
use std::sync::Arc;

/// The most a project of any version may make the reader hold, counted as it is read. The
/// largest drawing the canvas can hold, 8192 by 8192 with every storage tile at its most costly
/// (256 colours: 1,336 bytes with its table slot), is [`LARGEST_DRAWING`], about 334 MiB, so
/// this holds six of them; pixel art is mostly flat colour and packs far smaller. Saving checks
/// what it wrote by reading it back under this limit, beside the document and the file, so a
/// project near it needs about three times it to save: 6 GiB here, where the 4 GiB before this
/// needed 12. See `docs/format-v3.md`, Reading.
pub(crate) const MAX_DECODED_BYTES: usize = 2 << 30;

/// What one drawing of the largest canvas holds at most: see [`MAX_DECODED_BYTES`].
pub(crate) const LARGEST_DRAWING: usize = {
    let tiles = (pikslar_core::MAX_DIMENSION / TILE) * (pikslar_core::MAX_DIMENSION / TILE);
    // A packed tile of 8 bits: its overhead, 256 colours, one byte a cell, and its table slot.
    tiles * (48 + 256 * 4 + TILE_AREA + 8)
};

const _: () = assert!(6 * LARGEST_DRAWING <= MAX_DECODED_BYTES);

/// What each image record is charged beyond its table and tiles: its own image (an `Arc<Image>`,
/// 48 bytes) and what the reader holds for it while the file is read (its place in the lists
/// that hold it before it is a cel). Measured by `tests/project_resident.rs`.
pub(crate) const IMAGE_RECORD_CHARGE: usize = 96;
/// What each cel is charged, before any is read: the cel the layer keeps (24 bytes) and what
/// the reader holds for it while the file is read (its record, and its place in the lists of
/// images, holds and links that the checks build). Measured by `tests/project_resident.rs`.
/// Public for the safety checks that claim the most cels.
#[doc(hidden)]
pub const CEL_CHARGE: usize = 56;

pub(crate) const MAGIC: &[u8; 4] = b"PIKS";
/// The version `save` writes. A project older than this is upgraded only when the person agrees.
pub const WRITTEN_VERSION: u32 = crate::piks_v3::VERSION;
/// The newest version this build reads.
const NEWEST: u32 = crate::piks_v3::VERSION;
const SECTIONS_SINCE: u32 = 2;
/// Floating pixels. Only recovery copies have this.
pub(crate) const FLOATING: &[u8; 4] = b"FLOT";
pub(crate) const TRUNCATED: FormatError = FormatError::Corrupt("the file ends too early");
const BAD_TILE: FormatError = FormatError::Corrupt("bad tile data");

/// Layer names are cut to this when read, so every name fits the record that saves it.
pub(crate) const MAX_NAME_BYTES: usize = 4096;

/// A name no longer than [`MAX_NAME_BYTES`], cut where a character ends.
pub(crate) fn cut_name(name: &str) -> &str {
    let mut end = name.len().min(MAX_NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// A layer name from a file, with control, direction and zero-width characters turned into
/// spaces. One that is cut or is not text is noted, so Save asks for a new name and the
/// original keeps it. Only one that is not text is damage.
pub(crate) fn read_name(bytes: &[u8], notes: &mut LoadNotes) -> String {
    let text = String::from_utf8_lossy(bytes);
    let kept = cut_name(&text);
    let is_text = std::str::from_utf8(bytes).is_ok();
    if !is_text && !notes.damaged.contains(&"layer names") {
        notes.damaged.push("layer names");
    } else if is_text && kept.len() != bytes.len() {
        notes.names_shortened = true;
    }
    // A name never carries a newline or a mark that changes how it shows.
    pikslar_core::names::spaced(kept)
}

pub(crate) struct Out(pub(crate) Vec<u8>);

impl Out {
    pub(crate) fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub(crate) fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn u32s(&mut self, values: &[u32]) {
        self.u32(values.len() as u32);
        values.iter().for_each(|&v| self.u32(v));
    }
}

pub(crate) fn write_tile(out: &mut Out, tile: Option<&Tile>) {
    match tile {
        None => out.u8(0),
        Some(Tile::Solid(v)) => {
            out.u8(1);
            out.u32(*v);
        }
        Some(Tile::Packed {
            palette,
            bits,
            data,
        }) => {
            out.u8(2);
            out.u16(palette.len() as u16);
            out.u8(*bits);
            palette.iter().for_each(|&c| out.u32(c));
            out.0.extend_from_slice(data);
        }
        Some(Tile::Raw(raw)) => {
            out.u8(3);
            raw.iter().for_each(|&c| out.u32(c));
        }
    }
}

/// Refuses a document the writer cannot keep whole: one whose tiles, stored each on its own,
/// pass what a reader opens ([`pikslar_core::tileset::MAX_TILE_BYTES`]). The editor refuses
/// every change that would get there, so this is the last guard, not the rule.
pub fn check_writable(doc: &Document) -> Result<(), FormatError> {
    match doc.stored_tile_bytes() > pikslar_core::tileset::MAX_TILE_BYTES {
        true => Err(FormatError::TilesTooLarge),
        false => Ok(()),
    }
}

/// Serializes a document, unless it holds something the writer cannot keep.
pub fn try_save(doc: &Document) -> Result<Vec<u8>, FormatError> {
    check_writable(doc)?;
    Ok(save(doc))
}

/// Serializes a recovery copy, unless the document holds something the writer cannot keep.
pub fn try_save_recovery(
    doc: &Document,
    floating: Option<&FloatingSnapshot>,
) -> Result<Vec<u8>, FormatError> {
    check_writable(doc)?;
    Ok(save_recovery(doc, floating))
}

/// Serializes a document.
pub fn save(doc: &Document) -> Vec<u8> {
    crate::piks_v3::encode(doc, &[])
}

/// What [`save`] writes, but for the `Nids` section of frame ids and id floors: for checks
/// that an undone or failed change left everything else byte for byte as it was. Ids it used
/// up stay used up, and that is saved.
#[doc(hidden)]
pub fn save_without_ids(doc: &Document) -> Vec<u8> {
    crate::piks_v3::encode_with(doc, &[], false)
}

/// Serializes a recovery copy: the document as it stands, with any floating pixels beside it.
pub fn save_recovery(doc: &Document, floating: Option<&FloatingSnapshot>) -> Vec<u8> {
    match floating {
        Some(snapshot) => crate::piks_v3::encode(doc, &[(FLOATING, piks_float::encode(snapshot))]),
        None => save(doc),
    }
}

pub(crate) struct In<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) pos: usize,
}

impl<'a> In<'a> {
    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        let end = self.pos.checked_add(n).ok_or(TRUNCATED)?;
        let slice = self.data.get(self.pos..end).ok_or(TRUNCATED)?;
        self.pos = end;
        Ok(slice)
    }
    pub(crate) fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.bytes(1)?[0])
    }
    /// A byte a record may end before, read as 0 when it does.
    pub(crate) fn optional_u8(&mut self) -> Result<u8, FormatError> {
        match self.pos < self.data.len() {
            true => self.u8(),
            false => Ok(0),
        }
    }
    pub(crate) fn u16(&mut self) -> Result<u16, FormatError> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub(crate) fn u32(&mut self) -> Result<u32, FormatError> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub(crate) fn i32(&mut self) -> Result<i32, FormatError> {
        Ok(self.u32()? as i32)
    }
    fn u32_values(&mut self, count: usize) -> Result<Vec<u32>, FormatError> {
        let raw = self.bytes(count.checked_mul(4).ok_or(TRUNCATED)?)?;
        Ok(raw
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect())
    }
    pub(crate) fn u32_list(&mut self, max: usize) -> Result<Vec<u32>, FormatError> {
        let count = self.u32()? as usize;
        if count > max {
            return Err(FormatError::Corrupt("a list is impossibly long"));
        }
        self.u32_values(count)
    }
}

pub(crate) fn read_tile(input: &mut In<'_>) -> Result<Option<Tile>, FormatError> {
    Ok(match input.u8()? {
        0 => None,
        1 => Some(Tile::Solid(input.u32()?)),
        2 => {
            let count = input.u16()? as usize;
            let bits = input.u8()?;
            if !matches!(bits, 1 | 2 | 4 | 8) || count == 0 || count > 1usize << bits {
                return Err(BAD_TILE);
            }
            let palette = input.u32_values(count)?.into_boxed_slice();
            let data = input.bytes((TILE_AREA * bits as usize).div_ceil(8))?.into();
            Some(Tile::Packed {
                palette,
                bits,
                data,
            })
        }
        3 => {
            let values = input.u32_values(TILE_AREA)?;
            let raw: Box<[u32; TILE_AREA]> =
                values.into_boxed_slice().try_into().map_err(|_| BAD_TILE)?;
            Some(Tile::Raw(raw))
        }
        _ => return Err(BAD_TILE),
    })
}

/// Counts the memory a project will need, and refuses before it is allocated. Running out
/// aborts the program, so a small hostile file must not be able to ask for too much.
pub(crate) fn charge(spent: &mut usize, bytes: usize) -> Result<(), FormatError> {
    *spent = spent.saturating_add(bytes);
    if *spent > MAX_DECODED_BYTES {
        return Err(FormatError::FileTooLarge);
    }
    Ok(())
}

fn read_grid(
    input: &mut In<'_>,
    width: usize,
    height: usize,
    spent: &mut usize,
) -> Result<Cel, FormatError> {
    let slots = width.div_ceil(TILE) * height.div_ceil(TILE);
    // Every tile takes at least one byte, so a shorter file cannot hold this picture.
    if input.data.len().saturating_sub(input.pos) < slots {
        return Err(FormatError::Corrupt("an image is cut short"));
    }
    charge(spent, IMAGE_RECORD_CHARGE)?;
    // At most 2 MiB, and gone again before the next image: only the trimmed table is kept.
    let mut grid = Grid::new(width, height);
    for index in 0..slots {
        let tile = read_tile(input)?;
        if let Some(tile) = &tile {
            charge(spent, tile.heap_size())?;
        }
        grid.replace(index, tile.map(Arc::new));
    }
    let cel = Cel::new(grid);
    charge(spent, cel.table_bytes())?;
    Ok(cel)
}

struct RawLayer {
    layer: Layer,
    cel_ids: Vec<u32>,
}

fn read_layer(
    input: &mut In<'_>,
    frames: usize,
    notes: &mut LoadNotes,
) -> Result<RawLayer, FormatError> {
    let name_len = input.u16()? as usize;
    let name = read_name(input.bytes(name_len)?, notes);
    let visible = input.u8()? != 0;
    let opacity = input.u8()?;
    let blend = if input.u8()? != 0 {
        BlendMode::Multiply
    } else {
        BlendMode::Normal
    };
    let mode = if input.u8()? != 0 {
        ColorMode::Rgba
    } else {
        ColorMode::Palette
    };
    let cel_ids = input.u32_values(frames)?;
    let mut layer = Layer::new(name, mode);
    layer.visible = visible;
    layer.opacity = opacity;
    layer.blend = blend;
    Ok(RawLayer { layer, cel_ids })
}

fn check_trailer(bytes: &[u8]) -> Result<&[u8], FormatError> {
    if bytes.len() < 8 || &bytes[..4] != MAGIC {
        return Err(FormatError::Corrupt("not a Pikslar project"));
    }
    let (body, tail) = bytes.split_at(bytes.len() - 4);
    if u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]) != crc32(&[body]) {
        return Err(FormatError::Corrupt("checksum mismatch"));
    }
    Ok(body)
}

pub(crate) type Section<'a> = (&'a [u8], &'a [u8]);

pub(crate) fn read_sections<'a>(input: &mut In<'a>) -> Result<Vec<Section<'a>>, FormatError> {
    let count = input.u32()? as usize;
    if count > MAX_SECTIONS {
        return Err(FormatError::Corrupt("bad section count"));
    }
    (0..count)
        .map(|_| {
            let tag = input.bytes(4)?;
            let len = input.u32()? as usize;
            Ok((tag, input.bytes(len)?))
        })
        .collect()
}

pub(crate) type Parsed<'a> = (Document, Vec<Section<'a>>, LoadNotes);

fn parse<'a>(bytes: &'a [u8], spent: &mut usize) -> Result<Parsed<'a>, FormatError> {
    let body = check_trailer(bytes)?;
    let mut input = In { data: body, pos: 4 };
    let version = input.u32()?;
    if version == 0 || version > NEWEST {
        return Err(FormatError::Unsupported("a newer project version"));
    }
    if version == crate::piks_v3::VERSION {
        return crate::piks_v3::parse(&mut input, spent);
    }
    let (width, height) = (input.u32()? as usize, input.u32()? as usize);
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(FormatError::TooLarge);
    }
    let colors = input.u32_list(65_536)?;
    let swatches = input.u32_list(65_536)?;
    let durations = input.u32_list(MAX_FRAMES)?;
    if durations.is_empty() {
        return Err(FormatError::Corrupt("no frames"));
    }
    let layer_count = input.u32()? as usize;
    if layer_count == 0 || layer_count > MAX_LAYERS {
        return Err(FormatError::Corrupt("bad layer count"));
    }
    let mut notes = LoadNotes::default();
    charge(
        spent,
        (layer_count * durations.len()).saturating_mul(CEL_CHARGE),
    )?;
    let raw_layers = (0..layer_count)
        .map(|_| read_layer(&mut input, durations.len(), &mut notes))
        .collect::<Result<Vec<_>, _>>()?;
    let grid_count = input.u32()? as usize;
    if grid_count > layer_count * durations.len() {
        return Err(FormatError::Corrupt("bad image count"));
    }
    let grids = (0..grid_count)
        .map(|_| read_grid(&mut input, width, height, spent))
        .collect::<Result<Vec<_>, _>>()?;
    let sections = if version >= SECTIONS_SINCE {
        read_sections(&mut input)?
    } else {
        Vec::new()
    };
    if input.pos != body.len() {
        return Err(FormatError::Corrupt("unexpected data after the project"));
    }
    let mut layers = Vec::with_capacity(layer_count);
    for RawLayer { layer, cel_ids } in raw_layers {
        let cels = cel_ids
            .iter()
            .map(|&id| {
                let found = grids.get(id as usize).cloned();
                found.ok_or(FormatError::Corrupt("bad image reference"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        layers.push(layer.with_cels(cels));
    }
    let frames = durations
        .into_iter()
        .map(|ms| FrameInfo::new(ms.clamp(MIN_DURATION_MS, MAX_DURATION_MS)))
        .collect();
    let colors = Arc::new(ColorTable::from_parts(&colors, &swatches));
    let mut doc = Document::from_parts(width, height, colors, layers, frames)
        .map_err(|_| FormatError::Corrupt("layers do not match the frames"))?;
    crate::piks_anim::apply(&mut doc, &sections, &mut notes);
    crate::piks_ids::apply_ids(&mut doc, &sections, &mut notes);
    crate::piks_doc_id::apply_doc_id(&mut doc, &sections);
    keep_foreign(&mut doc, &sections, &mut notes);
    Ok((doc, sections, notes))
}

/// Reads a document, validating every count and index.
pub fn load(bytes: &[u8]) -> Result<Document, FormatError> {
    Ok(parse(bytes, &mut 0)?.0)
}

/// Reads a document and says what could not be kept.
pub fn load_noted(bytes: &[u8]) -> Result<(Document, LoadNotes), FormatError> {
    let (doc, _, notes) = parse(bytes, &mut 0)?;
    Ok((doc, notes))
}

/// As [`load_noted`], refused once reading has charged more than `room`: for documents read
/// from somewhere other than a project file, which ask for less than a project may.
pub fn load_noted_within(bytes: &[u8], room: usize) -> Result<(Document, LoadNotes), FormatError> {
    let mut spent = MAX_DECODED_BYTES.saturating_sub(room);
    let (doc, _, notes) = parse(bytes, &mut spent)?;
    Ok((doc, notes))
}

/// What reading a project charges its decode limit, as [`load`] counts it: for tests that
/// hold the charge to what a read really allocates.
#[doc(hidden)]
pub fn charges(bytes: &[u8]) -> Result<usize, FormatError> {
    let mut spent = 0;
    parse(bytes, &mut spent)?;
    Ok(spent)
}

/// As [`load`], with only `room` of the decode limit left: for checks that a file opens with
/// exactly its [`charges`] left and is refused for its size with a byte less.
#[doc(hidden)]
pub fn load_with_room(bytes: &[u8], room: usize) -> Result<Document, FormatError> {
    let mut spent = MAX_DECODED_BYTES.saturating_sub(room);
    Ok(parse(bytes, &mut spent)?.0)
}

/// What a recovery copy held.
pub struct Recovered {
    pub doc: Document,
    pub floating: Option<FloatingSnapshot>,
    /// There were floating pixels that could not be read. The document is still good.
    pub floating_damaged: bool,
    pub notes: LoadNotes,
}

/// Reads a recovery copy. Bad floating pixels never cost the document under them.
pub fn load_recovery(bytes: &[u8]) -> Result<Recovered, FormatError> {
    recover(bytes, &mut 0)
}

/// [`load_recovery`] charging `spent` for the floating pixels too, after the document: past the
/// limit, they are given up as damaged and the document is kept.
fn recover(bytes: &[u8], spent: &mut usize) -> Result<Recovered, FormatError> {
    let (doc, sections, notes) = parse(bytes, spent)?;
    let found = sections.iter().find(|(tag, _)| *tag == FLOATING);
    let decoded = found.map(|(_, data)| piks_float::decode(data, spent));
    let floating_damaged = matches!(decoded, Some(Err(_)));
    Ok(Recovered {
        doc,
        floating: decoded.and_then(Result::ok),
        floating_damaged,
        notes,
    })
}

#[cfg(test)]
mod limit_tests;
#[cfg(test)]
mod offset_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) mod v2_writer;
