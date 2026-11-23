//! Slices in a project file.
//!
//! A section of its own, written only when there are slices. A slice that makes no sense is
//! left out and reported. It never costs the art.

use crate::piks::{In, Out, Section};
use crate::piks_sections::LoadNotes;
use crate::FormatError;
use pikslar_core::{Document, Slice, SliceRect, MAX_SLICES};

/// Capital last letter: slices are measured on the canvas, which an older version could resize
/// without moving them, so it must not carry the section along.
pub(crate) const SLICES: &[u8; 4] = b"SLIC";

const HAS_PIVOT: u8 = 1;
const HAS_CENTER: u8 = 2;
/// Name length, color, bounds and flags: what every record holds.
const FIELDS: usize = 2 + 4 + 16 + 1;

fn write_rect(out: &mut Out, r: SliceRect) {
    out.i32(r.x);
    out.i32(r.y);
    out.u32(r.width);
    out.u32(r.height);
}

/// One framed record, or nothing for a slice whose record cannot be measured in the length
/// fields (a name of tens of thousands of bytes; the editor never makes one).
fn slice_record(slice: &Slice) -> Option<Vec<u8>> {
    let mut r = Out(Vec::new());
    r.u16(u16::try_from(slice.name.len()).ok()?);
    r.0.extend_from_slice(slice.name.as_bytes());
    r.u32(slice.color);
    write_rect(&mut r, slice.bounds);
    let flags = (u8::from(slice.pivot.is_some()) * HAS_PIVOT)
        | (u8::from(slice.center.is_some()) * HAS_CENTER);
    r.u8(flags);
    if let Some((x, y)) = slice.pivot {
        r.i32(x);
        r.i32(y);
    }
    if let Some(center) = slice.center {
        write_rect(&mut r, center);
    }
    let mut framed = Out(Vec::new());
    framed.u16(u16::try_from(r.0.len()).ok()?);
    framed.0.extend_from_slice(&r.0);
    Some(framed.0)
}

/// The slices section, if the document has slices that can be written.
pub(crate) fn slices_section(doc: &Document) -> Option<Vec<u8>> {
    let records: Vec<Vec<u8>> = doc.slices.iter().filter_map(slice_record).collect();
    if records.is_empty() {
        return None;
    }
    let mut out = Out(Vec::new());
    out.u32(u32::try_from(records.len()).ok()?);
    records.iter().for_each(|r| out.0.extend_from_slice(r));
    Some(out.0)
}

fn read_rect(r: &mut In<'_>) -> Result<SliceRect, FormatError> {
    Ok(SliceRect {
        x: r.i32()?,
        y: r.i32()?,
        width: r.u32()?,
        height: r.u32()?,
    })
}

/// One slice, and whether its record held more than this version reads. A slice that does not
/// pass [`Slice::checked`] is an error, but one the next record can still follow.
fn read_slice(input: &mut In<'_>) -> Result<Result<(Slice, bool), FormatError>, FormatError> {
    let len = input.u16()? as usize;
    // A record too short is skipped whole, so the next one can still be read. Running out of
    // data is the only error that ends the section.
    let held = input.bytes(len)?;
    let mut r = In { data: held, pos: 0 };
    if len < FIELDS {
        return Ok(Err(FormatError::Corrupt("a record is too short")));
    }
    Ok(read_fields(&mut r, len))
}

fn read_fields(r: &mut In<'_>, len: usize) -> Result<(Slice, bool), FormatError> {
    let bad = FormatError::Corrupt("bad slice");
    let name_len = r.u16()? as usize;
    let name = std::str::from_utf8(r.bytes(name_len)?).map_err(|_| bad)?;
    let color = r.u32()?;
    let bounds = read_rect(r)?;
    let flags = r.u8()?;
    let pivot = match flags & HAS_PIVOT {
        0 => None,
        _ => Some((r.i32()?, r.i32()?)),
    };
    let center = match flags & HAS_CENTER {
        0 => None,
        _ => Some(read_rect(r)?),
    };
    let slice = Slice {
        name: name.to_owned(),
        color,
        bounds,
        pivot,
        center,
    };
    let newer = r.pos != len || flags & !(HAS_PIVOT | HAS_CENTER) != 0;
    let slice = slice
        .checked()
        .map_err(|_| FormatError::Corrupt("bad slice"))?;
    Ok((slice, newer))
}

/// The good slices of a section, and whether anything in it was left out or newer.
fn decode(bytes: &[u8]) -> (Vec<Slice>, bool, bool) {
    let mut input = In {
        data: bytes,
        pos: 0,
    };
    let Ok(count) = input.u32() else {
        return (Vec::new(), true, false);
    };
    let count = count as usize;
    let mut damaged = count > MAX_SLICES;
    let (mut slices, mut newer) = (Vec::new(), false);
    for _ in 0..count.min(MAX_SLICES) {
        match read_slice(&mut input) {
            Ok(Ok((slice, more))) => {
                slices.push(slice);
                newer |= more;
            }
            Ok(Err(_)) => damaged = true,
            Err(_) => {
                damaged = true;
                break;
            }
        }
    }
    // Records left over after the count are not read, and not carried along either.
    damaged |= !damaged && input.pos != bytes.len();
    (slices, damaged, newer)
}

/// Fills in the slices a section describes, keeping those that make sense.
pub(crate) fn apply_slices(doc: &mut Document, sections: &[Section], notes: &mut LoadNotes) {
    let mut found = sections.iter().filter(|(t, _)| *t == SLICES);
    let Some((_, bytes)) = found.next() else {
        return;
    };
    let (slices, damaged, newer) = decode(bytes);
    doc.slices = slices;
    // Only the first section is read; a second would be lost by saving.
    if damaged || found.next().is_some() {
        notes.damaged.push("slices");
    }
    notes.newer_fields |= newer;
}
