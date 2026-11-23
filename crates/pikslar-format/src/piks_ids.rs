//! Persisted frame ids and the id floors in a project file: the `Nids` section.
//!
//! Written only when it says more than opening the file would work out anyway: when a floor is
//! above the ids in use, or the frames are not numbered 1 to n in order. Any other document
//! saves byte for byte as before. The last letter is lowercase, so a build that does not know
//! the section carries it along unchanged, even after editing the frames or layers.
//!
//! Each listed id carries what it was given for: a frame its print, a layer its check (a group's covers the layers it holds, so a group that an older build refilled is not matched) (see
//! `piks_ids_hash`). On open a frame keeps its id only if its print is in the list as often as
//! it is among the frames, and a listed layer keeps its id only if its check is unchanged.
//! Every other frame or layer gets an id above every id given, with a note, so no old id comes
//! to name something else, except between frames, or blank layers, that nothing in the file
//! tells apart. Edits that leave prints and checks alone (tileset pixels, palette colours,
//! layer settings, a cel moved to another layer of its frame) keep the ids: the frames are
//! still the same frames. The floors are kept whatever an older build did, since they only
//! rise.
//!
//! Layout, little-endian: `u32` version (1), `u32` layer floor, `u32` frame floor, `u32` layer
//! count, that many pairs of `u32` layer id and `u64` check from the bottom, `u32` frame count
//! (0 for no list), that many pairs of `u32` frame id and `u64` print in timeline order. Every
//! later version keeps the version and both floors where they are. See `docs/format-v3.md`.

use crate::piks::{In, Out, Section};
use crate::piks_ids_hash::{frame_prints, layer_checks};
use crate::piks_sections::LoadNotes;
use crate::FormatError;
use pikslar_core::{
    document::MAX_LAYERS, Document, FrameInfo, PersistedLayerId, MAX_FRAMES, MAX_FRAME_ID,
};
use std::borrow::Cow;
use std::collections::HashSet;

pub(crate) const IDS: &[u8; 4] = b"Nids";
pub(crate) const VERSION: u32 = 1;
const BAD: FormatError = FormatError::Corrupt("frame and layer ids do not parse");
/// What `lines` and `damaged` call the section.
pub(crate) const DAMAGED: &str = "frame and layer ids";
/// The highest floors a file may give unless a listed id is as high: half the ids, so a file
/// can never leave an opened document without room for new ones.
pub(crate) const LAYER_FLOOR_CAP: u32 = PersistedLayerId::MAX / 2;
pub(crate) const FRAME_FLOOR_CAP: u32 = MAX_FRAME_ID / 2;

/// An id and the print or check of what it was given for.
pub(crate) type Pair = (u32, u64);

/// What a version 1 section holds.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Ids {
    pub(crate) layer_floor: u32,
    pub(crate) frame_floor: u32,
    /// The layers in use when it was written, from the bottom.
    pub(crate) layers: Vec<Pair>,
    /// The frames in timeline order; empty for no list.
    pub(crate) frames: Vec<Pair>,
    /// Bytes followed what this version reads.
    pub(crate) newer: bool,
}

fn layer_ids(doc: &Document) -> Vec<u32> {
    let ids = doc.layers().iter().map(|l| l.persisted_id().get());
    ids.collect()
}

/// Distinct, none 0 and none above `floor`.
fn fit_under(ids: &[u32], floor: u32) -> bool {
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    let distinct = sorted.windows(2).all(|pair| pair[0] != pair[1]);
    let above = sorted.last().is_some_and(|&id| id > floor);
    sorted.first() != Some(&0) && distinct && !above
}

/// True if the frame ids are distinct and in range, as the document's commands keep them.
fn valid_frame_ids(doc: &Document) -> bool {
    let ids: Vec<u32> = doc.frames.iter().map(FrameInfo::id).collect();
    fit_under(&ids, MAX_FRAME_ID)
}

/// The document with ids it can save: frames made by hand (`doc.frames` is public) may share
/// an id or have none. The first frame with an id keeps it; the others get new ones, so the
/// file opens to exactly what it holds and saves the same again.
fn usable(doc: &Document) -> Cow<'_, Document> {
    if valid_frame_ids(doc) {
        return Cow::Borrowed(doc);
    }
    let mut seen = HashSet::new();
    let mut keep = |f: &FrameInfo| (1..=MAX_FRAME_ID).contains(&f.id()) && seen.insert(f.id());
    let ids: Vec<Option<u32>> = doc
        .frames
        .iter()
        .map(|f| keep(f).then_some(f.id()))
        .collect();
    let mut fixed = doc.clone();
    if fixed.keep_some_frame_ids(&ids).is_err() {
        fixed.renumber_frames();
    }
    Cow::Owned(fixed)
}

fn pairs(out: &mut Out, ids: &[u32], prints: &[u64]) {
    out.u32(ids.len() as u32);
    for (&id, print) in ids.iter().zip(prints) {
        out.u32(id);
        out.0.extend_from_slice(&print.to_le_bytes());
    }
}

/// The floor as `floor_fits` takes it: above the cap only up to the highest listed id. A floor
/// past both came from an id past the cap that a damaged or hand-made file gave and that is gone
/// now. Writing the lower floor lets the file open with its ids; an id between it and the old
/// floor can be given again, but only after that many new layers or frames.
fn readable_floor(floor: u32, cap: u32, listed: &[u32]) -> u32 {
    let highest = listed.iter().copied().max().unwrap_or(0);
    floor.min(cap.max(highest))
}

/// The section, if it says anything opening the file would not work out by itself.
pub(crate) fn ids_section(doc: &Document) -> Option<Vec<u8>> {
    let doc = usable(doc);
    let frames: Vec<u32> = doc.frames.iter().map(FrameInfo::id).collect();
    let layers = layer_ids(&doc);
    let count = frames.len() as u32;
    let numbered = frames.iter().copied().eq(1..=count);
    let layers_plain = doc.layer_floor() <= layers.iter().copied().max().unwrap_or(0);
    if numbered && layers_plain && doc.frame_floor() <= count {
        return None;
    }
    let mut out = Out(Vec::with_capacity(20 + (layers.len() + frames.len()) * 12));
    out.u32(VERSION);
    out.u32(readable_floor(doc.layer_floor(), LAYER_FLOOR_CAP, &layers));
    out.u32(readable_floor(doc.frame_floor(), FRAME_FLOOR_CAP, &frames));
    pairs(&mut out, &layers, &layer_checks(&doc));
    pairs(&mut out, &frames, &frame_prints(&doc));
    Some(out.0)
}

/// A count, checked against `max` and the bytes left before anything is allocated, then that
/// many pairs.
fn read_pairs(input: &mut In<'_>, max: usize) -> Result<Vec<Pair>, FormatError> {
    let count = input.u32()? as usize;
    let len = count.checked_mul(12).filter(|_| count <= max).ok_or(BAD)?;
    let bytes = input.bytes(len)?;
    let pair = |chunk: &[u8]| -> Option<Pair> {
        let id = u32::from_le_bytes(chunk.get(..4)?.try_into().ok()?);
        Some((id, u64::from_le_bytes(chunk.get(4..12)?.try_into().ok()?)))
    };
    bytes
        .chunks_exact(12)
        .map(pair)
        .collect::<Option<_>>()
        .ok_or(BAD)
}

/// A floor in range, and no higher than the cap unless a listed id is as high.
fn floor_fits(floor: u32, max: u32, cap: u32, listed: &[Pair]) -> bool {
    let highest = listed.iter().map(|&(id, _)| id).max().unwrap_or(0);
    floor <= max && (floor <= cap || floor == highest)
}

/// Reads a section. `Ok(None)` is a version this build does not know.
pub(crate) fn decode(bytes: &[u8]) -> Result<Option<Ids>, FormatError> {
    let mut input = In {
        data: bytes,
        pos: 0,
    };
    match input.u32()? {
        0 => return Err(BAD),
        VERSION => {}
        _ => return Ok(None),
    }
    let (layer_floor, frame_floor) = (input.u32()?, input.u32()?);
    let layers = read_pairs(&mut input, MAX_LAYERS)?;
    let frames = read_pairs(&mut input, MAX_FRAMES)?;
    let layers_fit = floor_fits(layer_floor, PersistedLayerId::MAX, LAYER_FLOOR_CAP, &layers);
    let frames_fit = floor_fits(frame_floor, MAX_FRAME_ID, FRAME_FLOOR_CAP, &frames);
    let ids = |pairs: &[Pair]| -> Vec<u32> { pairs.iter().map(|&(id, _)| id).collect() };
    let listed_fit = fit_under(&ids(&layers), layer_floor) && fit_under(&ids(&frames), frame_floor);
    if !layers_fit || !frames_fit || !listed_fit {
        return Err(BAD);
    }
    Ok(Some(Ids {
        layer_floor,
        frame_floor,
        layers,
        frames,
        newer: input.pos != bytes.len(),
    }))
}

/// The floors alone, which every version keeps at the start, for a section that does not
/// parse or is newer than this build. `None` if even they do not.
fn decode_floors(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut input = In {
        data: bytes,
        pos: 0,
    };
    let version = input.u32().ok()?;
    let (layer_floor, frame_floor) = (input.u32().ok()?, input.u32().ok()?);
    let fit = version != 0 && layer_floor <= LAYER_FLOOR_CAP && frame_floor <= FRAME_FLOOR_CAP;
    fit.then_some((layer_floor, frame_floor))
}

/// Gives the document the ids and floors a section holds, after everything else is read.
pub(crate) fn apply_ids(doc: &mut Document, sections: &[Section], notes: &mut LoadNotes) {
    let mut found = sections.iter().filter(|(tag, _)| *tag == IDS);
    let Some((_, bytes)) = found.next() else {
        return;
    };
    // A file carries one section; a second is not guessed at.
    let decoded = match found.next() {
        None => decode(bytes),
        Some(_) => Err(BAD),
    };
    match decoded {
        Ok(Some(ids)) => take_on(doc, ids, notes),
        Ok(None) => {
            notes.newer_fields = true;
            take_floors(doc, bytes);
        }
        Err(_) => {
            notes.damaged.push(DAMAGED);
            take_floors(doc, bytes);
        }
    }
}

/// For a section whose lists cannot be used: if its floors can, no id given before is given
/// again, so every frame gets a new one above them. Layers keep the ids in their records.
fn take_floors(doc: &mut Document, bytes: &[u8]) {
    if let Some((layer_floor, frame_floor)) = decode_floors(bytes) {
        doc.raise_layer_floor(layer_floor);
        doc.raise_frame_floor(frame_floor);
        doc.renumber_frames();
    }
}

fn take_on(doc: &mut Document, ids: Ids, notes: &mut LoadNotes) {
    notes.newer_fields |= ids.newer;
    doc.raise_layer_floor(ids.layer_floor);
    doc.raise_frame_floor(ids.frame_floor);
    renumber_changed_layers(doc, &ids, notes);
    if ids.frames.is_empty() {
        // Written for frames whose ids were not usable: none of the ids given is kept.
        doc.renumber_frames();
        return;
    }
    let kept = matched_ids(&ids.frames, &frame_prints(doc));
    if kept.iter().any(Option::is_none) {
        notes.frame_ids_renumbered = true;
    }
    if doc.keep_some_frame_ids(&kept).is_err() {
        notes.damaged.push(DAMAGED);
        doc.renumber_frames();
    }
}

/// The saved id for each frame now in the document, matched by print: a print that is in the
/// list as often as among the frames gives its ids in order; any other frame gets none. So
/// frames an older build drew on, retimed or added get new ids, the others keep theirs wherever
/// they were moved, and frames that look and play alike keep theirs only while no such frame
/// was added or removed.
///
/// Found by sorting, through the shared sorts of `u128` and `u64`: the saved pairs by print
/// and then place, so each print's ids are a run in saved order, and the prints now.
pub(crate) fn matched_ids(saved: &[Pair], now: &[u64]) -> Vec<Option<u32>> {
    let mut by_print: Vec<u128> = (saved.iter().enumerate())
        .map(|(at, &(_, print))| u128::from(print) << 64 | at as u128)
        .collect();
    by_print.sort_unstable();
    let mut sorted_now = now.to_vec();
    sorted_now.sort_unstable();
    let print_of = |key: &u128| (key >> 64) as u64;
    // How many of each print's ids are given so far, at the start of its run.
    let mut taken = vec![0usize; by_print.len()];
    let mut id_for = |&print: &u64| {
        let start = by_print.partition_point(|k| print_of(k) < print);
        let end = by_print.partition_point(|k| print_of(k) <= print);
        let among_now = sorted_now.partition_point(|&p| p <= print)
            - sorted_now.partition_point(|&p| p < print);
        if start == end || end - start != among_now {
            return None;
        }
        let next = taken.get_mut(start)?;
        let key = by_print.get(start + *next)?;
        *next += 1;
        saved.get(*key as u64 as usize).map(|&(id, _)| id)
    };
    now.iter().map(&mut id_for).collect()
}

/// A layer that is listed keeps its id only if its check is unchanged; one that is not listed,
/// with an id at or below the saved floor, was numbered by a build that did not keep the floor.
/// Either way its id may have been a removed layer's, so it gets one above every id given.
fn renumber_changed_layers(doc: &mut Document, ids: &Ids, notes: &mut LoadNotes) {
    if ids.layers.is_empty() {
        return;
    }
    let checks = layer_checks(doc);
    for (index, check) in checks.into_iter().enumerate() {
        let Some(id) = doc.layers().get(index).map(|l| l.persisted_id().get()) else {
            continue;
        };
        let suspect = match ids.layers.iter().find(|&&(listed, _)| listed == id) {
            Some(&(_, saved)) => saved != check,
            None => id <= ids.layer_floor,
        };
        if suspect && doc.renumber_layer(index).is_ok() {
            notes.layer_ids_renumbered = true;
        }
    }
}

#[cfg(test)]
#[path = "piks_ids_tests.rs"]
mod tests;
