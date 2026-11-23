//! Slice chunks (0x2022): named rectangles, brought into the document's own slices. Written from
//! the public file specification only; see [`crate::ase_read`].
//!
//! A slice in a file has a key per frame it changes in. A Pikslar slice is the same on every
//! frame, so one key stands for it: the earliest whose rectangle is not empty (the one in effect
//! at frame 0, when that is it). A slice whose keys differ, or that starts later than frame 1, is
//! told as such, and one whose keys are all empty is left out.

use crate::ase_chunks::{Bytes, Sprite};
use crate::ase_names::{tidy, Name};
use crate::ase_read::{AseWarning, Tally};
use crate::FormatError;
use pikslar_core::{Slice, SliceRect, MAX_SLICES, MAX_SLICE_NAME};
use std::borrow::Cow;

const NINE_PATCH: u32 = 1;
const HAS_PIVOT: u32 = 2;
/// The outline color of a slice that comes in: the one a slice made here starts with.
const SLICE_COLOR: u32 = 0xff8c_00ff;

/// What one key of a slice says.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    frame: u32,
    bounds: SliceRect,
    center: Option<SliceRect>,
    pivot: Option<(i32, i32)>,
}

fn rect(r: &mut Bytes<'_>) -> Result<SliceRect, FormatError> {
    let (x, y) = (r.i32()?, r.i32()?);
    let (width, height) = (r.u32()?, r.u32()?);
    Ok(SliceRect {
        x,
        y,
        width,
        height,
    })
}

fn key(r: &mut Bytes<'_>, flags: u32) -> Result<Key, FormatError> {
    let frame = r.u32()?;
    let bounds = rect(r)?;
    let center = if flags & NINE_PATCH != 0 {
        Some(rect(r)?)
    } else {
        None
    };
    let pivot = if flags & HAS_PIVOT != 0 {
        Some((r.i32()?, r.i32()?))
    } else {
        None
    };
    Ok(Key {
        frame,
        bounds,
        center,
        pivot,
    })
}

/// The slice as the document may keep it, with whatever part of it does not fit left off:
/// the centre, the pivot, or both. `None` when not even the rectangle fits. The flag is whether
/// anything was left off.
fn fitted(name: &str, key: &Key) -> Option<(Slice, bool)> {
    let tries = [
        (key.center, key.pivot),
        (None, key.pivot),
        (key.center, None),
        (None, None),
    ];
    tries.iter().find_map(|&(center, pivot)| {
        let slice = Slice {
            name: name.to_owned(),
            color: SLICE_COLOR,
            bounds: key.bounds,
            pivot,
            center,
        };
        // Leaving off what was not there to begin with is not leaving anything off.
        let lost = (center != key.center) | (pivot != key.pivot);
        slice.checked().ok().map(|s| (s, lost))
    })
}

/// What the keys of one slice chunk say, as far as they were read.
#[derive(Default)]
struct Keys {
    /// The key that stands for the slice: the earliest whose rectangle is not empty.
    chosen: Option<Key>,
    /// The first key read, to compare the rest with.
    seen: Option<Key>,
    /// Whether any key differs from the first.
    changes: bool,
}

impl Keys {
    fn add(&mut self, next: Key) {
        let held = *self.seen.get_or_insert(next);
        self.changes |=
            (next.bounds, next.center, next.pivot) != (held.bounds, held.center, held.pivot);
        let usable = next.bounds.width != 0 && next.bounds.height != 0;
        if usable && self.chosen.is_none_or(|c| next.frame < c.frame) {
            self.chosen = Some(next);
        }
    }
}

/// The count, flags and name at the start of a slice chunk.
fn head<'a>(r: &mut Bytes<'a>) -> Result<(u32, u32, Cow<'a, str>), FormatError> {
    let (count, flags) = (r.u32()?, r.u32()?);
    r.skip(4)?;
    Ok((count, flags, r.text()?))
}

impl Sprite<'_> {
    /// A slice chunk (0x2022). A chunk that ends early keeps the keys it has, or, with none, the
    /// slice is left out: the chunk's own length puts the next chunk where it is, so a bad slice
    /// never costs the rest of the file, as in a project file.
    pub fn slice_chunk(&mut self, data: &[u8]) {
        let mut r = Bytes(data);
        let Ok((count, flags, raw)) = head(&mut r) else {
            self.warnings.tally(Tally::SliceDropped, "", 0);
            return;
        };
        let name = tidy(raw, MAX_SLICE_NAME);
        let mut keys = Keys::default();
        let mut cut = false;
        // Each key takes bytes of the chunk, so a count past what the chunk holds runs out of
        // them here, with nothing kept but the keys read.
        for _ in 0..count {
            match key(&mut r, flags) {
                Ok(next) => keys.add(next),
                Err(_) => {
                    cut = true;
                    break;
                }
            }
        }
        self.keep_slice(name, &keys, cut, keys.seen.is_some());
    }

    fn keep_slice(&mut self, name: Name, keys: &Keys, cut: bool, any: bool) {
        let cut_name = name.cut;
        let shown = self.note_name(name);
        let fit = keys.chosen.filter(|_| self.slices.len() < MAX_SLICES);
        let kept = if shown.is_empty() {
            self.default_name()
        } else {
            shown.clone()
        };
        let Some((slice, lost, key)) = fit.and_then(|k| fitted(&kept, &k).map(|(s, l)| (s, l, k)))
        else {
            self.warnings.tally(Tally::SliceDropped, &shown, 0);
            if cut && any {
                self.warnings.tally(Tally::SliceCut, &shown, 0);
            }
            return;
        };
        let frame = key.frame as usize + 1;
        let words = [
            (Tally::SliceName, cut_name, 0),
            (Tally::SliceParts, lost, 0),
            (Tally::SliceKeys, keys.changes, frame),
            (Tally::SliceLate, key.frame > 0, frame),
            (Tally::SliceCut, cut, 0),
        ];
        for (tally, applies, at) in words {
            if applies {
                self.warnings.tally(tally, &kept, at);
            }
        }
        self.warnings
            .add(AseWarning::Skipped("slice colors and user data"));
        self.slices.push(slice);
    }

    /// The first "Slice N" no slice is called, as the app names a slice it makes.
    fn default_name(&self) -> String {
        (1..)
            .map(|n| format!("Slice {n}"))
            .find(|name| self.slices.iter().all(|s| &s.name != name))
            .unwrap_or_default()
    }
}
