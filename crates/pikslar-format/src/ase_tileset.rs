//! Tilesets and tilemap cels of an `.ase` file, read from the public specification only (M10).
//!
//! A tileset chunk (0x2023) holds the tiles as one picture, `tile width` by `tile height x number
//! of tiles`, zlib-packed, or names a tileset in another file, which is not read. A tilemap layer
//! (layer type 2) names a tileset by the number the file gives it, and its cels (cel type 3) are
//! grids of tile numbers with flip flags, whose bit masks the cel itself carries.
//!
//! **Numbers.** Where the file says tile 0 is the empty tile (flag 4 of the tileset, which every
//! recent file sets) the tiles from 1 on become tiles 1, 2, 3 and so on of a document tileset, in
//! that order, so a tile keeps its number; tile 0 is not brought in, whatever it holds. Where the
//! flag is off the file's tile 0 is a tile like any other and the empty cell is all ones, so the
//! tiles shift up by one. **Flips.** The cel's three masks (x, y, diagonal) become the three
//! flags of an [`Orientation`]; see [`orientation`].
//!
//! Everything is untrusted: counts and sizes are checked before anything is made for them, and
//! every picture is charged to the importer's budget as it is made.

use crate::ase_cel::{image_from, image_of, take_room, unpack, MAX_PIXELS, MAX_SIDE};
use crate::ase_chunks::{Bytes, Depth, Kind, Sprite};
use crate::ase_names::tidy;
use crate::ase_read::{AseWarning, Warnings};
use crate::FormatError;
use pikslar_core::orient::Orientation;
use pikslar_core::tilemap::MapCell;
use pikslar_core::tileset::MAX_TILESET_NAME;
use pikslar_core::tileset::{TileId, TilesetId, MAX_TILES, MAX_TILESETS, MAX_TILE_SIDE};
use pikslar_core::{Cel, ColorMode, Document, Image, LimitError};
use std::sync::Arc;

/// Tileset flag: the tileset is a file of its own, named after the flags.
const EXTERNAL: u32 = 1;
/// Tileset flag: the tiles are in this file.
const INSIDE: u32 = 2;
/// Tileset flag: tile 0 is the empty tile, and the empty cell of a map is 0.
const EMPTY_IS_ZERO: u32 = 4;
/// What a tile takes besides its pixels: its entry, its image's place and the allowance for the
/// allocator, charged to the file's budget so that many tiny tiles are not free.
const TILE_OVERHEAD: u64 = 128;
/// The most tileset chunks of one file that are kept, usable or not: what a document holds and
/// a few more, so that a file of thousands is read in time linear in its length. Those past it
/// are left out with a warning.
pub(crate) const MAX_KEPT_TILESETS: usize = MAX_TILESETS + 16;

/// How many tile pictures of a tileset are made between asking whether to stop. A picture is at
/// most 256 by 256 pixels, 256 tiles of the grid, so this is at most 4,096 of them, as in a cel.
const STOP_EVERY_PICTURES: usize = 16;

/// One tileset of the file, with its tiles already made.
pub(crate) struct AseTileset {
    /// The number the file gives the tileset, which layers name.
    pub id: u32,
    pub name: String,
    pub size: (usize, usize),
    /// The tile numbers of the file start at 0 with the empty tile.
    empty_is_zero: bool,
    /// The pictures, in order; picture `i` is the tile numbered `i + 1` here. `None` for a
    /// tileset that is not usable: kept in another file, or past what a document holds.
    pub tiles: Option<Vec<Arc<Image>>>,
}

/// How a tile number and its flags are read from a cel's values.
#[derive(Clone, Copy)]
pub(crate) struct Masks {
    id: u32,
    shift: u32,
    x: u32,
    y: u32,
    diagonal: u32,
    /// The value that means no tile where the file's tile 0 is a tile: all ones.
    none: u32,
}

/// What reading the cells of one map found wrong with them.
#[derive(Default)]
struct Faults {
    unknown: bool,
    oblong: bool,
}

/// The orientation of a cell whose x flip, y flip and diagonal flip are set as given.
///
/// The three flags are the flags of Tiled's tiles, which an [`Orientation`] shares: the diagonal
/// flip (x and y swapped) is done first, then the horizontal flip, then the vertical one. Aseprite
/// 1.3's own files put the three masks in the very bit places Tiled does (x at the top bit), as
/// its MIT-licensed `src/doc/tile.h` defines them. On square tiles all eight combinations match
/// its MIT-licensed renderer (`src/render/render.cpp` at aseprite 9cf3c128) pixel for pixel
/// (`ase_flip_render_tests.rs`); no file saved and drawn by Aseprite has been checked yet. See
/// `docs/ase-compatibility.md`.
pub(crate) fn orientation(x: bool, y: bool, diagonal: bool) -> Orientation {
    let index = u8::from(x) << 2 | u8::from(y) << 1 | u8::from(diagonal);
    Orientation::from_index(index).unwrap_or(Orientation::IDENTITY)
}

impl AseTileset {
    fn usable(&self) -> bool {
        self.tiles.is_some()
    }

    fn tile_count(&self) -> usize {
        self.tiles.as_ref().map_or(0, Vec::len)
    }

    /// The cell a value of a map cel stands for, as the bits of a [`MapCell`].
    fn cell(&self, raw: u32, m: &Masks, faults: &mut Faults) -> u32 {
        if !self.empty_is_zero && raw == m.none {
            return 0;
        }
        let number = (raw & m.id).checked_shr(m.shift).unwrap_or(0);
        let Some(id) = number.checked_add(u32::from(!self.empty_is_zero)) else {
            return 0;
        };
        if id == 0 {
            return 0;
        }
        let Some(tile) = usize::try_from(id)
            .ok()
            .filter(|&id| id <= self.tile_count())
            .and_then(|_| TileId::new(id))
        else {
            faults.unknown = true;
            return 0;
        };
        let flag = |mask: u32| raw & mask != 0;
        let mut turn = orientation(flag(m.x), flag(m.y), flag(m.diagonal));
        if turn.swaps_axes() && self.size.0 != self.size.1 {
            faults.oblong = true;
            turn = orientation(flag(m.x), flag(m.y), false);
        }
        MapCell::place(tile, turn).bits()
    }
}

impl Sprite<'_> {
    /// A tileset chunk (0x2023). A chunk that ends early or holds pictures that cannot be
    /// unpacked costs only its tileset; the limits of memory and time cost the file.
    pub fn tileset_chunk(&mut self, data: &[u8]) -> Result<(), FormatError> {
        match self.read_tileset(data) {
            Err(FormatError::Corrupt(_)) => {
                self.warnings
                    .add(AseWarning::Skipped("tilesets that are cut short"));
                Ok(())
            }
            other => other,
        }
    }

    fn read_tileset(&mut self, data: &[u8]) -> Result<(), FormatError> {
        let mut r = Bytes(data);
        let id = r.u32()?;
        let flags = r.u32()?;
        let count = r.u32()?;
        let (w, h) = (usize::from(r.u16()?), usize::from(r.u16()?));
        r.skip(2 + 14)?;
        let name = r.text()?;
        let name = self.note_name(tidy(name, MAX_TILESET_NAME));
        if flags & EXTERNAL != 0 {
            r.skip(8)?;
        }
        let empty_is_zero = flags & EMPTY_IS_ZERO != 0;
        if self.tileset_at.contains_key(&id) {
            let what = "tilesets that share a number with another";
            self.warnings.add(AseWarning::Skipped(what));
            return Ok(());
        }
        if self.tilesets.len() >= MAX_KEPT_TILESETS {
            let what = "tilesets past what a document holds";
            self.warnings.add(AseWarning::Skipped(what));
            return Ok(());
        }
        let mut set = AseTileset {
            id,
            name,
            size: (w, h),
            empty_is_zero,
            tiles: None,
        };
        if flags & INSIDE == 0 {
            self.warnings
                .add(AseWarning::Skipped("tilesets kept in other files"));
        } else {
            let length = r.u32()? as usize;
            set.tiles = self.tileset_tiles(&set, count, r.take(length)?)?;
        }
        self.usable_tilesets += usize::from(set.usable());
        self.tileset_at.insert(id, self.tilesets.len());
        self.tilesets.push(set);
        Ok(())
    }

    /// The tiles of a tileset from the packed picture that holds them, or `None` when the
    /// tileset is not one a document can hold.
    fn tileset_tiles(
        &mut self,
        set: &AseTileset,
        count: u32,
        stream: &[u8],
    ) -> Result<Option<Vec<Arc<Image>>>, FormatError> {
        let (w, h) = set.size;
        let side = 1..=MAX_TILE_SIDE;
        let held = usize::from(set.empty_is_zero);
        let kept = (count as usize).saturating_sub(held);
        let full = self.usable_tilesets >= MAX_TILESETS;
        if !side.contains(&w) || !side.contains(&h) || kept > MAX_TILES || full {
            let what = "tilesets with tiles of a size or number a document cannot hold";
            self.warnings.add(AseWarning::Skipped(what));
            return Ok(None);
        }
        let tile_bytes = (w * h) as u64 * self.depth.bytes();
        let expected = tile_bytes * u64::from(count);
        self.budget.unpacking(expected)?;
        let raw = if expected == 0 {
            Some(Vec::new())
        } else {
            match unpack(stream, expected as usize, self.stop) {
                Ok(raw) => raw,
                Err(FormatError::Corrupt(_)) => None,
                Err(other) => return Err(other),
            }
        };
        let Some(raw) = raw else {
            self.warnings
                .add(AseWarning::TilesetDamaged(set.name.clone()));
            // Blank tiles share one picture but each takes its own entry, so each is charged
            // as a tile that unpacked would be: a small file cannot ask for millions free.
            let entries = (kept as u64).saturating_mul(TILE_OVERHEAD);
            self.budget.held = self
                .budget
                .held
                .checked_sub(entries)
                .ok_or(FormatError::FileTooLarge)?;
            let blank = Arc::new(Image::blank());
            return Ok(Some(vec![blank; kept]));
        };
        self.tiles_from(&raw, set, held, tile_bytes as usize)
            .map(Some)
    }

    /// One picture per tile of `raw`, from the tile numbered `first`, charged to the budget.
    fn tiles_from(
        &mut self,
        raw: &[u8],
        set: &AseTileset,
        first: usize,
        tile_bytes: usize,
    ) -> Result<Vec<Arc<Image>>, FormatError> {
        // The pixels are held while the pictures are made, so they come out of the room.
        let before = self.budget.held.saturating_sub(raw.len() as u64);
        let mut room = before;
        let clear = Some(self.transparent);
        let mut tiles = Vec::new();
        for (n, pixels) in raw.chunks_exact(tile_bytes).skip(first).enumerate() {
            if n % STOP_EVERY_PICTURES == STOP_EVERY_PICTURES - 1 && (self.stop)() {
                return Err(FormatError::Stopped);
            }
            take_room(&mut room, TILE_OVERHEAD as usize)?;
            if self.depth == Depth::Indexed {
                let highest = pixels.iter().copied().filter(|&i| Some(i) != clear);
                self.highest = self.highest.max(highest.max());
            }
            let image = image_of(set.size, pixels, (self.depth, clear), &mut room, self.stop)?;
            tiles.push(Arc::new(image));
        }
        self.budget.held = self.budget.held.saturating_sub(before - room);
        Ok(tiles)
    }

    /// Settles which tilemap layers can be brought in, before the layers are put together:
    /// a layer whose tileset is not usable is left out, with a warning. Returns the tilesets
    /// that will be in the document, as (the file's number, the document's), in file order.
    pub fn settle_tilemap_layers(&mut self) -> Vec<(u32, TilesetId)> {
        let usable = self.tilesets.iter().filter(|t| t.usable());
        let ids: Vec<(u32, TilesetId)> = usable
            .zip(1..)
            .map(|(t, n)| (t.id, TilesetId::new(n)))
            .collect();
        for layer in &mut self.layers {
            let known = ids.iter().any(|(file, _)| *file == layer.tileset);
            if layer.kind == Kind::Tilemap && !known {
                layer.kind = Kind::Skipped;
                let name = layer.name.clone();
                self.warnings.add(AseWarning::TilemapLeftOut(name));
            }
        }
        ids
    }

    /// Puts the tilesets into the document, in the order [`Sprite::settle_tilemap_layers`]
    /// numbered them, so that the layers' tileset numbers are right.
    pub fn add_tilesets(
        &mut self,
        doc: &mut Document,
        mode: ColorMode,
        ids: &[(u32, TilesetId)],
    ) -> Result<(), FormatError> {
        let sets = std::mem::take(&mut self.tilesets);
        for (set, &(_, expected)) in sets.into_iter().filter(|t| t.usable()).zip(ids) {
            let id = doc.add_tileset(&set.name, mode, set.size).map_err(limit)?;
            if id != expected {
                return Err(FormatError::Corrupt("the tilesets are out of order"));
            }
            for image in set.tiles.unwrap_or_default() {
                doc.add_tile(id, image).map_err(limit)?;
            }
        }
        Ok(())
    }

    /// A tilemap cel (cel type 3) of the layer numbered `layer`, in frame `frame`: a picture of
    /// cells, each the tile number and flags of a [`MapCell`].
    pub fn tilemap_cel(
        &mut self,
        layer: usize,
        frame: usize,
        (x, y, opacity): (i16, i16, u8),
        r: &mut Bytes<'_>,
    ) -> Result<Cel, FormatError> {
        let (w, h) = (usize::from(r.u16()?), usize::from(r.u16()?));
        let bits = r.u16()?;
        let masks = [r.u32()?, r.u32()?, r.u32()?, r.u32()?];
        r.skip(10)?;
        let (name, tileset) = match self.layers.get(layer) {
            Some(l) => (l.name.clone(), l.tileset),
            None => return Err(FormatError::Corrupt("a cel on a layer that does not exist")),
        };
        if opacity != u8::MAX {
            self.warnings.add(AseWarning::CelOpacity(name.clone()));
        }
        if w > MAX_SIDE || h > MAX_SIDE || (w * h) as u64 > MAX_PIXELS {
            return Err(FormatError::TooLarge);
        }
        let step = match bits {
            8 | 16 | 32 => usize::from(bits / 8),
            _ => return Ok(damaged(&mut self.warnings, name, frame)),
        };
        let expected = (w * h * step) as u64;
        let Some(set) = self.tileset_at.get(&tileset).map(|&at| &self.tilesets[at]) else {
            return Ok(damaged(&mut self.warnings, name, frame));
        };
        self.budget.unpacking(expected)?;
        let Some(raw) = unpack(r.0, expected as usize, self.stop)? else {
            return Ok(damaged(&mut self.warnings, name, frame));
        };
        let masks = Masks::new(masks, bits);
        // The pixels are held while the picture is made, so they come out of the room.
        let before = self.budget.held.saturating_sub(raw.len() as u64);
        let mut room = before;
        let mut faults = Faults::default();
        let stop = self.stop;
        let cells = |at: usize| set.cell(value_at(&raw, at, step), &masks, &mut faults);
        let image = image_from((w, h), cells, &mut room, stop)?;
        self.budget.held = self.budget.held.saturating_sub(before - room);
        if faults.unknown {
            self.warnings.add(AseWarning::UnknownTile(name.clone()));
        }
        if faults.oblong {
            self.warnings.add(AseWarning::DiagonalOnOblong(name));
        }
        Cel::at(image, x.into(), y.into())
            .ok_or(FormatError::Corrupt("a cel sits too far from the canvas"))
    }
}

/// A map that cannot be read, left empty and said so.
fn damaged(warnings: &mut Warnings, layer: String, frame: usize) -> Cel {
    warnings.add(AseWarning::Damaged { layer, frame });
    Cel::blank()
}
impl Masks {
    fn new([id, x, y, diagonal]: [u32; 4], bits: u16) -> Masks {
        let width = match bits {
            8 => u32::from(u8::MAX),
            16 => u32::from(u16::MAX),
            _ => u32::MAX,
        };
        Masks {
            id,
            shift: id.trailing_zeros(),
            x,
            y,
            diagonal,
            none: width,
        }
    }
}

/// The `step`-byte little-endian value at number `at` of `raw`, or 0 past its end.
fn value_at(raw: &[u8], at: usize, step: usize) -> u32 {
    let Some(bytes) = raw.get(at * step..(at + 1) * step) else {
        return 0;
    };
    bytes
        .iter()
        .rev()
        .fold(0u32, |all, &byte| all << 8 | u32::from(byte))
}

fn limit(error: LimitError) -> FormatError {
    match error {
        LimitError::Dimensions => FormatError::TooLarge,
        // Tiles a project file could not keep: what is opened must save and open again.
        LimitError::TileMemory => FormatError::TilesTooLarge,
        _ => FormatError::Corrupt("the file does not fit a document"),
    }
}

#[cfg(test)]
#[path = "ase_tileset_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ase_tileset_scale_tests.rs"]
mod scale_tests;

#[cfg(test)]
#[path = "ase_flip_render_tests.rs"]
mod flip_render_tests;
