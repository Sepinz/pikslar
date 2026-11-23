//! Immutable 16x16 tiles, stored as compactly as their contents allow.

pub const TILE_SHIFT: usize = 4;
pub const TILE: usize = 1 << TILE_SHIFT;
pub const TILE_AREA: usize = TILE * TILE;

/// A fully decoded tile, used as a work buffer.
pub type RawTile = [u32; TILE_AREA];

/// One tile of committed image data. Never mutated once built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tile {
    /// Every cell holds this non-zero value.
    Solid(u32),
    /// A local list of cell values plus 1, 2, 4 or 8-bit indices into it.
    Packed {
        palette: Box<[u32]>,
        bits: u8,
        data: Box<[u8]>,
    },
    /// More than 256 distinct values.
    Raw(Box<RawTile>),
}

impl Tile {
    /// Builds the most compact tile for `px`, or `None` if every cell is 0.
    pub fn pack(px: &RawTile) -> Option<Tile> {
        let first = px[0];
        if px.iter().all(|&p| p == first) {
            return (first != 0).then_some(Tile::Solid(first));
        }
        let mut palette: Vec<u32> = Vec::with_capacity(16);
        let mut indices = [0u8; TILE_AREA];
        let (mut last, mut last_index) = (first, 0u8);
        palette.push(first);
        for (slot, &p) in indices.iter_mut().zip(px.iter()) {
            if p != last {
                let found = palette.iter().position(|&c| c == p);
                let index = match found {
                    Some(i) => i,
                    None if palette.len() == 256 => return Some(Tile::Raw(Box::new(*px))),
                    None => {
                        palette.push(p);
                        palette.len() - 1
                    }
                };
                last = p;
                last_index = index as u8;
            }
            *slot = last_index;
        }
        let bits = packed_bits(palette.len());
        let mut data = vec![0u8; (TILE_AREA * bits as usize).div_ceil(8)];
        for (i, &v) in indices.iter().enumerate() {
            let bit = i * bits as usize;
            data[bit / 8] |= v << (bit % 8);
        }
        Some(Tile::Packed {
            palette: palette.into_boxed_slice(),
            bits,
            data: data.into_boxed_slice(),
        })
    }

    /// True when every cell is 0, however the tile is encoded. Tiles made by [`Tile::pack`]
    /// never are, so this is for tiles that came from elsewhere. It runs on every tile a project
    /// loads, so it reads the packed indices and stops at the first cell that shows something.
    pub fn draws_nothing(&self) -> bool {
        match self {
            Tile::Solid(v) => *v == 0,
            Tile::Packed {
                palette,
                bits,
                data,
            } => packed_draws_nothing(palette, *bits, data),
            Tile::Raw(raw) => raw.iter().all(|&v| v == 0),
        }
    }

    /// Writes every cell into `out`.
    #[inline]
    pub fn decode(&self, out: &mut RawTile) {
        match self {
            Tile::Solid(v) => out.fill(*v),
            Tile::Raw(r) => out.copy_from_slice(&r[..]),
            Tile::Packed {
                palette,
                bits: 8,
                data,
            } => {
                for (o, &b) in out.iter_mut().zip(data.iter()) {
                    *o = palette.get(b as usize).copied().unwrap_or(0);
                }
            }
            Tile::Packed {
                palette,
                bits,
                data,
            } => {
                let per_byte = 8 / *bits as usize;
                let mask = (1u8 << *bits) - 1;
                for (i, o) in out.iter_mut().enumerate() {
                    let byte = data.get(i / per_byte).copied().unwrap_or(0);
                    let index = (byte >> ((i % per_byte) * *bits as usize)) & mask;
                    *o = palette.get(index as usize).copied().unwrap_or(0);
                }
            }
        }
    }

    /// Writes the cells of the rectangle at `from` of this `size` into `out` at `to`, leaving
    /// the rest of `out` alone. Both rectangles must lie inside a tile.
    pub fn decode_rect(
        &self,
        from: (usize, usize),
        size: (usize, usize),
        out: &mut RawTile,
        to: (usize, usize),
    ) {
        let (w, h) = size;
        for row in 0..h {
            let source = (from.1 + row) * TILE + from.0;
            let target = (to.1 + row) * TILE + to.0;
            let out = &mut out[target..target + w];
            match self {
                Tile::Solid(v) => out.fill(*v),
                Tile::Raw(r) => out.copy_from_slice(&r[source..source + w]),
                Tile::Packed { .. } => {
                    for (i, o) in out.iter_mut().enumerate() {
                        *o = self.get(source + i);
                    }
                }
            }
        }
    }

    /// Reads one cell, given its index within the tile.
    #[inline]
    pub fn get(&self, i: usize) -> u32 {
        match self {
            Tile::Solid(v) => *v,
            Tile::Raw(r) => r[i],
            Tile::Packed {
                palette,
                bits,
                data,
            } => {
                let bit = i * *bits as usize;
                let mask = ((1u16 << *bits) - 1) as u8;
                let byte = data.get(bit / 8).copied().unwrap_or(0);
                let index = (byte >> (bit % 8)) & mask;
                palette.get(index as usize).copied().unwrap_or(0)
            }
        }
    }

    /// Approximate memory held by this tile, for history budgeting.
    pub fn heap_size(&self) -> usize {
        const OVERHEAD: usize = 48;
        OVERHEAD
            + match self {
                Tile::Solid(_) => 0,
                Tile::Packed { palette, data, .. } => palette.len() * 4 + data.len(),
                Tile::Raw(_) => TILE_AREA * 4,
            }
    }
}

/// What a value becomes under an id map: 0 stays 0, and a value past the map stays itself.
fn moved_value(map: &[u32], value: u32) -> u32 {
    match value {
        0 => 0,
        v => map.get(v as usize).copied().unwrap_or(v),
    }
}

/// The index width [`Tile::pack`] gives a local palette of `len` values.
fn packed_bits(len: usize) -> u8 {
    match len {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    }
}

/// The most local colours for which a mapped palette is checked for repeats pair by pair.
const QUICK_PALETTE: usize = 16;

fn all_different(values: &[u32]) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(i, v)| !values[i + 1..].contains(v))
}

/// True for a packed tile exactly as [`Tile::pack`] makes it, with at most [`QUICK_PALETTE`]
/// colours: distinct values, every one used, in the order the cells first show them, at the
/// width pack picks. Moving such a palette's values through a map that keeps them distinct gives
/// what packing the moved cells would.
fn packed_as_pack_makes(palette: &[u32], bits: u8, data: &[u8]) -> bool {
    let len = palette.len();
    let width = usize::from(bits);
    if !(2..=QUICK_PALETTE).contains(&len)
        || bits != packed_bits(len)
        || data.len() != TILE_AREA * width / 8
        || !all_different(palette)
    {
        return false;
    }
    let mask = (1u8 << bits) - 1;
    let mut next = 0;
    for &byte in data {
        for shift in (0..8).step_by(width) {
            let index = usize::from((byte >> shift) & mask);
            if index > next || index >= len {
                return false;
            }
            next += usize::from(index == next);
        }
    }
    next == len
}

impl Tile {
    /// The tile with every value moved through `map` (see [`moved_value`]): `None` if no cell
    /// changes, else what [`Tile::pack`] makes of the moved cells. A packed tile of few colours
    /// whose values stay distinct only has its local palette moved, without decoding.
    pub(crate) fn map_values(&self, map: &[u32]) -> Option<Option<Tile>> {
        match self {
            Tile::Solid(v) => {
                let to = moved_value(map, *v);
                (to != *v).then(|| (to != 0).then_some(Tile::Solid(to)))
            }
            // Cells show only listed values, or 0 past the list, so if no listed value moves,
            // no cell does.
            Tile::Packed { palette, .. } if palette.iter().all(|&v| moved_value(map, v) == v) => {
                None
            }
            Tile::Packed {
                palette,
                bits,
                data,
            } if packed_as_pack_makes(palette, *bits, data) => {
                let moved: Box<[u32]> = palette.iter().map(|&v| moved_value(map, v)).collect();
                if !all_different(&moved) {
                    return self.map_cells(map);
                }
                let (bits, data) = (*bits, data.clone());
                Some(Some(Tile::Packed {
                    palette: moved,
                    bits,
                    data,
                }))
            }
            _ => self.map_cells(map),
        }
    }

    /// True if some cell holds `value`. A packed tile without a value other than 0 in its local
    /// palette cannot show it, and one as pack makes it shows every value it lists, so only
    /// other tiles are decoded.
    pub(crate) fn shows(&self, value: u32) -> bool {
        match self {
            Tile::Solid(v) => *v == value,
            Tile::Packed {
                palette,
                bits,
                data,
            } if value != 0 => {
                palette.contains(&value)
                    && (packed_as_pack_makes(palette, *bits, data) || self.decodes_to(value))
            }
            _ => self.decodes_to(value),
        }
    }

    fn decodes_to(&self, value: u32) -> bool {
        let mut raw: RawTile = [0; TILE_AREA];
        self.decode(&mut raw);
        raw.contains(&value)
    }

    /// [`Tile::map_values`] the long way: every cell decoded, moved, and packed again.
    fn map_cells(&self, map: &[u32]) -> Option<Option<Tile>> {
        let mut raw: RawTile = [0; TILE_AREA];
        self.decode(&mut raw);
        let mut moved = false;
        for px in raw.iter_mut() {
            let to = moved_value(map, *px);
            moved |= to != *px;
            *px = to;
        }
        moved.then(|| Tile::pack(&raw))
    }
}

/// True when every index in `data` picks a 0 from `palette`. An index past the palette also
/// reads as 0, as [`Tile::decode`] reads it.
fn packed_draws_nothing(palette: &[u32], bits: u8, data: &[u8]) -> bool {
    let blank = |index: usize| palette.get(index).is_none_or(|&c| c == 0);
    let bytes = (TILE_AREA * bits as usize).div_ceil(8);
    let data = &data[..bytes.min(data.len())];
    let short = data.len() < bytes;
    let byte_blank: &dyn Fn(u8) -> bool = match bits {
        // Cells past short data are left as 0, which shows nothing.
        8 => &|byte| blank(byte as usize),
        // Widths no file holds are decoded, so the answer always agrees with decoding.
        3 | 5..=7 => return decodes_blank(palette, bits, data),
        1 | 2 | 4 => {
            let bits = bits as usize;
            let mask = (1u16 << bits) - 1;
            let set = (0..1usize << bits)
                .filter(|&index| blank(index))
                .fold(0u16, |set, index| set | 1 << index);
            // A missing byte reads as index 0 in every cell it covers.
            if set == 0 || (short && set & 1 == 0) {
                return false;
            }
            &move |byte| (0..8 / bits).all(|k| set >> ((byte as u16 >> (k * bits)) & mask) & 1 == 1)
        }
        _ => return false,
    };
    // Most tiles show something somewhere, so a few spread-out bytes usually settle it before
    // the full scan, which stops at the first byte that shows something.
    let len = data.len();
    let probes = [len / 2, len.saturating_sub(1), len / 4, len * 3 / 4];
    probes
        .iter()
        .all(|&at| data.get(at).is_none_or(|&b| byte_blank(b)))
        && data.iter().all(|&b| byte_blank(b))
}

fn decodes_blank(palette: &[u32], bits: u8, data: &[u8]) -> bool {
    let tile = Tile::Packed {
        palette: palette.into(),
        bits,
        data: data.into(),
    };
    let mut raw = [0u32; TILE_AREA];
    tile.decode(&mut raw);
    raw.iter().all(|&v| v == 0)
}

#[cfg(test)]
#[path = "tile_map_tests.rs"]
mod map_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(px: &RawTile) -> Option<Tile> {
        let tile = Tile::pack(px);
        let mut out = [0; TILE_AREA];
        if let Some(t) = &tile {
            t.decode(&mut out);
            for (i, &p) in px.iter().enumerate() {
                assert_eq!(t.get(i), p, "get({i})");
            }
        }
        assert_eq!(&out, px);
        tile
    }

    #[test]
    fn blank_tile_stores_nothing() {
        assert_eq!(Tile::pack(&[0; TILE_AREA]), None);
    }

    #[test]
    fn uniform_tile_is_solid() {
        assert_eq!(round_trip(&[7; TILE_AREA]), Some(Tile::Solid(7)));
    }

    fn bits_for(colors: u32) -> u8 {
        let mut px = [0; TILE_AREA];
        for (i, p) in px.iter_mut().enumerate() {
            *p = (i as u32 % colors) * 1000 + 1;
        }
        match round_trip(&px) {
            Some(Tile::Packed { bits, .. }) => bits,
            other => panic!("expected packed, got {other:?}"),
        }
    }

    #[test]
    fn index_width_tracks_color_count() {
        assert_eq!(bits_for(2), 1);
        assert_eq!(bits_for(4), 2);
        assert_eq!(bits_for(16), 4);
        assert_eq!(bits_for(17), 8);
        assert_eq!(bits_for(256), 8);
    }

    #[test]
    fn all_values_different_survives() {
        let mut px = [0; TILE_AREA];
        for (i, p) in px.iter_mut().enumerate() {
            *p = i as u32 + 1;
        }
        // 256 distinct values still fit an 8-bit local palette.
        assert!(matches!(
            round_trip(&px),
            Some(Tile::Packed { bits: 8, .. })
        ));
    }

    #[test]
    fn transparent_mixed_with_color_round_trips() {
        let mut px = [0; TILE_AREA];
        px[17] = 0xff00_00ff;
        px[255] = 0x8000_ff00;
        round_trip(&px);
    }
}
