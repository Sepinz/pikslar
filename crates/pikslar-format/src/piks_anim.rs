//! Tags and holds in a project file.
//!
//! Sections of their own, written only when used, so a plain drawing stays as small as it
//! was. A section that does not make sense is left out and reported. It never costs the art.

use crate::piks::{In, Out, Section};
use crate::piks_sections::LoadNotes;
use crate::FormatError;
use pikslar_core::{
    Direction, Document, Tag, MAX_FRAMES, MAX_FRAME_RATE, MAX_TAGS, MAX_TAG_NAME, MAX_TAG_REPEAT,
};

pub(crate) const TAGS: &[u8; 4] = b"TAGS";
pub(crate) const LINKS: &[u8; 4] = b"LINK";
pub(crate) const RATE: &[u8; 4] = b"Rate";

const BAD: FormatError = FormatError::Corrupt("bad animation data");

fn direction_code(d: Direction) -> u8 {
    match d {
        Direction::Forward => 0,
        Direction::Reverse => 1,
        Direction::PingPong => 2,
    }
}

fn direction_from(code: u8) -> Option<Direction> {
    match code {
        0 => Some(Direction::Forward),
        1 => Some(Direction::Reverse),
        2 => Some(Direction::PingPong),
        _ => None,
    }
}

fn encode_tags(doc: &Document) -> Vec<u8> {
    let mut out = Out(Vec::new());
    out.u32(doc.tags.len() as u32);
    for tag in &doc.tags {
        out.u8(direction_code(tag.direction));
        out.u32(tag.from as u32);
        out.u32(tag.to as u32);
        out.u16(tag.name.len() as u16);
        out.0.extend_from_slice(tag.name.as_bytes());
    }
    // After the records, where a version that knows no repeats stops reading.
    if doc.tags.iter().any(|t| t.repeat != 0) {
        doc.tags.iter().for_each(|t| out.u16(t.repeat));
    }
    out.0
}

/// The tags section, if the document has tags.
pub(crate) fn tags_section(doc: &Document) -> Option<Vec<u8>> {
    (!doc.tags.is_empty()).then(|| encode_tags(doc))
}

/// The tags, and whether bytes a later version added follow what this one reads.
fn decode_tags(bytes: &[u8], frames: usize) -> Result<(Vec<Tag>, bool), FormatError> {
    let mut input = In {
        data: bytes,
        pos: 0,
    };
    let count = input.u32()? as usize;
    if count > MAX_TAGS {
        return Err(BAD);
    }
    let mut tags = Vec::with_capacity(count);
    for _ in 0..count {
        let direction = direction_from(input.u8()?).ok_or(BAD)?;
        let (from, to) = (input.u32()? as usize, input.u32()? as usize);
        let length = input.u16()? as usize;
        let name = std::str::from_utf8(input.bytes(length)?).map_err(|_| BAD)?;
        if from > to || to >= frames {
            return Err(BAD);
        }
        tags.push(Tag {
            name: name.chars().take(MAX_TAG_NAME).collect(),
            from,
            to,
            direction,
            repeat: 0,
            id: pikslar_core::TagId::fresh(),
        });
    }
    if input.pos < bytes.len() {
        for tag in &mut tags {
            tag.repeat = input.u16().map_err(|_| BAD)?.min(MAX_TAG_REPEAT);
        }
    }
    Ok((tags, input.pos < bytes.len()))
}

fn decode_links(bytes: &[u8], doc: &Document) -> Result<Vec<Vec<u32>>, FormatError> {
    let mut input = In {
        data: bytes,
        pos: 0,
    };
    let lists = doc
        .layers()
        .iter()
        .map(|_| {
            let list = input.u32_list(MAX_FRAMES)?;
            (list.len() == doc.frames.len()).then_some(list).ok_or(BAD)
        })
        .collect::<Result<Vec<_>, _>>()?;
    (input.pos == bytes.len()).then_some(lists).ok_or(BAD)
}

/// Frames that claim to share a drawing are made to, so edits cannot make them differ.
fn apply_links(doc: &mut Document, lists: Vec<Vec<u32>>) {
    for (layer, links) in lists.iter().enumerate() {
        let applied = doc.set_holds(layer, links);
        debug_assert!(applied.is_ok(), "decode_links checks every list's length");
    }
}

fn find<'a>(sections: &[Section<'a>], tag: &[u8; 4]) -> Option<&'a [u8]> {
    sections.iter().find(|(t, _)| *t == tag).map(|(_, b)| *b)
}

/// Fills in the tags a section describes.
pub(crate) fn apply_tags(doc: &mut Document, sections: &[Section], notes: &mut LoadNotes) {
    match find(sections, TAGS).map(|b| decode_tags(b, doc.frames.len())) {
        Some(Ok((tags, newer))) => {
            doc.tags = tags;
            notes.newer_fields |= newer;
        }
        Some(Err(_)) => notes.damaged.push("tags"),
        None => {}
    }
}

/// Fills in what the sections of a version 2 file describe.
pub(crate) fn apply(doc: &mut Document, sections: &[Section], notes: &mut LoadNotes) {
    apply_tags(doc, sections, notes);
    let find = |tag: &[u8; 4]| find(sections, tag);
    match find(LINKS).map(|b| decode_links(b, doc)) {
        Some(Ok(lists)) => apply_links(doc, lists),
        Some(Err(_)) => notes.damaged.push("holds"),
        None => {}
    }
}

/// The frame rate section, if the document is timed by frame rate: one byte. Lowercase last
/// letter, so a version that does not know it carries it along.
pub(crate) fn rate_section(doc: &Document) -> Option<Vec<u8>> {
    doc.frame_rate.map(|fps| vec![fps])
}

/// Times the document by the rate a section gives. One that makes no sense leaves it in
/// milliseconds and is reported.
pub(crate) fn apply_rate(doc: &mut Document, sections: &[Section], notes: &mut LoadNotes) {
    match find(sections, RATE) {
        Some(&[fps @ 1..=MAX_FRAME_RATE]) => {
            doc.frame_rate = Some(fps);
            snap_to_ticks(doc, notes);
        }
        Some(_) => notes.damaged.push("frame rate"),
        None => {}
    }
}

/// Times by rate are whole ticks. A file from a version that retimed in 10 ms steps may hold
/// others, which are rounded to the nearest tick and reported, since saving would rewrite them.
fn snap_to_ticks(doc: &mut Document, notes: &mut LoadNotes) {
    let mut changed = false;
    for index in 0..doc.frames.len() {
        let Some(ms) = doc.frames.get(index).map(|f| f.duration_ms) else {
            continue;
        };
        let snapped = doc.snapped(ms);
        if snapped != ms {
            if let Some(frame) = doc.frames.get_mut(index) {
                frame.duration_ms = snapped;
                changed = true;
            }
        }
    }
    if changed {
        notes.rounded_to_fps = doc.frame_rate;
    }
}
