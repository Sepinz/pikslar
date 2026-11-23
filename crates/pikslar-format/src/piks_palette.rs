//! Palette groups in a project file: the `Pgrp` section.
//!
//! Written only when the palette has groups, so a file without any is byte for byte as before.
//! The last letter is lowercase, so a version that does not know the section carries it along
//! unchanged. That is safe because groups name swatch ids and no older version renumbers ids.
//! An id that no longer names a swatch is dropped when read, and said so. A section that does
//! not parse is left out whole and reported as damaged, and never costs the art.

use crate::piks::{In, Out, Section};
use crate::piks_sections::LoadNotes;
use crate::FormatError;
use pikslar_core::color::MAX_COLORS;
use pikslar_core::{Document, GroupKind, PaletteGroup, MAX_GROUPS, MAX_GROUP_NAME};

pub(crate) const GROUPS: &[u8; 4] = b"Pgrp";

/// The longest section read: the most groups, each with the longest name and as many ids as a
/// table holds, plus a little room per record for fields a later version may add. Longer is
/// damage, so a crafted section cannot make reading slow.
pub(crate) const MAX_SECTION_BYTES: usize = 4 + MAX_GROUPS * (MAX_COLORS * 4 + 1024);

const GROUP_KIND: u8 = 0;
const RAMP_KIND: u8 = 1;
/// Kind, name length, and id count: what every record holds before the name and ids.
const FIELDS: usize = 1 + 2 + 4;
const BAD: FormatError = FormatError::Corrupt("palette groups do not parse");

pub(crate) fn write_group(out: &mut Out, group: &PaletteGroup) {
    // Cut where a character ends, so any name the writer is handed is safe.
    let name = crate::palette_file::cut(&group.name, MAX_GROUP_NAME * 4);
    let mut record = Out(Vec::with_capacity(
        FIELDS + name.len() + group.ids.len() * 4,
    ));
    record.u8(match group.kind {
        GroupKind::Group => GROUP_KIND,
        GroupKind::Ramp => RAMP_KIND,
    });
    record.u16(name.len() as u16);
    record.0.extend_from_slice(name.as_bytes());
    record.u32s(&group.ids);
    out.u32(record.0.len() as u32);
    out.0.extend_from_slice(&record.0);
}

/// The groups section, if the palette has groups.
pub(crate) fn groups_section(doc: &Document) -> Option<Vec<u8>> {
    let groups = doc.colors.groups();
    if groups.is_empty() {
        return None;
    }
    let mut out = Out(Vec::new());
    out.u32(groups.len() as u32);
    groups.iter().for_each(|g| write_group(&mut out, g));
    Some(out.0)
}

/// One group. The flag is true if the record held more than this version reads or a kind it
/// does not know, which is read as a plain group.
fn read_group(input: &mut In<'_>) -> Result<(PaletteGroup, bool), FormatError> {
    let len = input.u32()? as usize;
    let mut r = In {
        data: input.bytes(len)?,
        pos: 0,
    };
    let kind = r.u8()?;
    let name_len = r.u16()? as usize;
    let name = std::str::from_utf8(r.bytes(name_len)?).map_err(|_| BAD)?;
    // `u32_list` checks the count against the bytes left before it allocates. A group never
    // names more ids than a table holds.
    let ids = r.u32_list((r.data.len() / 4).min(MAX_COLORS))?;
    let newer = r.pos != len || kind > RAMP_KIND;
    let kind = if kind == RAMP_KIND {
        GroupKind::Ramp
    } else {
        GroupKind::Group
    };
    let name = name.to_owned();
    Ok((PaletteGroup { name, kind, ids }, newer))
}

pub(crate) fn decode(bytes: &[u8]) -> Result<(Vec<PaletteGroup>, bool), FormatError> {
    if bytes.len() > MAX_SECTION_BYTES {
        return Err(BAD);
    }
    let mut input = In {
        data: bytes,
        pos: 0,
    };
    let count = input.u32()? as usize;
    if count > MAX_GROUPS {
        return Err(BAD);
    }
    let mut newer = false;
    let mut groups = Vec::with_capacity(count);
    for _ in 0..count {
        let (group, more) = read_group(&mut input)?;
        newer |= more;
        groups.push(group);
    }
    match input.pos == bytes.len() {
        true => Ok((groups, newer)),
        false => Err(BAD),
    }
}

/// Gives the palette the groups a section holds, after its swatches are known.
pub(crate) fn apply_groups(doc: &mut Document, sections: &[Section], notes: &mut LoadNotes) {
    let mut found = sections.iter().filter(|(tag, _)| *tag == GROUPS);
    let Some((_, bytes)) = found.next() else {
        return;
    };
    // A file carries one section; a second is not guessed at, and the user is told.
    let decoded = match found.next() {
        None => decode(bytes),
        Some(_) => Err(BAD),
    };
    match decoded {
        Ok((groups, newer)) => {
            notes.newer_fields |= newer;
            let table = std::sync::Arc::make_mut(&mut doc.colors);
            notes.palette_trimmed |= table.set_groups(groups);
        }
        Err(_) => notes.damaged.push("palette groups"),
    }
}
