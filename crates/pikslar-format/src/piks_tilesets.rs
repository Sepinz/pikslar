//! The `TSET` section of a project file: tilesets and their tiles (M10), laid out in
//! `docs/format-v3.md`. Map layers and map images are in `piks_v3`; this reads and writes the
//! tileset table and settles what the map cells may name once the table is known.
//!
//! The last letter of the tag is a capital: tile ids are placed by map cells, which an older
//! version that cannot read them would break if it carried the section along. Such a version
//! refuses the file anyway, by required feature bit 2, while there is any tileset.

use crate::piks::{charge, In, Out, Section};
use crate::piks_sections::LoadNotes;
use crate::piks_v3::{read_image, record, write_image, MAP, TILE_BOUND};
use crate::FormatError;
use pikslar_core::orient::FLIPPED_DIAGONALLY;
use pikslar_core::tilemap::MapCell;
use pikslar_core::tileset::{
    clean_name, stored_tile_bytes, TileEntry, TileId, Tileset, TilesetId, Tilesets, MAX_TILES,
    MAX_TILESETS, MAX_TILE_BYTES,
};
use pikslar_core::{ColorMode, Document, Image, LayerKind};

pub(crate) const TILESETS: &[u8; 4] = b"TSET";
/// Id, name length, mode, tile width and height, next tile id, tile count.
const FIELDS: usize = 4 + 2 + 1 + 2 + 2 + 4 + 4;
/// A tile is its id and an image record, which is at least its kind, place and size.
const LEAST_TILE: usize = 4 + 9;
const DAMAGED: FormatError = FormatError::Corrupt("the tilesets are damaged");

fn tileset_record(set: &Tileset) -> Vec<u8> {
    let mut r = Out(Vec::new());
    r.u32(set.id().get());
    let name = crate::piks::cut_name(set.name());
    r.u16(name.len() as u16);
    r.0.extend_from_slice(name.as_bytes());
    r.u8(matches!(set.mode(), ColorMode::Rgba) as u8);
    let (w, h) = set.tile_size();
    r.u16(w as u16);
    r.u16(h as u16);
    r.u32(set.next_tile_id());
    r.u32(set.len() as u32);
    for entry in set.tiles() {
        r.u32(entry.id.get());
        write_image(&mut r, &entry.image, 0);
    }
    r.0
}

/// The section, if the document has a tileset, or has had one: the counters are kept so that an
/// id is never given twice, even after every tileset is removed. `None` for a document that
/// never had one, which is saved as before.
pub(crate) fn tilesets_section(doc: &Document) -> Option<Vec<u8>> {
    if *doc.tilesets == Tilesets::default() {
        return None;
    }
    let mut out = Out(Vec::new());
    out.u32(doc.tilesets.next_id());
    out.u32(doc.tilesets.len() as u32);
    for set in doc.tilesets.iter() {
        let record = tileset_record(set);
        out.u32(u32::try_from(record.len()).ok()?);
        out.0.extend_from_slice(&record);
    }
    Some(out.0)
}

/// One tile's image, refused if it is a map, charged to `spent` what it holds once read: what
/// `read_image` charges any image, topped up to [`stored_tile_bytes`], which counts a tile's own
/// record and index place too.
pub(crate) fn read_tile_image(r: &mut In<'_>, spent: &mut usize) -> Result<Image, FormatError> {
    let before = *spent;
    let (kind, image) = read_image(r, spent, Some(TILE_BOUND))?;
    if kind == MAP {
        return Err(DAMAGED);
    }
    let charged = spent.saturating_sub(before);
    charge(spent, stored_tile_bytes(&image).saturating_sub(charged))?;
    Ok(image)
}

fn read_tiles(
    r: &mut In<'_>,
    count: usize,
    spent: &mut usize,
    pixels: &mut usize,
) -> Result<Vec<TileEntry>, FormatError> {
    if count > MAX_TILES || r.data.len() - r.pos < count.saturating_mul(LEAST_TILE) {
        return Err(DAMAGED);
    }
    let mut tiles = Vec::with_capacity(count);
    for _ in 0..count {
        let id = TileId::new(r.u32()?).ok_or(DAMAGED)?;
        let image = read_tile_image(r, spent)?;
        // Counted as the editor counts it, so whatever the editor lets be made opens again.
        *pixels = pixels.saturating_add(stored_tile_bytes(&image));
        if *pixels > MAX_TILE_BYTES {
            return Err(FormatError::FileTooLarge);
        }
        tiles.push(TileEntry {
            id,
            image: image.into(),
        });
    }
    Ok(tiles)
}

fn read_tileset(
    input: &mut In<'_>,
    spent: &mut usize,
    pixels: &mut usize,
    notes: &mut LoadNotes,
) -> Result<Tileset, FormatError> {
    let len = input.u32()? as usize;
    let mut r = record(input, len, FIELDS)?;
    let id = r.u32()?;
    let name_len = r.u16()? as usize;
    let name = std::str::from_utf8(r.bytes(name_len)?).map_err(|_| DAMAGED)?;
    let mode = match r.u8()? {
        0 => ColorMode::Palette,
        1 => ColorMode::Rgba,
        _ => {
            return Err(FormatError::Unsupported(
                "a tileset color mode from a newer Pikslar",
            ))
        }
    };
    let (w, h) = (r.u16()? as usize, r.u16()? as usize);
    let next_tile = r.u32()?;
    let count = r.u32()? as usize;
    let tiles = read_tiles(&mut r, count, spent, pixels)?;
    notes.newer_fields |= r.pos != len;
    let cleaned = clean_name(name);
    if cleaned != name && !notes.damaged.contains(&"tileset names") {
        notes.damaged.push("tileset names");
    }
    Tileset::from_parts(TilesetId::new(id), cleaned, mode, (w, h), next_tile, tiles).ok_or(DAMAGED)
}

/// Reads the tileset table. A file with a tileset needs required feature bit 2 and a file may
/// carry only one `TSET`; anything else that does not parse is refused whole, since layers place
/// the tiles.
pub(crate) fn read(
    sections: &[Section<'_>],
    features: u32,
    spent: &mut usize,
    notes: &mut LoadNotes,
) -> Result<Tilesets, FormatError> {
    let mut found = sections.iter().filter(|(tag, _)| *tag == TILESETS);
    let Some((_, data)) = found.next() else {
        return Ok(Tilesets::default());
    };
    if found.next().is_some() {
        return Err(FormatError::Corrupt("two tileset sections"));
    }
    let mut input = In { data, pos: 0 };
    let next_id = input.u32()?;
    let count = input.u32()? as usize;
    if count > MAX_TILESETS {
        return Err(DAMAGED);
    }
    let mut pixels = 0;
    let mut list = Vec::with_capacity(count);
    for _ in 0..count {
        list.push(read_tileset(&mut input, spent, &mut pixels, notes)?);
    }
    if input.pos != data.len() {
        return Err(DAMAGED);
    }
    let table = Tilesets::from_parts(next_id, list).ok_or(DAMAGED)?;
    if !table.is_empty() && features & crate::piks_v3::TILEMAPS == 0 {
        return Err(FormatError::Corrupt("tilesets without their feature"));
    }
    Ok(table)
}

/// The tileset each image belongs to: the one its map layers use, `None` for pixels. Refused when
/// a layer names a tileset the file lacks or of another colour mode, when an image is a map for
/// one layer and pixels for another, when a map drawing is shared by layers of two tilesets, or
/// when a map layer's image is not a map.
pub(crate) fn owners(
    images: &[(u8, Image)],
    kinds: &[LayerKind],
    placed: &[Vec<usize>],
    tilesets: &Tilesets,
) -> Result<Vec<Option<TilesetId>>, FormatError> {
    let mut owners: Vec<Option<TilesetId>> = vec![None; images.len()];
    for (kind, row) in kinds.iter().zip(placed) {
        let wants = if matches!(kind, LayerKind::Tilemap(_)) {
            MAP
        } else {
            0
        };
        let tileset = match kind {
            LayerKind::Tilemap(id) => {
                tilesets
                    .get(*id)
                    .ok_or(FormatError::Corrupt("a tilemap layer has no tileset"))?;
                Some(*id)
            }
            _ => None,
        };
        for &at in row {
            if images[at].0 != wants {
                return Err(FormatError::Corrupt("an image does not suit its layer"));
            }
            if owners[at].is_some() && owners[at] != tileset {
                return Err(FormatError::Corrupt("a map is shared by two tilesets"));
            }
            owners[at] = tileset;
        }
    }
    Ok(owners)
}

/// Makes each map image what the tileset allows: a cell naming a tile the tileset lacks, or an
/// empty cell with flag bits, becomes empty, and a diagonal flip on oblong tiles is dropped. Any
/// of these is noted, so Save asks for a new name and the file keeps what was lost.
pub(crate) fn clean_maps(
    images: &mut [(u8, Image)],
    owners: &[Option<TilesetId>],
    tilesets: &Tilesets,
    notes: &mut LoadNotes,
) {
    for ((_, image), owner) in images.iter_mut().zip(owners) {
        let Some(set) = owner.and_then(|id| tilesets.get(id)) else {
            continue;
        };
        let square = set.tile_size().0 == set.tile_size().1;
        let mut changed = false;
        let cleaned = image.map_cells(|bits| {
            let cell = MapCell::from_bits(bits);
            let kept = match cell.tile() {
                None => 0,
                Some(tile) if set.position(tile).is_none() => 0,
                Some(_) if !square => bits & !FLIPPED_DIAGONALLY,
                Some(_) => bits,
            };
            changed |= kept != bits;
            kept
        });
        if changed {
            *image = cleaned;
            if !notes.damaged.contains(&"tilemap cells") {
                notes.damaged.push("tilemap cells");
            }
        }
    }
}

#[cfg(test)]
#[path = "piks_tilesets_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "piks_tile_budget_tests.rs"]
mod budget_tests;
