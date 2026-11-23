//! Cel chunks: the drawings of an Aseprite file, from raw, zlib-packed or linked cels.
//! Written from the public file specification only; see [`crate::ase_read`].

use crate::ase_chunks::{Bytes, Depth, Kind, Sprite};
use crate::ase_read::AseWarning;
use crate::inflate::inflate_until;
use crate::FormatError;
use pikslar_core::tile::{RawTile, Tile, TILE, TILE_AREA};
use pikslar_core::{rgba, Cel, Grid, Image, MAX_DIMENSION};
use std::sync::Arc;

/// The longest side of a drawing: what an image reaches from its corner.
pub(crate) const MAX_SIDE: usize = pikslar_core::MAX_TILE_REACH as usize * TILE;
/// The most pixels one drawing may have.
pub(crate) const MAX_PIXELS: u64 = (MAX_DIMENSION as u64) * (MAX_DIMENSION as u64);
/// The bytes one slot of an image's table takes.
pub(crate) const SLOT_BYTES: usize = std::mem::size_of::<Option<Arc<Tile>>>();

const RAW: u16 = 0;
const LINKED: u16 = 1;
const COMPRESSED: u16 = 2;
const TILEMAP: u16 = 3;

/// What one file may ask of the machine, counted before it is asked.
pub(crate) struct Budget {
    /// Bytes of pixels all the cels may unpack in all: this bounds the time.
    pub work: u64,
    /// Bytes of memory the file may hold at once: the drawings kept so far, and what the cel
    /// being read needs besides. This bounds the memory.
    pub held: u64,
}

impl Budget {
    /// A cel of `raw` bytes is about to be unpacked. Unpacking grows a buffer to that size by
    /// doubling, so the buffer and its last copy need half as much again.
    pub(crate) fn unpacking(&mut self, raw: u64) -> Result<(), FormatError> {
        self.work = self
            .work
            .checked_sub(raw)
            .ok_or(FormatError::FileTooLarge)?;
        if raw.saturating_add(raw / 2) > self.held {
            return Err(FormatError::FileTooLarge);
        }
        Ok(())
    }
}

/// What one cel chunk says before its pixels.
struct CelHead {
    layer: usize,
    x: i16,
    y: i16,
    opacity: u8,
    kind: u16,
    z: i16,
}

fn head(r: &mut Bytes<'_>) -> Result<CelHead, FormatError> {
    let layer = usize::from(r.u16()?);
    let (x, y, opacity) = (r.i16()?, r.i16()?, r.u8()?);
    let kind = r.u16()?;
    let z = r.i16()?;
    r.skip(5)?;
    Ok(CelHead {
        layer,
        x,
        y,
        opacity,
        kind,
        z,
    })
}

/// A cel's size and its pixels as the file stores them, after the checks that keep them in
/// bounds. The pixels are `None` when the stream's checksum says they are damaged.
type Unpacked = (usize, usize, Option<Vec<u8>>);

fn pixels(
    r: &mut Bytes<'_>,
    kind: u16,
    depth: Depth,
    budget: &mut Budget,
    stop: &dyn Fn() -> bool,
) -> Result<Unpacked, FormatError> {
    let (w, h) = (usize::from(r.u16()?), usize::from(r.u16()?));
    if w > MAX_SIDE || h > MAX_SIDE || (w * h) as u64 > MAX_PIXELS {
        return Err(FormatError::TooLarge);
    }
    let expected = (w * h) as u64 * depth.bytes();
    budget.unpacking(expected)?;
    let expected = expected as usize;
    let raw = if kind == RAW {
        Some(r.take(expected)?.to_vec())
    } else {
        unpack(r.0, expected, stop)?
    };
    Ok((w, h, raw))
}

/// A zlib stream (RFC 1950) of exactly `expected` bytes, or `None` if its checksum is wrong or
/// missing. `stop` is asked as the stream is unpacked (see [`inflate_until`]).
pub(crate) fn unpack(
    stream: &[u8],
    expected: usize,
    stop: &dyn Fn() -> bool,
) -> Result<Option<Vec<u8>>, FormatError> {
    const BAD: FormatError = FormatError::Corrupt("a cel's pixels are not a zlib stream");
    let (cmf, flg) = match stream {
        [cmf, flg, ..] => (*cmf, *flg),
        _ => return Err(BAD),
    };
    // Deflate with a window of at most 32 KiB, a valid check, and no preset dictionary.
    if cmf & 15 != 8 || cmf >> 4 > 7 || (u32::from(cmf) * 256 + u32::from(flg)) % 31 != 0 {
        return Err(BAD);
    }
    if flg & 0x20 != 0 {
        return Err(BAD);
    }
    let deflated = stream.get(2..).ok_or(BAD)?;
    let (raw, used) = inflate_until(deflated, expected, stop)?;
    if raw.len() != expected {
        return Err(FormatError::Corrupt("a cel's pixels are the wrong size"));
    }
    let trailer = deflated.get(used..used.saturating_add(4));
    Ok((trailer == Some(&adler32(&raw).to_be_bytes()[..])).then_some(raw))
}

/// The Adler-32 checksum of RFC 1950.
fn adler32(data: &[u8]) -> u32 {
    // The most bytes that can be summed before the sums could pass 32 bits.
    const RUN: usize = 5552;
    const MOD: u32 = 65_521;
    let (mut a, mut b) = (1u32, 0u32);
    for run in data.chunks(RUN) {
        for &byte in run {
            a += u32::from(byte);
            b += a;
        }
        (a, b) = (a % MOD, b % MOD);
    }
    b << 16 | a
}

/// Aseprite's 8-bit multiply: `a * b / 255`, rounded.
fn mul_un8(a: u8, b: u8) -> u8 {
    let t = u32::from(a) * u32::from(b) + 0x80;
    (((t >> 8) + t) >> 8) as u8
}

/// Multiplies every alpha of `raw`, the pixels of a gray or RGBA cel, by the cel's `opacity`.
fn fade(raw: &mut [u8], depth: Depth, opacity: u8) {
    let step = depth.bytes() as usize;
    for pixel in raw.chunks_exact_mut(step) {
        if let Some(alpha) = pixel.last_mut() {
            *alpha = mul_un8(*alpha, opacity);
        }
    }
}

/// One cell of a pixel. Grayscale pixels become RGBA ones. Indexed pixels become the color
/// table's place for the index, which is one past it because place 0 is clear; the transparent
/// index becomes clear itself.
fn cell(raw: &[u8], at: usize, depth: Depth, clear: Option<u8>) -> u32 {
    match depth {
        Depth::Indexed => match raw.get(at) {
            Some(&i) if Some(i) != clear => u32::from(i) + 1,
            _ => 0,
        },
        // A grey pixel is a value and an alpha: the same value in all three colors.
        Depth::Gray => match raw.get(at * 2..at * 2 + 2) {
            Some(&[v, a]) => rgba(v, v, v, a),
            _ => 0,
        },
        Depth::Rgba => match raw.get(at * 4..at * 4 + 4) {
            Some(&[r, g, b, a]) => rgba(r, g, b, a),
            _ => 0,
        },
    }
}

/// How many tiles of a picture are made between asking whether to stop: a mebipixel.
const STOP_EVERY_TILES: usize = 4096;

/// The picture of `w` by `h` pixels, a tile at a time. What it keeps (its table and the tiles
/// that are not blank) is taken from `room` as it is made, and the file is refused when `room`
/// runs out, before more is made than `room` allows. `stop` is asked every
/// [`STOP_EVERY_TILES`] tiles.
pub(crate) fn image_of(
    size: (usize, usize),
    raw: &[u8],
    (depth, clear): (Depth, Option<u8>),
    room: &mut u64,
    stop: &dyn Fn() -> bool,
) -> Result<Image, FormatError> {
    image_from(size, |at| cell(raw, at, depth, clear), room, stop)
}

/// As [`image_of`], with the value of each cell, by its number in reading order, given by
/// `value`.
pub(crate) fn image_from(
    (w, h): (usize, usize),
    mut value: impl FnMut(usize) -> u32,
    room: &mut u64,
    stop: &dyn Fn() -> bool,
) -> Result<Image, FormatError> {
    if w == 0 || h == 0 {
        return Ok(Image::blank());
    }
    let mut grid = Grid::new(w, h);
    let slots = grid.tiles_wide() * h.div_ceil(TILE);
    take_room(room, slots.saturating_mul(SLOT_BYTES))?;
    let mut tile: RawTile = [0; TILE_AREA];
    for index in 0..slots {
        if index % STOP_EVERY_TILES == STOP_EVERY_TILES - 1 && stop() {
            return Err(FormatError::Stopped);
        }
        let (x0, y0) = (
            index % grid.tiles_wide() * TILE,
            index / grid.tiles_wide() * TILE,
        );
        tile.fill(0);
        for y in 0..TILE.min(h - y0) {
            for x in 0..TILE.min(w - x0) {
                tile[y * TILE + x] = value((y0 + y) * w + x0 + x);
            }
        }
        let packed = Tile::pack(&tile);
        take_room(room, packed.as_ref().map_or(0, Tile::heap_size))?;
        grid.replace(index, packed.map(Arc::new));
    }
    Ok(grid.into_image())
}

pub(crate) fn take_room(room: &mut u64, bytes: usize) -> Result<(), FormatError> {
    *room = room
        .checked_sub(bytes as u64)
        .ok_or(FormatError::FileTooLarge)?;
    Ok(())
}

impl Sprite<'_> {
    /// A cel chunk (0x2005).
    pub fn cel_chunk(&mut self, frame: usize, data: &[u8]) -> Result<(), FormatError> {
        let mut r = Bytes(data);
        let cel = head(&mut r)?;
        let layer = self
            .layers
            .get(cel.layer)
            .ok_or(FormatError::Corrupt("a cel on a layer that does not exist"))?;
        let name = layer.name.clone();
        let layer_kind = layer.kind;
        if !matches!(layer_kind, Kind::Image | Kind::Tilemap) {
            let what = "cels on layers that are not brought in";
            self.warnings.add(AseWarning::Skipped(what));
            return Ok(());
        }
        if cel.z != 0 {
            self.warnings.add(AseWarning::CelZIndex(name.clone()));
        }
        let map = layer_kind == Kind::Tilemap;
        let made = self.cel_of(&cel, (frame, &name, map), &mut r)?;
        match made {
            Some(made) => {
                self.note_opacity(cel.layer, frame, cel.opacity);
                self.place(cel.layer, frame, made)
            }
            None if cel.kind == LINKED => {
                self.warnings.add(AseWarning::UnresolvedLink(name));
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// The cel a cel chunk makes on a layer of its kind (`map` for a tilemap layer), or none
    /// for one that cannot show there, with a warning.
    fn cel_of(
        &mut self,
        cel: &CelHead,
        (frame, name, map): (usize, &str, bool),
        r: &mut Bytes<'_>,
    ) -> Result<Option<Cel>, FormatError> {
        Ok(match cel.kind {
            LINKED => self.linked(cel, frame, name, r)?,
            RAW | COMPRESSED if !map => Some(self.drawn(cel, frame, name, r)?),
            TILEMAP if map => {
                Some(self.tilemap_cel(cel.layer, frame, (cel.x, cel.y, cel.opacity), r)?)
            }
            RAW | COMPRESSED | TILEMAP => {
                // A drawing on a map layer, or a map on a drawing layer: neither can show.
                let what = if map {
                    "drawings on tilemap layers"
                } else {
                    "tilemap cels"
                };
                self.warnings.add(AseWarning::Skipped(what));
                None
            }
            _ => {
                self.warnings
                    .add(AseWarning::Skipped("cels of an unknown type"));
                None
            }
        })
    }

    fn drawn(
        &mut self,
        cel: &CelHead,
        frame: usize,
        name: &str,
        r: &mut Bytes<'_>,
    ) -> Result<Cel, FormatError> {
        let (w, h, raw) = pixels(r, cel.kind, self.depth, &mut self.budget, self.stop)?;
        if (self.stop)() {
            return Err(FormatError::Stopped);
        }
        let Some(raw) = raw else {
            let (layer, frame) = (name.to_owned(), frame);
            self.warnings.add(AseWarning::Damaged { layer, frame });
            return Ok(Cel::blank());
        };
        let mut raw = raw;
        if cel.opacity != u8::MAX {
            if self.depth == Depth::Indexed {
                // A drawing of palette places has no alpha of its own to take it from.
                self.warnings.add(AseWarning::CelOpacity(name.to_owned()));
            } else {
                fade(&mut raw, self.depth, cel.opacity);
            }
        }
        let background = self.layers.get(cel.layer).is_some_and(|l| l.background);
        let clear = (!background).then_some(self.transparent);
        if self.depth == Depth::Indexed {
            let highest = raw.iter().copied().filter(|&i| Some(i) != clear).max();
            self.highest = self.highest.max(highest);
        }
        // The pixels are held while the drawing is made, so they come out of the room.
        let before = self.budget.held.saturating_sub(raw.len() as u64);
        let mut room = before;
        let image = image_of((w, h), &raw, (self.depth, clear), &mut room, self.stop)?;
        self.budget.held = self.budget.held.saturating_sub(before - room);
        Cel::at(image, cel.x.into(), cel.y.into())
            .ok_or(FormatError::Corrupt("a cel sits too far from the canvas"))
    }

    /// A linked cel shows the drawing of an earlier frame, and joins its hold.
    fn linked(
        &mut self,
        cel: &CelHead,
        frame: usize,
        name: &str,
        r: &mut Bytes<'_>,
    ) -> Result<Option<Cel>, FormatError> {
        let target = usize::from(r.u16()?);
        let layer = self.layers.get_mut(cel.layer);
        let Some(layer) = layer.filter(|_| target < frame) else {
            return Ok(None);
        };
        let Some(Some(source)) = layer.cels.get(target) else {
            return Ok(None);
        };
        let source = source.clone();
        // A hold is one drawing, so a link shows its source's opacity, not one of its own.
        let source_opacity = layer.opacities.get(target).copied();
        if source_opacity.is_some_and(|o| o != cel.opacity) {
            self.warnings.add(AseWarning::CelOpacity(name.to_owned()));
        }
        let hold = match layer.holds.get(target) {
            Some(&id) if id != 0 => id,
            _ => target as u32 + 1,
        };
        for at in [target, frame] {
            if let Some(slot) = layer.holds.get_mut(at) {
                *slot = hold;
            }
        }
        Ok(Some(source))
    }

    /// Remembers the opacity a cel was stored with, for the cels that link to it.
    fn note_opacity(&mut self, layer: usize, frame: usize, opacity: u8) {
        let slot = self
            .layers
            .get_mut(layer)
            .and_then(|l| l.opacities.get_mut(frame));
        if let Some(slot) = slot {
            *slot = opacity;
        }
    }

    fn place(&mut self, layer: usize, frame: usize, cel: Cel) -> Result<(), FormatError> {
        let slot = self
            .layers
            .get_mut(layer)
            .and_then(|l| l.cels.get_mut(frame))
            .ok_or(FormatError::Corrupt("a cel on a frame that does not exist"))?;
        if slot.is_some() {
            return Err(FormatError::Corrupt("two cels on one layer and frame"));
        }
        *slot = Some(cel);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(work: u64, held: u64) -> Budget {
        Budget { work, held }
    }

    #[test]
    fn the_pixels_all_the_cels_ask_for_are_limited() {
        let mut budget = budget(40, u64::MAX);
        let data = [0u8; 64];
        let size = |w: u8| [w, 0, 2, 0];
        let mut ask = |w: u8| {
            let mut bytes = size(w).to_vec();
            bytes.extend_from_slice(&data);
            pixels(&mut Bytes(&bytes), RAW, Depth::Rgba, &mut budget, &|| false)
                .map(|p| p.2.map(|r| r.len()))
        };
        // 2 by 2 pixels of 4 bytes is 16, and the budget is 40: two fit and the third does not.
        assert_eq!(ask(2).ok(), Some(Some(16)));
        assert_eq!(ask(2).ok(), Some(Some(16)));
        assert!(matches!(ask(2), Err(FormatError::FileTooLarge)));
    }

    #[test]
    fn a_cel_that_cannot_be_unpacked_in_the_memory_left_is_refused() {
        // 16 bytes of pixels need 24 with the room an unpacking buffer takes.
        assert!(budget(1000, 24).unpacking(16).is_ok());
        assert!(matches!(
            budget(1000, 23).unpacking(16),
            Err(FormatError::FileTooLarge)
        ));
    }

    fn noise(len: usize) -> Vec<u8> {
        (0..len).map(|i| ((i * 7919) ^ (i >> 3)) as u8).collect()
    }

    #[test]
    fn the_tiles_kept_are_charged_as_they_are_made() {
        let raw = noise(128 * 128);
        let mut room = u64::MAX;
        let image = image_of((128, 128), &raw, (Depth::Indexed, None), &mut room, &|| {
            false
        })
        .expect("fits");
        assert_eq!(u64::MAX - room, image.heap_size() as u64);
        assert!(image.heap_size() > 128 * 128, "noise is not packed small");
        // Room for the table and one tile is not room for four.
        let mut room = (SLOT_BYTES * 4 + 64 * 64 * 2) as u64;
        let refused = image_of((128, 128), &raw, (Depth::Indexed, None), &mut room, &|| {
            false
        });
        assert!(matches!(refused, Err(FormatError::FileTooLarge)));
    }

    #[test]
    fn a_blank_drawing_is_charged_only_for_the_table_it_was_made_with() {
        let raw = vec![0u8; 128 * 128];
        let mut room = u64::MAX;
        let image = image_of(
            (128, 128),
            &raw,
            (Depth::Indexed, Some(0)),
            &mut room,
            &|| false,
        )
        .expect("fits");
        let table = (128 / TILE) * (128 / TILE) * SLOT_BYTES;
        assert_eq!(u64::MAX - room, table as u64);
        // Nothing is kept of a blank drawing, but its table was made on the way.
        assert_eq!(image.heap_size(), 0);
    }
}
