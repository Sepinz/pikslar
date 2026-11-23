//! Reading a brush library. Everything in the bytes is untrusted.

use super::{
    Handle, LibraryBrush, Paint, BODY_FIXED_LEN, HEADER_LEN, MAGIC, MAX_BODY_LEN, MAX_BRUSHES,
    MAX_FILE_BYTES, MAX_NAME_BYTES, MAX_PIXEL_BYTES, MAX_SIDE, RECORD_HEAD_LEN, VERSION,
};
use crate::{crc32, inflate, FormatError};

/// What the reader found besides the brushes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Records the file declared (up to the limit) that could not be loaded.
    pub lost: usize,
    /// The file is from a newer build. It was read as version 1; do not write over it.
    pub newer_version: bool,
    /// The file was longer than the reader's limit and was read only as far as that.
    pub cut: bool,
    /// More brushes or pixels than the limits allow; what fitted was kept.
    pub over_limit: bool,
}

impl Report {
    /// Whether anything was lost or left out.
    pub fn is_clean(&self) -> bool {
        self.lost == 0 && !self.cut && !self.over_limit
    }
}

/// The brushes that could be read, and what happened to the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    pub brushes: Vec<LibraryBrush>,
    pub report: Report,
}

/// Why one record was not loaded.
enum Skip {
    Damaged,
    OverBudget,
}

/// Reads a library. Fails only if the data is not a library at all (too short, wrong first
/// bytes, version 0); otherwise every record that can be read is returned, never more than the
/// limits allow, and the report counts the rest.
pub fn read_library(bytes: &[u8]) -> Result<Library, FormatError> {
    let mut report = Report {
        cut: bytes.len() > MAX_FILE_BYTES,
        ..Report::default()
    };
    let bytes = &bytes[..bytes.len().min(MAX_FILE_BYTES)];
    let head = bytes
        .get(..HEADER_LEN)
        .ok_or(FormatError::Corrupt("not a brush library"))?;
    if head[..4] != MAGIC {
        return Err(FormatError::Corrupt("not a brush library"));
    }
    let version = u32_at(head, 4);
    if version == 0 {
        return Err(FormatError::Corrupt("brush library version 0"));
    }
    report.newer_version = version > VERSION;
    let declared = u32_at(head, 8) as usize;
    report.over_limit = declared > MAX_BRUSHES;
    let wanted = declared.min(MAX_BRUSHES);

    let mut brushes = Vec::new();
    let mut budget = MAX_PIXEL_BYTES;
    let mut pos = HEADER_LEN;
    let mut processed = 0;
    while processed < wanted {
        let Some(body) = next_body(bytes, &mut pos) else {
            break;
        };
        processed += 1;
        match body.and_then(|b| decode_body(b, &mut budget)) {
            Ok(brush) => brushes.push(brush),
            Err(Skip::OverBudget) => {
                report.over_limit = true;
                report.lost += 1;
            }
            Err(Skip::Damaged) => report.lost += 1,
        }
    }
    // Records the file declared but whose framing could not be found.
    report.lost += wanted - processed;
    if !report.over_limit && processed == wanted {
        report.lost += trailing(bytes, pos);
    }
    Ok(Library { brushes, report })
}

/// Records (or stray bytes counted as one) after the last declared record.
fn trailing(bytes: &[u8], mut pos: usize) -> usize {
    let mut count = 0;
    while next_body(bytes, &mut pos).is_some() {
        count += 1;
    }
    count + usize::from(pos < bytes.len())
}

/// The next record's body, with `pos` moved past it. `None` when there is no whole record
/// left to frame; `Some(Err)` when the record is framed but its checksum is wrong.
fn next_body<'a>(bytes: &'a [u8], pos: &mut usize) -> Option<Result<&'a [u8], Skip>> {
    let head = bytes.get(*pos..pos.checked_add(RECORD_HEAD_LEN)?)?;
    let len = u32_at(head, 0) as usize;
    let crc = u32_at(head, 4);
    if len > MAX_BODY_LEN {
        return None;
    }
    let start = *pos + RECORD_HEAD_LEN;
    let body = bytes.get(start..start.checked_add(len)?)?;
    *pos = start + len;
    if crc32(&[body]) == crc {
        Some(Ok(body))
    } else {
        Some(Err(Skip::Damaged))
    }
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[at..at + 4]);
    u32::from_le_bytes(a)
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

/// The fixed fields of one record and where its pixel bytes start.
struct Head<'a> {
    name: String,
    paint: Paint,
    orientation: u8,
    handle: Handle,
    width: u16,
    height: u16,
    packing: u8,
    data: &'a [u8],
}

/// Checks the name and fixed fields of a body whose checksum matched. Control, direction and
/// zero-width characters in the name become spaces, so a name can never break a line of the
/// interface or show as something else.
fn decode_head(body: &[u8]) -> Result<Head<'_>, Skip> {
    let name_len = u16_at(body, 0).ok_or(Skip::Damaged)? as usize;
    if name_len > MAX_NAME_BYTES {
        return Err(Skip::Damaged);
    }
    let name = body.get(2..2 + name_len).ok_or(Skip::Damaged)?;
    let name = std::str::from_utf8(name).map_err(|_| Skip::Damaged)?;
    let name = pikslar_core::names::spaced(name);
    let rest = body.get(2 + name_len..).ok_or(Skip::Damaged)?;
    if rest.len() < BODY_FIXED_LEN {
        return Err(Skip::Damaged);
    }
    let paint = match rest[0] {
        0 => Paint::OwnColors,
        1 => Paint::Shape,
        _ => return Err(Skip::Damaged),
    };
    let orientation = rest[1];
    let handle = match rest[2] {
        0 => Handle::Centre,
        h @ 1..=4 => Handle::Corner(h - 1),
        _ => return Err(Skip::Damaged),
    };
    let width = u16_at(rest, 3).ok_or(Skip::Damaged)?;
    let height = u16_at(rest, 5).ok_or(Skip::Damaged)?;
    if orientation > 7 || !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
        return Err(Skip::Damaged);
    }
    Ok(Head {
        name,
        paint,
        orientation,
        handle,
        width,
        height,
        packing: rest[7],
        data: &rest[BODY_FIXED_LEN..],
    })
}

/// Checks and decodes one body whose checksum matched. `budget` is the decoded pixel bytes
/// still allowed; it is spent only when the record loads.
fn decode_body(body: &[u8], budget: &mut usize) -> Result<LibraryBrush, Skip> {
    let head = decode_head(body)?;
    let need = LibraryBrush::pixel_bytes(head.width, head.height);
    let rgba = match head.packing {
        0 if head.data.len() == need => head.data.to_vec(),
        1 => inflate::inflate(head.data, need).map_err(|_| Skip::Damaged)?,
        _ => return Err(Skip::Damaged),
    };
    if rgba.len() != need {
        return Err(Skip::Damaged);
    }
    if need > *budget {
        return Err(Skip::OverBudget);
    }
    *budget -= need;
    Ok(LibraryBrush {
        name: head.name,
        width: head.width,
        height: head.height,
        paint: head.paint,
        orientation: head.orientation,
        handle: head.handle,
        rgba,
    })
}
