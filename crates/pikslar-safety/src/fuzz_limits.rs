//! The resource limits of the readers, reached on purpose. Damaged projects are opened with
//! exactly what they are charged left, with a byte less, and with a smaller room picked by the
//! case, so every charge of the decode limit (cels, image records, tables, tiles), the first
//! ones included, is met by the random cases. And files that claim the most a reader allows and
//! hold almost none of it are read each run: projects of every version with the most cels on
//! the largest canvas, read with less of the limit left than their cels are charged, which must
//! be refused for their size before anything is made; an Aseprite file with the most layers over
//! the most frames, whose tables of frames are charged as they are made; and an Aseprite file of
//! the most tilemap layers over one large tileset, looked at once for its blending. Each must
//! end quickly, holding no more than its limit.

use crate::craft;
use crate::fuzz_tiles::ase_build::{plain, tilemap_layer, tileset_chunk, Ase};
use crate::tracking::{measure, Usage};
use pikslar_format::{import_ase, piks, FormatError};
use std::time::{Duration, Instant};

const MEGABYTE: usize = 1 << 20;
/// Most a claimed project may hold while it is refused. Refused at its first charge, it holds
/// almost nothing (under 0.1 MB is measured); its layers' records alone would be about 4 MB.
pub const CLAIM_PEAK_LIMIT: usize = 32 * MEGABYTE;
/// Most the claimed Aseprite file may hold: 256 layers' tables of 4,096 frames (about 21 MB
/// is measured), and the document made of them.
pub const ASE_CLAIM_PEAK_LIMIT: usize = 48 * MEGABYTE;
/// Most the claimed Aseprite file of tilemap layers may hold: its tileset of 4 million pixels
/// (16 MB) unpacked and kept, and the document made of it (about 29 MB is measured).
pub const ASE_TILES_CLAIM_PEAK_LIMIT: usize = 64 * MEGABYTE;
/// Slowest a claimed file may be refused.
pub const CLAIM_TIME_LIMIT: Duration = Duration::from_secs(5);

/// One in this many accepted projects is opened again at its charge and below it.
const CHARGE_ONE_IN: u64 = 4;

/// A project that opened must open again with exactly its charge left and be refused for its
/// size with a byte less, and with the smaller room [`room_below`] picks for `case`: the charge
/// is the one the limit holds it to, and it is checked as it is made, at every place it is
/// made. Returns whether the check was made.
pub fn charge_holds(bytes: &[u8], case: u64) -> Result<bool, String> {
    if case % CHARGE_ONE_IN != 0 {
        return Ok(false);
    }
    let charge = piks::charges(bytes).map_err(|e| format!("an opened project: {e}"))?;
    if let Err(e) = piks::load_with_room(bytes, charge) {
        return Err(format!("refused with its charge of {charge} left: {e}"));
    }
    if charge == 0 {
        return Ok(true);
    }
    for room in [charge - 1, room_below(charge, case)] {
        match piks::load_with_room(bytes, room) {
            Err(FormatError::FileTooLarge) => {}
            other => {
                return Err(format!(
                    "with {room} left of its charge of {charge}: {:?}",
                    other.map(|_| ())
                ))
            }
        }
    }
    Ok(true)
}

/// A room below a `charge` above 0, picked by `case` on every scale from nothing up, so that
/// over the cases a read is refused at each place it is charged, the first and smallest too.
pub fn room_below(charge: usize, case: u64) -> usize {
    let mixed = (case / CHARGE_ONE_IN)
        .wrapping_add(1)
        .wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let scale = charge >> (mixed % u64::from(usize::BITS)) as u32;
    (mixed >> 8) as usize % scale.max(1)
}

/// What reading one claimed file did.
pub struct Claim {
    pub name: &'static str,
    pub file_bytes: usize,
    pub usage: Usage,
    pub took: Duration,
}

/// How a claimed file must end.
#[derive(Clone, Copy)]
enum Ending {
    /// Opened.
    Opens,
    /// Opened, or refused for any reason.
    Either,
    /// Refused, with the error this says is the right one.
    Refused(fn(&FormatError) -> bool),
}

fn held_to(
    name: &'static str,
    bytes: &[u8],
    (limit, ending): (usize, Ending),
    read: impl FnOnce(&[u8]) -> Result<(), FormatError>,
) -> Result<Claim, String> {
    let started = Instant::now();
    let (result, usage) = measure(|| read(bytes));
    let took = started.elapsed();
    match (ending, &result) {
        (Ending::Opens, Err(e)) => return Err(format!("the claimed {name} file was refused: {e}")),
        (Ending::Refused(_), Ok(())) => {
            return Err(format!("the claimed {name} file was accepted"))
        }
        (Ending::Refused(right), Err(e)) if !right(e) => {
            return Err(format!(
                "the claimed {name} file was refused otherwise: {e:?}"
            ))
        }
        _ => {}
    }
    if usage.peak > limit {
        return Err(format!("the claimed {name} file held {} bytes", usage.peak));
    }
    if took > CLAIM_TIME_LIMIT {
        return Err(format!("the claimed {name} file took {took:?}"));
    }
    Ok(Claim {
        name,
        file_bytes: bytes.len(),
        usage,
        took,
    })
}

/// The names of [`claims`], in order.
pub const CLAIM_NAMES: [&str; 5] = [
    "v1 project",
    "v2 project",
    "v3 project",
    "aseprite",
    "aseprite tilemaps",
];

/// Reads each claimed file against its limits.
pub fn claims() -> Result<Vec<Claim>, String> {
    let mut out = Vec::new();
    for (version, name) in (1u32..=3).zip(CLAIM_NAMES) {
        out.push(project_claim(version, name, (256, 4096))?);
    }
    let read = |b: &[u8]| import_ase(b).map(|_| ());
    let ase = claimed_ase(4096, 256);
    let limit = (ASE_CLAIM_PEAK_LIMIT, Ending::Either);
    out.push(held_to(CLAIM_NAMES[3], &ase, limit, read)?);
    let tiles = claimed_ase_tilemaps(255, 4096);
    let limit = (ASE_TILES_CLAIM_PEAK_LIMIT, Ending::Opens);
    out.push(held_to(CLAIM_NAMES[4], &tiles, limit, read)?);
    Ok(out)
}

/// What a project of `layers` layers of `frames` frames is charged for its cels, before any
/// is read.
fn cel_charge(layers: u32, frames: u32) -> usize {
    layers as usize * frames as usize * piks::CEL_CHARGE
}

/// A claimed project of `version` read with a byte less of the decode limit left than its cels
/// are charged, as if what came before had spent the rest: that first charge takes it past the
/// 2 GiB limit, so it must be refused for its size before anything is made. With exactly the
/// charge left it must get past it and be refused as cut short, so the claim is met by the
/// limit and not by the missing images.
fn project_claim(
    version: u32,
    name: &'static str,
    (layers, frames): (u32, u32),
) -> Result<Claim, String> {
    let bytes = claimed_project(version, layers, frames, layers * frames);
    let cels = cel_charge(layers, frames);
    let too_large = |e: &FormatError| matches!(e, FormatError::FileTooLarge);
    let over = |b: &[u8]| piks::load_with_room(b, cels - 1).map(|_| ());
    let claim = held_to(
        name,
        &bytes,
        (CLAIM_PEAK_LIMIT, Ending::Refused(too_large)),
        over,
    )?;
    match piks::load_with_room(&bytes, cels) {
        Err(FormatError::Corrupt(_)) => Ok(claim),
        other => Err(format!(
            "the claimed {name} file with its cels' charge left: {:?}",
            other.map(|_| ())
        )),
    }
}

fn u16s(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32s(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// A sealed project of `version` on the largest canvas with `layers` layers of `frames` frames
/// and `images` images claimed, every layer record whole (version 1 and 2 layers list their
/// cels) and nothing after the image count.
pub fn claimed_project(version: u32, layers: u32, frames: u32, images: u32) -> Vec<u8> {
    let mut out = b"PIKS".to_vec();
    u32s(&mut out, version);
    if version == 3 {
        u32s(&mut out, 0);
    }
    let side = pikslar_core::MAX_DIMENSION as u32;
    // The canvas, a list of one colour, an empty list of swatches, then the frames' lengths.
    [side, side, 1, 0, 0]
        .iter()
        .for_each(|&v| u32s(&mut out, v));
    u32s(&mut out, frames);
    (0..frames).for_each(|_| u32s(&mut out, 100));
    u32s(&mut out, layers);
    for n in 0..layers {
        match version {
            3 => {
                let mut record = Vec::new();
                u32s(&mut record, n + 1);
                record.extend_from_slice(&[1, 255, 0, 1]);
                u16s(&mut record, 0);
                u16s(&mut out, record.len() as u16);
                out.extend_from_slice(&record);
            }
            _ => {
                u16s(&mut out, 0);
                out.extend_from_slice(&[1, 255, 0, 1]);
                (0..frames).for_each(|_| u32s(&mut out, 0));
            }
        }
    }
    u32s(&mut out, images);
    u32s(&mut out, 0);
    craft::resealed(out)
}

/// An Aseprite file of `frames` empty frames on a small canvas, whose first frame names
/// `layers` layers: each layer is given a place in every frame as it is read, charged to the
/// file's budget.
pub fn claimed_ase(frames: u16, layers: usize) -> Vec<u8> {
    let first = (0..layers).map(|n| plain(&format!("L{n}"))).collect();
    let mut ase = Ase::new(16, 16, 32).frame(100, first);
    for _ in 1..frames {
        ase = ase.frame(100, Vec::new());
    }
    ase.bytes()
}

/// An Aseprite file of one plain layer and `layers` tilemap layers over one tileset of `tiles`
/// opaque 32 by 32 tiles that differ, so that choosing its blending looks at every pixel.
pub fn claimed_ase_tilemaps(layers: usize, tiles: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(tiles as usize * 1024 * 4);
    for t in 0..tiles {
        for p in 0..1024u32 {
            pixels.extend_from_slice(&[(p + t) as u8, (p * 7) as u8, (t >> 8) as u8, 255]);
        }
    }
    let mut chunks = vec![plain("art")];
    chunks.extend((0..layers).map(|n| tilemap_layer(0, 0, &format!("map {n}"), 0)));
    chunks.push(tileset_chunk(0, 0, tiles, (32, 32), "opaque", &pixels));
    Ase::new(16, 16, 32).frame(100, chunks).bytes()
}

/// A stream that inflated to `whole` is inflated again with a stop that answers true from its
/// `after`th question on: it must end stopped, or, if it asked fewer times, as it did before.
pub fn stop_holds(bytes: &[u8], limit: usize, whole: &[u8], after: usize) -> Result<(), String> {
    let asked = std::cell::Cell::new(0usize);
    let stop = || {
        asked.set(asked.get() + 1);
        asked.get() > after
    };
    match pikslar_format::inflate::inflate_until(bytes, limit, &stop) {
        Err(FormatError::Stopped) if asked.get() > after => Ok(()),
        Ok((out, _)) if asked.get() <= after && out == whole => Ok(()),
        other => Err(format!(
            "stopped after {after} questions, asked {}: {:?}",
            asked.get(),
            other.map(|(out, _)| out.len())
        )),
    }
}

#[cfg(test)]
#[path = "fuzz_limits_tests.rs"]
mod tests;
