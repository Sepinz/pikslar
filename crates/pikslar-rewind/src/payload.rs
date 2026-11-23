//! What one stored step holds, before framing: see "Record format" in `docs/rewind.md`.
//! Reading treats the bytes as untrusted: every count is checked against what is left.

use crate::RewindError;
use pikslar_core::grid::Cell;
use pikslar_core::tile::{Tile, TILE_AREA};
use pikslar_core::{OlderStep, StepView, TileKey};
use std::sync::Arc;

pub const EMPTY: u8 = 0;
pub const PIXELS: u8 = 1;
pub const WHOLE: u8 = 2;
/// A pixel step that split its cel's drawing off another frame's, which it names.
pub const SHARED_PIXELS: u8 = 3;

const BAD: RewindError = RewindError::Damaged("a stored step does not read");

/// A step's kind and payload, as the walk passes it.
pub fn encode(view: StepView<'_>) -> (u8, Vec<u8>) {
    match view {
        StepView::Unchanged => (EMPTY, Vec::new()),
        StepView::Pixels {
            layer,
            frame,
            before,
            shares,
        } => encode_pixels((layer, frame), before, shares),
        StepView::Whole(doc) => (WHOLE, pikslar_format::piks::save(doc)),
    }
}

/// A step as the walk passes it, taken out of the editor to encode later.
pub enum Taken {
    /// A pixel step's tiles, shared with the editor: tiles are not what history is charged
    /// by, so holding them changes no charge.
    Pixels {
        layer: usize,
        frame: usize,
        before: Vec<(TileKey, Cell)>,
        shares: Option<usize>,
    },
    /// Encoded as it passed. A whole document is never held: a copy shares drawings that
    /// history charges by how many hold them, and the walk plays steps back while it is held.
    Encoded(u8, Vec<u8>),
}

impl Taken {
    pub fn of(view: StepView<'_>) -> Taken {
        match view {
            StepView::Pixels {
                layer,
                frame,
                before,
                shares,
            } => Taken::Pixels {
                layer,
                frame,
                before: before.to_vec(),
                shares,
            },
            other => {
                let (kind, payload) = encode(other);
                Taken::Encoded(kind, payload)
            }
        }
    }

    pub fn encode(self) -> (u8, Vec<u8>) {
        match self {
            Taken::Pixels {
                layer,
                frame,
                before,
                shares,
            } => encode_pixels((layer, frame), &before, shares),
            Taken::Encoded(kind, payload) => (kind, payload),
        }
    }
}

fn encode_pixels(
    (layer, frame): (usize, usize),
    before: &[(TileKey, Cell)],
    shares: Option<usize>,
) -> (u8, Vec<u8>) {
    let mut out = Vec::with_capacity(10 + before.len() * 80);
    // Within a document's limits, which fit 16 bits.
    out.extend_from_slice(&(layer as u16).to_le_bytes());
    out.extend_from_slice(&(frame as u16).to_le_bytes());
    out.extend_from_slice(&(before.len() as u32).to_le_bytes());
    for (key, cell) in before {
        out.extend_from_slice(&key.x.to_le_bytes());
        out.extend_from_slice(&key.y.to_le_bytes());
        write_tile(&mut out, cell.as_deref());
    }
    match shares {
        Some(other) => {
            out.extend_from_slice(&(other as u16).to_le_bytes());
            (SHARED_PIXELS, out)
        }
        None => (PIXELS, out),
    }
}

/// As `.piks` stores a tile.
fn write_tile(out: &mut Vec<u8>, tile: Option<&Tile>) {
    let words = |out: &mut Vec<u8>, values: &[u32]| {
        values
            .iter()
            .for_each(|v| out.extend_from_slice(&v.to_le_bytes()))
    };
    match tile {
        None => out.push(0),
        Some(Tile::Solid(v)) => {
            out.push(1);
            words(out, &[*v]);
        }
        Some(Tile::Packed {
            palette,
            bits,
            data,
        }) => {
            out.push(2);
            out.extend_from_slice(&(palette.len() as u16).to_le_bytes());
            out.push(*bits);
            words(out, palette);
            out.extend_from_slice(data);
        }
        Some(Tile::Raw(raw)) => {
            out.push(3);
            words(out, &raw[..]);
        }
    }
}

/// Reads a payload back into a step. A whole document goes through the `.piks` reader, held to
/// what a stored step may decode to; a pixel step is read here, every count checked, naming at
/// most `room` tiles, each once.
pub fn decode(kind: u8, payload: &[u8], room: usize) -> Result<OlderStep, RewindError> {
    let most = crate::frame::MOST_DECODED as usize;
    match kind {
        EMPTY if payload.is_empty() => Ok(OlderStep::Unchanged),
        PIXELS => decode_pixels(payload, room, false),
        SHARED_PIXELS => decode_pixels(payload, room, true),
        WHOLE => match pikslar_format::piks::load_noted_within(payload, most) {
            Ok((doc, notes)) if notes.can_save_over() => Ok(OlderStep::Whole(Box::new(doc))),
            _ => Err(BAD),
        },
        _ => Err(BAD),
    }
}

struct Input<'a>(&'a [u8]);

impl<'a> Input<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], RewindError> {
        if n > self.0.len() {
            return Err(BAD);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn u8(&mut self) -> Result<u8, RewindError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, RewindError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, RewindError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn words(&mut self, count: usize) -> Result<Vec<u32>, RewindError> {
        let raw = self.take(count.checked_mul(4).ok_or(BAD)?)?;
        let words = raw.chunks_exact(4);
        Ok(words
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect())
    }
}

/// The most a payload of `kind` naming at most `room` tiles can be, decoded: checked before it
/// is inflated, so a step can never make the reader hold more than its document could need.
pub fn most_decoded(kind: u8, room: usize) -> u64 {
    // A tile's key, then its largest shape: 8-bit packed with every color, or raw.
    let packed = 1 + 2 + 1 + 4 * 256 + TILE_AREA;
    let tile = 4 + packed.max(1 + 4 * TILE_AREA);
    let most = u64::from(crate::frame::MOST_DECODED);
    match kind {
        EMPTY => 0,
        PIXELS | SHARED_PIXELS => (10 + room as u64 * tile as u64).min(most),
        _ => most,
    }
}

fn decode_pixels(payload: &[u8], room: usize, shared: bool) -> Result<OlderStep, RewindError> {
    let mut input = Input(payload);
    let layer = input.u16()? as usize;
    let frame = input.u16()? as usize;
    let count = input.u32()? as usize;
    // Every tile takes at least five bytes, and the document has room for only so many.
    if count == 0 || count > input.0.len() / 5 || count > room {
        return Err(BAD);
    }
    let mut before = Vec::new();
    before.try_reserve_exact(count).map_err(|_| BAD)?;
    for _ in 0..count {
        let x = input.u16()? as i16;
        let y = input.u16()? as i16;
        let tile = read_tile(&mut input)?.map(Arc::new);
        before.push((TileKey { x, y }, tile));
    }
    let shares = match shared {
        true => Some(input.u16()? as usize),
        false => None,
    };
    if !input.0.is_empty() || !distinct(&before) {
        return Err(BAD);
    }
    Ok(OlderStep::Pixels {
        layer,
        frame,
        before,
        shares,
    })
}

/// True if no tile is named twice.
fn distinct(tiles: &[(TileKey, Cell)]) -> bool {
    let mut keys: Vec<(i16, i16)> = tiles.iter().map(|(k, _)| (k.x, k.y)).collect();
    keys.sort_unstable();
    keys.windows(2).all(|pair| pair[0] != pair[1])
}

/// A tile as `.piks` stores it, refused unless its shape is one a tile can have. A tile that
/// shows nothing is kept as none, as the document keeps it.
fn read_tile(input: &mut Input<'_>) -> Result<Option<Tile>, RewindError> {
    let tile = match input.u8()? {
        0 => return Ok(None),
        1 => Tile::Solid(input.u32()?),
        2 => {
            let count = input.u16()? as usize;
            let bits = input.u8()?;
            if !matches!(bits, 1 | 2 | 4 | 8) || count == 0 || count > 1usize << bits {
                return Err(BAD);
            }
            let palette = input.words(count)?.into_boxed_slice();
            let data = input.take((TILE_AREA * bits as usize).div_ceil(8))?.into();
            Tile::Packed {
                palette,
                bits,
                data,
            }
        }
        3 => {
            let raw: Box<[u32; TILE_AREA]> = input
                .words(TILE_AREA)?
                .into_boxed_slice()
                .try_into()
                .map_err(|_| BAD)?;
            Tile::Raw(raw)
        }
        _ => return Err(BAD),
    };
    Ok((!tile.draws_nothing()).then_some(tile))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::{ColorMode, Document};

    fn tiles() -> Vec<(TileKey, Cell)> {
        let mut raw = [0u32; TILE_AREA];
        raw.iter_mut()
            .enumerate()
            .for_each(|(i, v)| *v = i as u32 + 1);
        let mut few = [0u32; TILE_AREA];
        few[3] = 7;
        vec![
            (TileKey { x: 0, y: 0 }, None),
            (TileKey { x: -1, y: 2 }, Some(Arc::new(Tile::Solid(5)))),
            (TileKey { x: 1, y: 1 }, Tile::pack(&few).map(Arc::new)),
            (TileKey { x: 2, y: -3 }, Tile::pack(&raw).map(Arc::new)),
        ]
    }

    #[test]
    fn every_kind_of_step_reads_back_as_it_was_written() {
        let before = tiles();
        let view = StepView::Pixels {
            layer: 3,
            frame: 9,
            before: &before,
            shares: None,
        };
        let (kind, bytes) = encode(view);
        let Ok(OlderStep::Pixels {
            layer: 3,
            frame: 9,
            before: back,
            shares: None,
        }) = decode(kind, &bytes, 64)
        else {
            panic!("pixels read back");
        };
        assert_eq!(back, before);
        let view = StepView::Pixels {
            layer: 3,
            frame: 9,
            before: &before,
            shares: Some(4),
        };
        let (kind, bytes) = encode(view);
        assert_eq!(kind, SHARED_PIXELS);
        let shared = decode(kind, &bytes, 64);
        assert!(matches!(
            shared,
            Ok(OlderStep::Pixels {
                shares: Some(4),
                ..
            })
        ));
        let doc = Document::new(5, 3, ColorMode::Rgba).expect("valid");
        let (kind, bytes) = encode(StepView::Whole(&doc));
        assert!(matches!(decode(kind, &bytes, 64), Ok(OlderStep::Whole(d)) if *d == doc));
        let (kind, bytes) = encode(StepView::Unchanged);
        assert!(matches!(decode(kind, &bytes, 64), Ok(OlderStep::Unchanged)));
    }

    #[test]
    fn a_step_cut_short_or_padded_or_of_an_unknown_kind_is_refused() {
        let before = tiles();
        let (kind, bytes) = encode(StepView::Pixels {
            layer: 0,
            frame: 0,
            before: &before,
            shares: Some(1),
        });
        for len in 0..bytes.len() {
            assert!(decode(kind, &bytes[..len], 64).is_err(), "cut to {len}");
        }
        let mut padded = bytes.clone();
        padded.push(0);
        assert!(decode(kind, &padded, 64).is_err());
        assert!(decode(9, &bytes, 64).is_err());
        assert!(decode(EMPTY, &[1], 64).is_err());
        assert!(decode(WHOLE, &bytes, 64).is_err());
        assert!(
            decode(PIXELS, &bytes, 64).is_err(),
            "read as the other kind"
        );
    }

    #[test]
    fn a_huge_count_is_refused_before_anything_is_allocated() {
        let mut bytes = vec![0, 0, 0, 0];
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        assert!(decode(PIXELS, &bytes, 64).is_err());
        let mut packed = vec![0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2];
        packed.extend_from_slice(&[0xff, 0xff, 8]);
        assert!(
            decode(PIXELS, &packed, 64).is_err(),
            "more colors than the bits allow"
        );
    }
}
