//! Indexed PNG rows: each straight RGBA cell written as its place in a palette, packed into as
//! few bits as the palette needs. A cell whose colour is not in the palette is an error, never
//! the nearest colour: an indexed image is the same picture or it is not written.

use std::io;

/// The most colours a PNG palette holds.
pub const MAX_COLOURS: usize = 256;

/// The fewest bits per cell that number `colours` entries: 1, 2, 4 or 8.
pub fn depth_for(colours: usize) -> u8 {
    match colours {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    }
}

/// A palette as a writer looks colours up in it.
pub(crate) struct Indexer {
    /// Each colour and its place, sorted by colour.
    sorted: Vec<(u32, u8)>,
    depth: u8,
    /// The colour last looked up and its place: pixel art runs of one colour cost one test.
    last: (u32, u8),
}

fn invalid(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what)
}

impl Indexer {
    /// A palette of 1 to 256 different straight RGBA colours, in the order they are written.
    pub(crate) fn new(palette: &[u32]) -> io::Result<Self> {
        if palette.is_empty() || palette.len() > MAX_COLOURS {
            return Err(invalid("a palette holds 1 to 256 colours"));
        }
        let mut sorted: Vec<(u32, u8)> = palette
            .iter()
            .zip(0..=u8::MAX)
            .map(|(&c, i)| (c, i))
            .collect();
        pikslar_core::sorting::sort_packed(
            &mut sorted,
            |&(colour, at)| (u64::from(colour) << 8) | u64::from(at),
            |key| ((key >> 8) as u32, key as u8),
        );
        if sorted.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err(invalid("a palette names each colour once"));
        }
        let last = sorted[0];
        Ok(Indexer {
            sorted,
            depth: depth_for(palette.len()),
            last,
        })
    }

    pub(crate) fn depth(&self) -> u8 {
        self.depth
    }

    /// Bytes in one packed row of `width` cells, without its filter byte.
    pub(crate) fn row_bytes(&self, width: usize) -> usize {
        (width * self.depth as usize).div_ceil(8)
    }

    fn place(&mut self, colour: u32) -> Option<u8> {
        if self.last.0 == colour {
            return Some(self.last.1);
        }
        let at = self.sorted.binary_search_by_key(&colour, |e| e.0).ok()?;
        self.last = self.sorted[at];
        Some(self.last.1)
    }

    /// Appends `row` packed, the first cell in the highest bits of its byte, as PNG orders them.
    /// On a colour not in the palette nothing is appended.
    pub(crate) fn pack(&mut self, row: &[u32], out: &mut Vec<u8>) -> io::Result<()> {
        let start = out.len();
        let depth = u32::from(self.depth);
        let per_byte = 8 / depth;
        let (mut byte, mut filled) = (0u32, 0u32);
        for &cell in row {
            let Some(place) = self.place(cell) else {
                out.truncate(start);
                return Err(invalid("a colour that is not in the palette"));
            };
            byte = (byte << depth) | u32::from(place);
            filled += 1;
            if filled == per_byte {
                out.push(byte as u8);
                (byte, filled) = (0, 0);
            }
        }
        if filled > 0 {
            out.push((byte << (depth * (per_byte - filled))) as u8);
        }
        Ok(())
    }
}

impl Indexer {
    /// Appends each cell of `row` as its place, one byte a cell, as GIF codes them. On a colour
    /// not in the palette nothing is appended.
    pub(crate) fn places(&mut self, row: &[u32], out: &mut Vec<u8>) -> io::Result<()> {
        let start = out.len();
        for &cell in row {
            let Some(place) = self.place(cell) else {
                out.truncate(start);
                return Err(invalid("a colour that is not in the palette"));
            };
            out.push(place);
        }
        Ok(())
    }
}

/// The PLTE body: red, green and blue of each entry.
pub(crate) fn plte(palette: &[u32]) -> Vec<u8> {
    palette
        .iter()
        .flat_map(|c| [*c as u8, (c >> 8) as u8, (c >> 16) as u8])
        .collect()
}

/// The tRNS body: each entry's alpha up to the last that is not opaque. None if all are.
pub(crate) fn trns(palette: &[u32]) -> Option<Vec<u8>> {
    let last = palette.iter().rposition(|c| c >> 24 != 255)?;
    Some(palette[..=last].iter().map(|c| (c >> 24) as u8).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_depth_is_the_fewest_bits_that_number_every_colour() {
        let cases = [
            (1, 1),
            (2, 1),
            (3, 2),
            (4, 2),
            (5, 4),
            (16, 4),
            (17, 8),
            (255, 8),
            (256, 8),
        ];
        for (colours, depth) in cases {
            assert_eq!(depth_for(colours), depth, "{colours} colours");
        }
    }

    #[test]
    fn cells_pack_from_the_highest_bits_and_rows_end_on_a_byte() {
        let palette: Vec<u32> = (0..4).map(|i| 0xff00_0000 | i).collect();
        let mut ix = Indexer::new(&palette).expect("palette");
        let mut out = Vec::new();
        ix.pack(
            &[palette[1], palette[2], palette[3], palette[0], palette[3]],
            &mut out,
        )
        .expect("packed");
        assert_eq!(out, [0b0110_1100, 0b1100_0000]);
        assert_eq!(ix.row_bytes(5), 2);
        let palette: Vec<u32> = (0..2).map(|i| 0xff00_0000 | i).collect();
        let mut ix = Indexer::new(&palette).expect("palette");
        let mut out = Vec::new();
        let row: Vec<u32> = [1, 0, 1, 1, 0, 0, 0, 1, 1]
            .iter()
            .map(|&i| palette[i])
            .collect();
        ix.pack(&row, &mut out).expect("packed");
        assert_eq!(out, [0b1011_0001, 0b1000_0000]);
        let palette: Vec<u32> = (0..16).map(|i| 0xff00_0000 | i).collect();
        let mut ix = Indexer::new(&palette).expect("palette");
        let mut out = Vec::new();
        ix.pack(&[palette[15], palette[1], palette[7]], &mut out)
            .expect("packed");
        assert_eq!(out, [0xf1, 0x70]);
    }

    #[test]
    fn eight_bit_cells_are_their_places_in_the_order_given() {
        let palette: Vec<u32> = (0..256).rev().map(|i| 0xff00_0000 | i).collect();
        let mut ix = Indexer::new(&palette).expect("palette");
        let mut out = Vec::new();
        ix.pack(&[0xff00_00ff, 0xff00_0000, 0xff00_0080], &mut out)
            .expect("packed");
        assert_eq!(out, [0, 255, 127]);
    }

    #[test]
    fn a_colour_not_in_the_palette_is_refused_and_appends_nothing() {
        let palette = [0xff00_0000, 0xff00_0001];
        let mut ix = Indexer::new(&palette).expect("palette");
        let mut out = vec![9];
        assert!(ix
            .pack(&[palette[0], palette[1], 0xff00_0002], &mut out)
            .is_err());
        assert_eq!(out, [9]);
        let mut row = vec![palette[1]; 17];
        row.push(0xff00_0002);
        assert!(ix.pack(&row, &mut out).is_err(), "after two whole bytes");
        assert_eq!(out, [9]);
        assert!(ix.pack(&[0xfe00_0000], &mut out).is_err());
    }

    #[test]
    fn palettes_hold_one_to_256_different_colours() {
        assert!(Indexer::new(&[]).is_err());
        let full: Vec<u32> = (0..256).collect();
        assert!(Indexer::new(&full).is_ok());
        let over: Vec<u32> = (0..257).collect();
        assert!(Indexer::new(&over).is_err());
        assert!(Indexer::new(&[5, 6, 5]).is_err());
    }

    #[test]
    fn transparency_lists_alphas_up_to_the_last_translucent_entry() {
        let opaque = [0xff00_0000, 0xff12_3456];
        assert_eq!(trns(&opaque), None);
        let mixed = [0x0000_0000, 0x80ff_0000, 0xff00_ff00, 0xff00_00ff];
        assert_eq!(trns(&mixed), Some(vec![0, 0x80]));
        assert_eq!(plte(&mixed[1..3]), [0, 0, 0xff, 0, 0xff, 0]);
        let late = [0xff00_0000, 0x0100_0000];
        assert_eq!(trns(&late), Some(vec![255, 1]));
    }
}
