//! Version 3 of the `.piks` project format, laid out in `docs/format-v3.md`.
//!
//! Each distinct image is stored once as a tight table of tiles placed in its own coordinates,
//! and each cel names its image, its position and its hold. Records carry their length, so a
//! newer version can append fields that this one skips.

use crate::piks::{
    charge, read_sections, read_tile, write_tile, In, Out, Parsed, Section, MAGIC, TRUNCATED,
};
use crate::piks_sections::{keep_foreign, LoadNotes};
use crate::{crc32, FormatError};
use pikslar_core::document::{MAX_FRAMES, MAX_LAYERS};
use pikslar_core::grid::Cell;
use pikslar_core::reference::{MAX_REFERENCE_BYTES, MAX_REFERENCE_SIDE};
use pikslar_core::tileset::{TilesetId, Tilesets};
use pikslar_core::{
    BlendMode, Blending, Cel, ColorMode, ColorTable, Document, FrameInfo, Image, Layer, LayerKind,
    MAX_DIMENSION, MAX_DURATION_MS, MAX_TILE_REACH, MIN_DURATION_MS, TILE,
};
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) const VERSION: u32 = 3;
/// What a layer record holds before its name: id, visible, opacity, blend, mode, name length.
const LAYER_FIELDS: usize = 10;
/// Kind, image, x, y and link.
const CEL_FIELDS: usize = 13;
pub(crate) use crate::piks::MAX_NAME_BYTES;
/// Persisted layer ids above this are refused, so adding layers can never run out of numbers.
const MAX_PERSISTED_ID: u32 = pikslar_core::PersistedLayerId::MAX;
use crate::piks::{CEL_CHARGE, IMAGE_RECORD_CHARGE};
/// The flags byte appended after a layer's name, written only when one is set, so a file with
/// none is read by older versions as before. An older reader ignores it and loses only the lock.
const LOCKED: u8 = 1;
/// A group that shows only its own row.
const FOLDED: u8 = 2;
/// The kind byte after the flags, written only for a reference layer, a group, or a layer in
/// a group. A depth byte follows it: how many groups the layer is in.
const REFERENCE_KIND: u8 = 1;
const GROUP_KIND: u8 = 2;
/// A tilemap layer: its `u32` tileset id follows the depth byte, which is always written for it.
const TILEMAP_KIND: u8 = 3;
/// The header's required features: a reader that does not know a set bit refuses the file.
/// A reference layer needs it, since an older version would draw the reference into exports;
/// so does a group, since an older version would show what a hidden group hides.
const REFERENCES: u32 = 1;
const GROUPS: u32 = 2;
/// Set while the document has any tileset: an older version would show a map's cells as colours.
pub(crate) const TILEMAPS: u32 = 4;
/// Set while the document uses Aseprite-compatible blending: an older version would draw its
/// layers with Pikslar's blending and export them so, and saving over the file would lose it.
const ASEPRITE_BLENDING: u32 = 8;
const KNOWN_FEATURES: u32 = REFERENCES | GROUPS | TILEMAPS | ASEPRITE_BLENDING;
const PIXELS: u8 = 0;
/// An image record whose values are map cells, not colours.
pub(crate) const MAP: u8 = 1;
const OUT_OF_RANGE: FormatError = FormatError::Corrupt("a frame is placed past the offset range");

/// Names are cut when read, so one longer than a record can hold only comes from a bug.
fn write_name(out: &mut Out, name: &str) {
    let name = crate::piks::cut_name(name);
    out.u16(name.len() as u16);
    out.0.extend_from_slice(name.as_bytes());
}

fn write_layer(out: &mut Out, layer: &Layer) {
    let name_len = layer.name.len().min(MAX_NAME_BYTES);
    let mut record = Out(Vec::with_capacity(LAYER_FIELDS + name_len));
    record.u32(layer.persisted_id().get());
    record.u8(layer.visible as u8);
    record.u8(layer.opacity);
    record.u8(layer.blend.number());
    record.u8(matches!(layer.mode, ColorMode::Rgba) as u8);
    write_name(&mut record, &layer.name);
    let kind = match layer.kind {
        LayerKind::Paint => 0,
        LayerKind::Reference => REFERENCE_KIND,
        LayerKind::Group => GROUP_KIND,
        LayerKind::Tilemap(_) => TILEMAP_KIND,
    };
    let grouped = kind == GROUP_KIND || layer.depth > 0;
    let folded = kind == GROUP_KIND && layer.folded;
    if layer.locked || kind != 0 || grouped {
        let flags = if layer.locked { LOCKED } else { 0 } | if folded { FOLDED } else { 0 };
        record.u8(flags);
    }
    if kind != 0 || grouped {
        record.u8(kind);
    }
    if grouped || kind == TILEMAP_KIND {
        record.u8(layer.depth);
    }
    if let LayerKind::Tilemap(tileset) = layer.kind {
        record.u32(tileset.get());
    }
    out.u16(record.0.len() as u16);
    out.0.extend_from_slice(&record.0);
}

pub(crate) fn write_image(out: &mut Out, image: &Image, kind: u8) {
    let (x0, y0, tw, th) = image.table();
    out.u8(kind);
    out.u16(x0 as i16 as u16);
    out.u16(y0 as i16 as u16);
    out.u16(tw as u16);
    out.u16(th as u16);
    for ty in y0..y0 + th as i32 {
        for tx in x0..x0 + tw as i32 {
            write_tile(out, image.tile(tx, ty));
        }
    }
}

/// Every distinct image in order of first use with its record kind, and each cel's index into
/// that list. A drawing shown on paint layers and on map layers, or on map layers of two
/// tilesets, as one blank drawing of an added frame is, gets a record for each, since a map
/// record belongs to one tileset.
fn number_images(doc: &Document) -> (Vec<(&Image, u8)>, Vec<Vec<u32>>) {
    let mut seen: HashMap<(usize, Option<TilesetId>), u32> = HashMap::new();
    let mut images: Vec<(&Image, u8)> = Vec::new();
    let mut indices = Vec::with_capacity(doc.layers().len());
    for layer in doc.layers() {
        let mut row = Vec::with_capacity(layer.frame_count());
        for cel in layer.cels() {
            let next = images.len() as u32;
            let index = *seen
                .entry((cel.image_id(), layer.tileset()))
                .or_insert(next);
            if index == next {
                images.push((cel.image(), if layer.is_tilemap() { MAP } else { PIXELS }));
            }
            row.push(index);
        }
        indices.push(row);
    }
    (images, indices)
}

/// Each cel's link, numbered from 1 across the document. A cel that does not show its link's
/// first drawing is written in no link, keeping its own drawing, and a link left with one cel
/// is none.
fn number_links(doc: &Document) -> Vec<Vec<u32>> {
    // Each link's first cel, as a place in `owners`: a map of plain numbers, a kind the
    // program has anyway.
    let mut first: HashMap<u32, usize> = HashMap::new();
    let mut owners: Vec<&Cel> = Vec::new();
    let mut count: HashMap<u32, usize> = HashMap::new();
    let mut held: Vec<Vec<u32>> = Vec::with_capacity(doc.layers().len());
    for layer in doc.layers() {
        let row = layer.cels().iter().map(|cel| {
            let at = *first.entry(cel.link()).or_insert(owners.len());
            if at == owners.len() {
                owners.push(cel);
            }
            let owner = owners.get(at).copied().unwrap_or(cel);
            let id = match cel.link() != 0 && owner.same_image(cel) {
                true => cel.link(),
                false => 0,
            };
            *count.entry(id).or_default() += 1;
            id
        });
        held.push(row.collect());
    }
    // From the bottom layer up, each layer's links that no lower layer numbered come next, in
    // the order of their ids. Without links across layers, this is each layer's holds ranked
    // after the holds of the layers before, as earlier builds wrote them.
    let mut numbers: HashMap<u32, u32> = HashMap::new();
    for row in &held {
        let shared = |id: &u32| *id != 0 && count.get(id).is_some_and(|&n| n > 1);
        let mut fresh: Vec<u32> = row.iter().copied().filter(shared).collect();
        fresh.retain(|id| !numbers.contains_key(id));
        fresh.sort_unstable();
        fresh.dedup();
        for id in fresh {
            let next = numbers.len() as u32 + 1;
            numbers.insert(id, next);
        }
    }
    let number = |id: &u32| numbers.get(id).copied().unwrap_or(0);
    held.iter()
        .map(|row| row.iter().map(number).collect())
        .collect()
}

fn write_sections(out: &mut Out, doc: &Document, extra: &[(&[u8; 4], Vec<u8>)], write_ids: bool) {
    let tags = crate::piks_anim::tags_section(doc);
    let rate = crate::piks_anim::rate_section(doc);
    let groups = crate::piks_palette::groups_section(doc);
    let slices = crate::piks_slices::slices_section(doc);
    let tilesets = crate::piks_tilesets::tilesets_section(doc);
    let terrains = crate::piks_terrain::terrains_section(doc);
    let ids = match write_ids {
        true => crate::piks_ids::ids_section(doc),
        false => None,
    };
    let doc_id = match write_ids {
        true => crate::piks_doc_id::doc_id_section(doc),
        false => None,
    };
    let every: Vec<(&[u8; 4], &[u8])> = tags
        .iter()
        .map(|bytes| (crate::piks_anim::TAGS, &bytes[..]))
        .chain(
            rate.iter()
                .map(|bytes| (crate::piks_anim::RATE, &bytes[..])),
        )
        .chain(
            groups
                .iter()
                .map(|bytes| (crate::piks_palette::GROUPS, &bytes[..])),
        )
        .chain(
            tilesets
                .iter()
                .map(|bytes| (crate::piks_tilesets::TILESETS, &bytes[..])),
        )
        .chain(
            terrains
                .iter()
                .map(|bytes| (crate::piks_terrain::TERRAINS, &bytes[..])),
        )
        .chain(
            slices
                .iter()
                .map(|bytes| (crate::piks_slices::SLICES, &bytes[..])),
        )
        .chain(ids.iter().map(|bytes| (crate::piks_ids::IDS, &bytes[..])))
        .chain(
            doc_id
                .iter()
                .map(|bytes| (crate::piks_doc_id::DOC_ID, &bytes[..])),
        )
        .chain(extra.iter().map(|(tag, bytes)| (*tag, &bytes[..])))
        .chain(doc.foreign.iter().map(|f| (&f.tag, &f.data[..])))
        .collect();
    out.u32(every.len() as u32);
    for (tag, bytes) in every {
        out.0.extend_from_slice(tag);
        out.u32(bytes.len() as u32);
        out.0.extend_from_slice(bytes);
    }
}

/// Serializes a document as version 3, with `extra` sections after its tags.
pub(crate) fn encode(doc: &Document, extra: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    encode_with(doc, extra, true)
}

/// As [`encode`], leaving out the `Nids` section when `write_ids` is false, as a build from
/// before it writes a file: for tests of what such a build does with a carried one.
pub(crate) fn encode_with(
    doc: &Document,
    extra: &[(&[u8; 4], Vec<u8>)],
    write_ids: bool,
) -> Vec<u8> {
    let mut out = Out(Vec::with_capacity(4096));
    out.0.extend_from_slice(MAGIC);
    out.u32(VERSION);
    let has = |kind: LayerKind| doc.layers().iter().any(|l| l.kind == kind);
    let references = if has(LayerKind::Reference) {
        REFERENCES
    } else {
        0
    };
    let groups = if has(LayerKind::Group) { GROUPS } else { 0 };
    let tilemaps = if doc.tilesets.is_empty() { 0 } else { TILEMAPS };
    let blending = match doc.blending {
        Blending::Pikslar => 0,
        Blending::Aseprite => ASEPRITE_BLENDING,
    };
    out.u32(references | groups | tilemaps | blending);
    out.u32(doc.width as u32);
    out.u32(doc.height as u32);
    out.u32s(doc.colors.colors());
    out.u32s(doc.colors.swatches());
    let durations: Vec<u32> = doc.frames.iter().map(|f| f.duration_ms).collect();
    out.u32s(&durations);
    out.u32(doc.layers().len() as u32);
    doc.layers()
        .iter()
        .for_each(|layer| write_layer(&mut out, layer));
    let (images, indices) = number_images(doc);
    out.u32(images.len() as u32);
    for (image, kind) in &images {
        write_image(&mut out, image, *kind);
    }
    for ((layer, indices), links) in doc.layers().iter().zip(&indices).zip(number_links(doc)) {
        for ((cel, &index), link) in layer.cels().iter().zip(indices).zip(links) {
            let (x, y) = cel.position();
            out.u8(CEL_FIELDS as u8);
            out.u8(PIXELS);
            out.u32(index);
            out.u16(x as u16);
            out.u16(y as u16);
            out.u32(link);
        }
    }
    write_sections(&mut out, doc, extra, write_ids);
    let crc = crc32(&[&out.0]);
    out.u32(crc);
    out.0
}

struct RawLayer {
    layer: Layer,
    persisted_id: u32,
}

/// One length-prefixed record, and whether it held more than this version reads.
pub(crate) fn record<'a>(
    input: &mut In<'a>,
    len: usize,
    known: usize,
) -> Result<In<'a>, FormatError> {
    let data = input.bytes(len)?;
    if len < known {
        return Err(FormatError::Corrupt("a record is too short"));
    }
    Ok(In { data, pos: 0 })
}

fn read_layer(input: &mut In<'_>, notes: &mut LoadNotes) -> Result<RawLayer, FormatError> {
    let len = input.u16()? as usize;
    let mut r = record(input, len, LAYER_FIELDS)?;
    let persisted_id = r.u32()?;
    let visible = r.u8()?;
    let opacity = r.u8()?;
    let blend = r.u8()?;
    let mode = match r.u8()? {
        0 => ColorMode::Palette,
        1 => ColorMode::Rgba,
        _ => {
            return Err(FormatError::Unsupported(
                "a layer color mode from a newer Pikslar",
            ))
        }
    };
    let name_len = r.u16()? as usize;
    if name_len > len - LAYER_FIELDS {
        return Err(FormatError::Corrupt("a layer name runs past its record"));
    }
    let name = crate::piks::read_name(r.bytes(name_len)?, notes);
    let (flags, kind_byte, depth) = (r.optional_u8()?, r.optional_u8()?, r.optional_u8()?);
    let tileset = match kind_byte == TILEMAP_KIND {
        true => Some(r.u32()?),
        false => None,
    };
    notes.newer_fields |= r.pos != len
        || kind_byte > TILEMAP_KIND
        || visible > 1
        || BlendMode::from_number(blend).is_none()
        || flags & !(LOCKED | FOLDED) != 0;
    if persisted_id == 0 || persisted_id > MAX_PERSISTED_ID {
        return Err(FormatError::Corrupt("a layer has an impossible number"));
    }
    let mut layer = Layer::new(name, mode);
    layer.visible = visible != 0;
    layer.kind = match tileset {
        Some(id) => LayerKind::Tilemap(TilesetId::new(id)),
        None => layer_kind(kind_byte),
    };
    layer.locked = flags & LOCKED != 0;
    layer.depth = depth;
    layer.folded = layer.kind == LayerKind::Group && flags & FOLDED != 0;
    layer.opacity = opacity;
    layer.blend = BlendMode::from_number(blend).unwrap_or(BlendMode::Normal);
    Ok(RawLayer {
        layer,
        persisted_id,
    })
}

/// A kind this version does not know is read as paint and protects the file: a kind that must
/// not be, as a reference must not be exported, also sets a required feature.
fn layer_kind(byte: u8) -> LayerKind {
    match byte {
        REFERENCE_KIND => LayerKind::Reference,
        GROUP_KIND => LayerKind::Group,
        _ => LayerKind::Paint,
    }
}

/// A reference or a group needs its required feature, which is what keeps an older version
/// from opening the file: one without it was not written by Pikslar.
fn check_features(features: u32, layers: &[RawLayer]) -> Result<(), FormatError> {
    let has = |kind: LayerKind| layers.iter().any(|raw| raw.layer.kind == kind);
    if has(LayerKind::Reference) && features & REFERENCES == 0 {
        return Err(FormatError::Corrupt(
            "a reference layer without its feature",
        ));
    }
    if has(LayerKind::Group) && features & GROUPS == 0 {
        return Err(FormatError::Corrupt("a layer group without its feature"));
    }
    if layers.iter().any(|raw| raw.layer.is_tilemap()) && features & TILEMAPS == 0 {
        return Err(FormatError::Corrupt("a tilemap layer without its feature"));
    }
    Ok(())
}

fn within_reach(first: i16, size: u16) -> bool {
    let first = first as i64;
    first >= -(MAX_TILE_REACH as i64) && first + size as i64 <= MAX_TILE_REACH as i64
}

/// Where a tileset tile's image may lie: from tile 0, at most a tileset's side in tiles.
pub(crate) const TILE_BOUND: u16 = (pikslar_core::tileset::MAX_TILE_SIDE / TILE) as u16;

/// Reads one image and its kind. Its table may reach past the canvas, as far as an image can
/// reach, and is charged before it is made. A `bound` asks for the table to lie from tile 0 to
/// that many tiles, as a tileset's tile does, and is checked before the table is allocated.
pub(crate) fn read_image(
    input: &mut In<'_>,
    spent: &mut usize,
    bound: Option<u16>,
) -> Result<(u8, Image), FormatError> {
    let kind = input.u8()?;
    if kind != PIXELS && kind != MAP {
        return Err(FormatError::Unsupported(
            "an image kind from a newer Pikslar",
        ));
    }
    let (x0, y0) = (input.u16()? as i16, input.u16()? as i16);
    let (tw, th) = (input.u16()?, input.u16()?);
    let inside = |first: i16, size: u16| match bound {
        Some(bound) => first >= 0 && first as i64 + size as i64 <= bound as i64,
        None => within_reach(first, size),
    };
    if (tw == 0) != (th == 0) || !inside(x0, tw) || !inside(y0, th) {
        return Err(FormatError::Corrupt("an image table is out of shape"));
    }
    let slots = tw as usize * th as usize;
    // Every tile takes at least one byte, so a shorter file cannot hold this table.
    if input.data.len().saturating_sub(input.pos) < slots {
        return Err(TRUNCATED);
    }
    let table = slots.saturating_mul(std::mem::size_of::<Cell>());
    charge(spent, IMAGE_RECORD_CHARGE.saturating_add(table))?;
    let mut cells: Vec<Cell> = Vec::with_capacity(slots);
    for _ in 0..slots {
        let tile = read_tile(input)?;
        if let Some(tile) = &tile {
            charge(spent, tile.heap_size())?;
        }
        cells.push(tile.map(Arc::new));
    }
    let image = Image::from_tiles(x0, y0, tw, cells)
        .ok_or(FormatError::Corrupt("an image table is out of shape"))?;
    Ok((kind, image))
}

struct RawCel {
    image: usize,
    x: i32,
    y: i32,
    link: u32,
}

fn read_cel(
    input: &mut In<'_>,
    images: usize,
    notes: &mut LoadNotes,
) -> Result<RawCel, FormatError> {
    let len = input.u8()? as usize;
    let mut r = record(input, len, CEL_FIELDS)?;
    if r.u8()? != PIXELS {
        return Err(FormatError::Unsupported(
            "a frame kind from a newer Pikslar",
        ));
    }
    let image = r.u32()? as usize;
    let (x, y) = (i32::from(r.u16()? as i16), i32::from(r.u16()? as i16));
    let link = r.u32()?;
    notes.newer_fields |= len != CEL_FIELDS;
    if image >= images {
        return Err(FormatError::Corrupt("bad image reference"));
    }
    if !Cel::in_range(x, y) {
        return Err(OUT_OF_RANGE);
    }
    Ok(RawCel { image, x, y, link })
}

struct Header {
    features: u32,
    width: usize,
    height: usize,
    colors: Vec<u32>,
    swatches: Vec<u32>,
    frames: Vec<FrameInfo>,
}

fn read_header(input: &mut In<'_>) -> Result<Header, FormatError> {
    let features = input.u32()?;
    if features & !KNOWN_FEATURES != 0 {
        return Err(FormatError::Unsupported("a newer project version"));
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
    let frames = durations
        .into_iter()
        .map(|ms| FrameInfo::new(ms.clamp(MIN_DURATION_MS, MAX_DURATION_MS)))
        .collect();
    Ok(Header {
        features,
        width,
        height,
        colors,
        swatches,
        frames,
    })
}

/// Reads what follows the version number. `input` is the body, checksum already checked.
pub(crate) fn parse<'a>(input: &mut In<'a>, spent: &mut usize) -> Result<Parsed<'a>, FormatError> {
    let mut notes = LoadNotes::default();
    let header = read_header(input)?;
    let layer_count = input.u32()? as usize;
    if layer_count == 0 || layer_count > MAX_LAYERS {
        return Err(FormatError::Corrupt("bad layer count"));
    }
    let raw_layers = (0..layer_count)
        .map(|_| read_layer(input, &mut notes))
        .collect::<Result<Vec<_>, _>>()?;
    check_features(header.features, &raw_layers)?;
    let frame_count = header.frames.len();
    let image_count = input.u32()? as usize;
    if image_count > layer_count * frame_count {
        return Err(FormatError::Corrupt("bad image count"));
    }
    // Every cel is charged before any is read, and every image before its table is made.
    charge(
        spent,
        (layer_count * frame_count).saturating_mul(CEL_CHARGE),
    )?;
    let images = (0..image_count)
        .map(|_| read_image(input, spent, None))
        .collect::<Result<Vec<_>, _>>()?;
    let raw_cels = (0..layer_count)
        .map(|_| {
            (0..frame_count)
                .map(|_| read_cel(input, image_count, &mut notes))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let sections = read_sections(input)?;
    if input.pos != input.data.len() {
        return Err(FormatError::Corrupt("unexpected data after the project"));
    }
    let tilesets = crate::piks_tilesets::read(&sections, header.features, spent, &mut notes)?;
    let doc = assemble(
        (header, raw_layers, images),
        &raw_cels,
        (&sections, tilesets),
        &mut notes,
    )?;
    Ok((doc, sections, notes))
}

/// The images as the layers that place them may hold them: each map cleaned to what its
/// tileset allows. Refused when an image does not suit its layer or no frame uses it.
fn settle_images(
    mut images: Vec<(u8, Image)>,
    (raw_layers, raw_cels): (&[RawLayer], &[Vec<RawCel>]),
    tilesets: &Tilesets,
    notes: &mut LoadNotes,
) -> Result<Vec<Image>, FormatError> {
    let kinds: Vec<LayerKind> = raw_layers.iter().map(|raw| raw.layer.kind).collect();
    let placed: Vec<Vec<usize>> = raw_cels
        .iter()
        .map(|row| row.iter().map(|cel| cel.image).collect())
        .collect();
    let owners = crate::piks_tilesets::owners(&images, &kinds, &placed, tilesets)?;
    crate::piks_tilesets::clean_maps(&mut images, &owners, tilesets, notes);
    let mut used = vec![false; images.len()];
    raw_cels
        .iter()
        .flatten()
        .for_each(|cel| used[cel.image] = true);
    if used.contains(&false) {
        return Err(FormatError::Corrupt("an image no frame uses"));
    }
    Ok(images.into_iter().map(|(_, image)| image).collect())
}

fn assemble(
    (header, raw_layers, images): (Header, Vec<RawLayer>, Vec<(u8, Image)>),
    raw_cels: &[Vec<RawCel>],
    (sections, tilesets): (&[Section<'_>], Tilesets),
    notes: &mut LoadNotes,
) -> Result<Document, FormatError> {
    let images = settle_images(images, (&raw_layers, raw_cels), &tilesets, notes)?;
    // Frames naming one image share it, each from its own place.
    let shared: Vec<Cel> = images.into_iter().map(Cel::from_image).collect();
    let ids: Vec<u32> = raw_layers.iter().map(|raw| raw.persisted_id).collect();
    let mut layers = Vec::with_capacity(raw_layers.len());
    for (raw, row) in raw_layers.into_iter().zip(raw_cels) {
        let cels = row
            .iter()
            .map(|c| shared[c.image].moved_to(c.x, c.y).ok_or(OUT_OF_RANGE))
            .collect::<Result<Vec<_>, _>>()?;
        layers.push(raw.layer.with_cels(cels));
    }
    let colors = Arc::new(ColorTable::from_parts(&header.colors, &header.swatches));
    let mut doc = Document::from_parts(header.width, header.height, colors, layers, header.frames)
        .map_err(|_| OUT_OF_RANGE)?;
    doc.tilesets = Arc::new(tilesets);
    if header.features & ASEPRITE_BLENDING != 0 {
        doc.blending = Blending::Aseprite;
    }
    doc.keep_persisted_ids(&ids)
        .map_err(|_| FormatError::Corrupt("two layers have one number"))?;
    // A link breaking a rule of the links, such as joining layers of two color modes or cels
    // naming different images, keeps each layer's frames that share one image as a hold, and
    // is said to be damaged. No drawing changes.
    let links: Vec<Vec<u32>> = raw_cels
        .iter()
        .map(|row| row.iter().map(|cel| cel.link).collect())
        .collect();
    let broken = doc
        .set_links(&links)
        .map_err(|_| FormatError::Corrupt("holds do not match the frames"))?;
    if broken > 0 && !notes.damaged.contains(&"holds") {
        notes.damaged.push("holds");
    }
    check_groups(&doc)?;
    doc.check_tilemaps()
        .map_err(|_| FormatError::Corrupt("a tilemap layer breaks the tilemap rules"))?;
    check_references(&mut doc, notes)?;
    crate::piks_anim::apply_tags(&mut doc, sections, notes);
    crate::piks_anim::apply_rate(&mut doc, sections, notes);
    crate::piks_palette::apply_groups(&mut doc, sections, notes);
    crate::piks_slices::apply_slices(&mut doc, sections, notes);
    crate::piks_terrain::apply_terrains(&mut doc, sections, notes);
    // Last: the prints and checks are of the frames and layers as they now stand.
    crate::piks_ids::apply_ids(&mut doc, sections, notes);
    crate::piks_doc_id::apply_doc_id(&mut doc, sections);
    keep_foreign(&mut doc, sections, notes);
    Ok(doc)
}

/// Refuses groups out of order, too deep, or with pixels of their own.
fn check_groups(doc: &Document) -> Result<(), FormatError> {
    doc.check_groups()
        .map_err(|_| FormatError::Corrupt("layer groups are out of order"))?;
    let groups = doc.layers().iter().filter(|l| l.kind == LayerKind::Group);
    match groups.flat_map(Layer::cels).all(Cel::is_blank) {
        true => Ok(()),
        false => Err(FormatError::Corrupt("a layer group has pixels")),
    }
}

/// Refuses reference pictures past what placing one allows, and shows each reference's one
/// drawing on every frame, held, as the editor keeps it.
fn check_references(doc: &mut Document, notes: &mut LoadNotes) -> Result<(), FormatError> {
    const TOO_LARGE: FormatError = FormatError::Corrupt("a reference picture is too large");
    let side = MAX_REFERENCE_SIDE.div_ceil(TILE);
    for layer in doc
        .layers()
        .iter()
        .filter(|l| l.kind == LayerKind::Reference)
    {
        if layer.cels().iter().any(|cel| {
            let (_, _, wide, high) = cel.image().table();
            wide > side || high > side
        }) {
            return Err(TOO_LARGE);
        }
    }
    if doc.reference_bytes() > MAX_REFERENCE_BYTES {
        return Err(TOO_LARGE);
    }
    let changed = doc
        .hold_references_counted()
        .map_err(|_| FormatError::Corrupt("holds do not match the frames"))?;
    if changed > 0 {
        notes.damaged.push("reference pictures");
    }
    Ok(())
}

#[cfg(test)]
#[path = "piks_v3_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "piks_v3_links_tests.rs"]
mod links_tests;
