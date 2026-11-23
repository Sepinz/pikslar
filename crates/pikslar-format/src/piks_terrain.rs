//! The `Atil` section of a project file: the terrain sets of tilesets (M11), laid out in
//! `docs/format-v3.md`.
//!
//! Written only when some tileset has a terrain set, so a file without any is byte for byte as
//! before. The last letter is lowercase, so a version that does not know the section carries it
//! along unchanged. That is safe because tile ids are never given twice: a carried slot names a
//! tile that is still there or one that is gone, never another. On load a slot naming a gone
//! tile is emptied and noted, and a set of a tileset that is gone is left out and noted; neither
//! is damage. A section that does not parse is left out whole and reported as damaged, and never
//! costs the art.

use crate::piks::{In, Out, Section};
use crate::piks_sections::LoadNotes;
use crate::piks_v3::record;
use crate::FormatError;
use pikslar_core::autotile::{TerrainKind, TerrainSet, MAX_TERRAINS};
use pikslar_core::tileset::{TileId, Tileset, TilesetId, MAX_TILESETS};
use pikslar_core::Document;

pub(crate) const TERRAINS: &[u8; 4] = b"Atil";

/// Tileset id and set count: what a tileset record holds before its sets.
const TILESET_FIELDS: usize = 4 + 4;
/// Id, kind, name length and slot count: what a set record holds besides its name and slots.
const SET_FIELDS: usize = 2 + 1 + 2 + 1;
/// The least a set takes in the section: its record length and fields.
const LEAST_SET: usize = 4 + SET_FIELDS;
/// The least a tileset takes in the section: its record length and fields.
const LEAST_TILESET: usize = 4 + TILESET_FIELDS;
const BAD: FormatError = FormatError::Corrupt("terrain sets do not parse");
const DAMAGED: &str = "terrain sets";

fn kind_byte(kind: TerrainKind) -> u8 {
    match kind {
        TerrainKind::Edge4 => 0,
        TerrainKind::Corner4 => 1,
        TerrainKind::Blob47 => 2,
    }
}

fn kind_of(byte: u8) -> Option<TerrainKind> {
    match byte {
        0 => Some(TerrainKind::Edge4),
        1 => Some(TerrainKind::Corner4),
        2 => Some(TerrainKind::Blob47),
        _ => None,
    }
}

fn write_set(out: &mut Out, set: &TerrainSet) {
    let name = crate::piks::cut_name(set.name());
    let mut r = Out(Vec::with_capacity(
        SET_FIELDS + name.len() + set.slots().len() * 4,
    ));
    r.u16(set.id());
    r.u8(kind_byte(set.kind()));
    r.u16(name.len() as u16);
    r.0.extend_from_slice(name.as_bytes());
    r.u8(set.slots().len() as u8);
    set.slots()
        .iter()
        .for_each(|slot| r.u32(slot.map_or(0, TileId::get)));
    out.u32(r.0.len() as u32);
    out.0.extend_from_slice(&r.0);
}

fn write_tileset(out: &mut Out, tileset: &Tileset) {
    let mut r = Out(Vec::new());
    r.u32(tileset.id().get());
    r.u32(tileset.terrains().len() as u32);
    tileset
        .terrains()
        .iter()
        .for_each(|set| write_set(&mut r, set));
    out.u32(r.0.len() as u32);
    out.0.extend_from_slice(&r.0);
}

/// The section, if any tileset has a terrain set. Tilesets with none are left out.
pub(crate) fn terrains_section(doc: &Document) -> Option<Vec<u8>> {
    let with: Vec<&Tileset> = doc
        .tilesets
        .iter()
        .filter(|set| !set.terrains().is_empty())
        .collect();
    if with.is_empty() {
        return None;
    }
    let mut out = Out(Vec::new());
    out.u32(with.len() as u32);
    with.iter().for_each(|set| write_tileset(&mut out, set));
    Some(out.0)
}

/// What a section held that this version reads, and what it had to leave out.
#[derive(Default)]
struct Decoded {
    tilesets: Vec<(TilesetId, Vec<TerrainSet>)>,
    /// Fields after what this version reads, or a kind it does not know.
    newer: bool,
    /// A slot naming a tile no tileset can hold, emptied.
    trimmed: bool,
}

/// One set, or `None` for a kind from a newer version, which is left out.
fn read_set(input: &mut In<'_>, decoded: &mut Decoded) -> Result<Option<TerrainSet>, FormatError> {
    let len = input.u32()? as usize;
    let mut r = record(input, len, SET_FIELDS)?;
    let id = r.u16()?;
    let kind = r.u8()?;
    let name_len = r.u16()? as usize;
    let name = std::str::from_utf8(r.bytes(name_len)?).map_err(|_| BAD)?;
    let count = r.u8()? as usize;
    let raw = r.bytes(count * 4)?;
    decoded.newer |= r.pos != len;
    let Some(kind) = kind_of(kind) else {
        decoded.newer = true;
        return Ok(None);
    };
    if count != kind.slot_count() {
        return Err(BAD);
    }
    let slots = raw.chunks_exact(4).map(|b| {
        let number = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let tile = TileId::new(number);
        decoded.trimmed |= number != 0 && tile.is_none();
        tile
    });
    let slots = slots.collect();
    TerrainSet::from_parts(id, name.to_owned(), kind, slots)
        .ok_or(BAD)
        .map(Some)
}

fn read_tileset(input: &mut In<'_>, decoded: &mut Decoded) -> Result<(), FormatError> {
    let len = input.u32()? as usize;
    let mut r = record(input, len, TILESET_FIELDS)?;
    let tileset = TilesetId::new(r.u32()?);
    let count = r.u32()? as usize;
    if count > MAX_TERRAINS || r.data.len() - r.pos < count.saturating_mul(LEAST_SET) {
        return Err(BAD);
    }
    let mut sets: Vec<TerrainSet> = Vec::with_capacity(count);
    for _ in 0..count {
        if let Some(set) = read_set(&mut r, decoded)? {
            if sets.iter().any(|other| other.id() == set.id()) {
                return Err(BAD);
            }
            sets.push(set);
        }
    }
    decoded.newer |= r.pos != len;
    if decoded.tilesets.iter().any(|(id, _)| *id == tileset) {
        return Err(BAD);
    }
    decoded.tilesets.push((tileset, sets));
    Ok(())
}

fn decode(bytes: &[u8]) -> Result<Decoded, FormatError> {
    let mut input = In {
        data: bytes,
        pos: 0,
    };
    let count = input.u32()? as usize;
    if count > MAX_TILESETS || bytes.len() - input.pos < count.saturating_mul(LEAST_TILESET) {
        return Err(BAD);
    }
    let mut decoded = Decoded::default();
    decoded.tilesets.reserve_exact(count);
    for _ in 0..count {
        read_tileset(&mut input, &mut decoded)?;
    }
    match input.pos == bytes.len() {
        true => Ok(decoded),
        false => Err(BAD),
    }
}

/// Gives the tilesets the terrain sets a section holds, once the tilesets and maps are known.
/// Nothing is given unless the whole section parses.
pub(crate) fn apply_terrains(doc: &mut Document, sections: &[Section], notes: &mut LoadNotes) {
    let mut found = sections.iter().filter(|(tag, _)| *tag == TERRAINS);
    let Some((_, bytes)) = found.next() else {
        return;
    };
    let decoded = match (found.next(), decode(bytes)) {
        (None, Ok(decoded)) => decoded,
        _ => return notes.damaged.push(DAMAGED),
    };
    // All or nothing: every tileset's sets are checked before any is given, so a refusal part
    // way cannot leave earlier tilesets holding sets. A tileset a version that carried the
    // section removed is not a refusal.
    let taken = |(tileset, sets): &(TilesetId, Vec<TerrainSet>)| {
        let found = doc.check_terrains(*tileset, sets);
        matches!(found, Ok(()) | Err(pikslar_core::LimitError::NoSuchItem))
    };
    let refused = !decoded.tilesets.iter().all(taken);
    if refused {
        return notes.damaged.push(DAMAGED);
    }
    notes.newer_fields |= decoded.newer;
    notes.terrain_trimmed |= decoded.trimmed;
    for (tileset, sets) in decoded.tilesets {
        match doc.load_terrains(tileset, sets) {
            Ok(trimmed) => notes.terrain_trimmed |= trimmed,
            Err(pikslar_core::LimitError::NoSuchItem) => notes.terrain_trimmed = true,
            // Checked above, so this is not expected; if it happens, the user is told and the
            // file is guarded like any other damaged section, never silently half read.
            Err(_) => {
                if !notes.damaged.contains(&DAMAGED) {
                    notes.damaged.push(DAMAGED);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "piks_terrain_tests.rs"]
mod tests;
