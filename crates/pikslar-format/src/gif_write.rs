//! GIF89a written as its rows come: one global palette, and each frame an area of the canvas
//! that is kept for the next frame or cleared before it. A cell is opaque or, if the image has
//! one, the clear entry, which leaves what is beneath; any other colour is an error, never the
//! nearest: a GIF is the same picture or it is not written. What a frame must draw, and what
//! must be cleared, is the caller's to work out.
//!
//! What is held is one band's indices, the LZW table and up to 64 KiB of coded bytes.

use crate::lzw::Lzw;
use crate::png_index::{Indexer, MAX_COLOURS};
use std::io::{self, Write};

/// Coded bytes held before they go to the file.
const FLUSH_AT: usize = 64 * 1024;
/// The largest sub-block GIF allows.
const BLOCK: usize = 255;

/// What is left of a frame when the next is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposal {
    /// The frame stays, and the next is drawn over it (disposal 1).
    Keep,
    /// The frame's area is cleared to the background, the clear entry (disposal 2).
    Clear,
}

impl Disposal {
    /// The disposal as a graphic control extension's packed byte holds it.
    fn packed(self) -> u8 {
        match self {
            Disposal::Keep => 1 << 2,
            Disposal::Clear => 2 << 2,
        }
    }
}

/// Where a frame's cells go on the canvas, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}
/// The most plays a loop extension can count: its count is a 16-bit number of repeats.
pub const MAX_PLAYS: u32 = 65_536;

fn invalid(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what)
}

/// The bits that number `entries` palette entries, 1 to 8: GIF tables hold `1 << bits`.
pub fn table_bits(entries: usize) -> u8 {
    let mut bits = 1;
    while (1usize << bits) < entries {
        bits += 1;
    }
    bits
}

/// A GIF being written into `out`.
pub struct GifStream<W: Write> {
    out: W,
    held: Vec<u8>,
    size: (u16, u16),
    indexer: Indexer,
    transparent: bool,
    /// Bits an index has as LZW reads it: at least 2, as GIF requires.
    min: u8,
}

/// The palette as the writer looks cells up in it: the clear entry first if there is one.
fn entries(opaque: &[u32], transparent: bool) -> io::Result<Vec<u32>> {
    if opaque.iter().any(|c| c >> 24 != 255) {
        return Err(invalid("a GIF's colours are opaque, with one clear entry"));
    }
    let palette: Vec<u32> = transparent
        .then_some(0)
        .into_iter()
        .chain(opaque.iter().copied())
        .collect();
    if palette.is_empty() || palette.len() > MAX_COLOURS {
        return Err(invalid("a GIF palette holds 1 to 256 entries"));
    }
    Ok(palette)
}

fn side(n: usize) -> io::Result<u16> {
    u16::try_from(n)
        .ok()
        .filter(|&n| n > 0)
        .ok_or_else(|| invalid("a GIF is 1 to 65535 a side"))
}

impl<W: Write> GifStream<W> {
    /// Starts a GIF of `width` by `height` whose cells are the colours `opaque`, each opaque and
    /// different, and clear ones if `transparent`. `plays` is how many times it plays, 0 for
    /// forever; one play writes no loop extension.
    pub fn new(
        out: W,
        (width, height): (usize, usize),
        (opaque, transparent): (&[u32], bool),
        plays: u32,
    ) -> io::Result<Self> {
        let size = (side(width)?, side(height)?);
        if plays > MAX_PLAYS {
            return Err(invalid("a GIF plays at most 65536 times, or forever"));
        }
        let palette = entries(opaque, transparent)?;
        let bits = table_bits(palette.len());
        let mut gif = GifStream {
            out,
            held: Vec::new(),
            size,
            indexer: Indexer::new(&palette)?,
            transparent,
            min: bits.max(2),
        };
        gif.header(&palette, bits, plays);
        Ok(gif)
    }

    fn header(&mut self, palette: &[u32], bits: u8, plays: u32) {
        let (w, h) = self.size;
        self.held.extend_from_slice(b"GIF89a");
        self.held.extend_from_slice(&w.to_le_bytes());
        self.held.extend_from_slice(&h.to_le_bytes());
        // A global table, 8 bits a channel in the source, not sorted, then its size.
        self.held.extend_from_slice(&[0xf0 | (bits - 1), 0, 0]);
        for &c in palette {
            self.held
                .extend_from_slice(&[c as u8, (c >> 8) as u8, (c >> 16) as u8]);
        }
        self.held
            .resize(self.held.len() + 3 * ((1 << bits) - palette.len()), 0);
        if plays != 1 {
            let repeats = plays.saturating_sub(1) as u16;
            self.held
                .extend_from_slice(b"\x21\xff\x0bNETSCAPE2.0\x03\x01");
            self.held.extend_from_slice(&repeats.to_le_bytes());
            self.held.push(0);
        }
    }

    fn flush_held(&mut self) -> io::Result<()> {
        self.out.write_all(&self.held)?;
        self.held.clear();
        Ok(())
    }

    /// The area's corner and size as a GIF holds them, if it is inside the canvas and not
    /// empty.
    fn placed(&self, area: Area) -> io::Result<[u16; 4]> {
        let (w, h) = (usize::from(self.size.0), usize::from(self.size.1));
        let inside = area.width > 0
            && area.height > 0
            && area.x.checked_add(area.width).is_some_and(|r| r <= w)
            && area.y.checked_add(area.height).is_some_and(|b| b <= h);
        if !inside {
            return Err(invalid("a frame's area is not empty and inside the canvas"));
        }
        // Inside a canvas of at most 65535 a side, each fits.
        Ok([area.x, area.y, area.width, area.height].map(|n| n as u16))
    }

    /// Starts a frame in `area`, shown for `delay` hundredths of a second and then left as
    /// `after` says. Clearing needs a clear entry: without one the background is a colour.
    pub fn frame(
        &mut self,
        delay: u16,
        area: Area,
        after: Disposal,
    ) -> io::Result<GifFrame<'_, W>> {
        let placed = self.placed(area)?;
        if after == Disposal::Clear && !self.transparent {
            return Err(invalid("a frame is cleared only to a clear entry"));
        }
        let clear = u8::from(self.transparent);
        self.held
            .extend_from_slice(&[0x21, 0xf9, 4, after.packed() | clear]);
        self.held.extend_from_slice(&delay.to_le_bytes());
        self.held.extend_from_slice(&[0, 0, 0x2c]);
        for n in placed {
            self.held.extend_from_slice(&n.to_le_bytes());
        }
        self.held.extend_from_slice(&[0, self.min]);
        let lzw = Lzw::new(self.min);
        Ok(GifFrame {
            gif: self,
            lzw,
            indices: Vec::new(),
            mapped: Vec::new(),
            width: area.width,
            rows_left: area.height,
        })
    }

    /// Ends the GIF and returns where it was written.
    pub fn finish(mut self) -> io::Result<W> {
        self.held.push(0x3b);
        self.flush_held()?;
        self.out.flush()?;
        Ok(self.out)
    }
}

/// One frame of a [`GifStream`], taking its rows in order.
pub struct GifFrame<'g, W: Write> {
    gif: &'g mut GifStream<W>,
    lzw: Lzw,
    indices: Vec<u8>,
    mapped: Vec<u32>,
    width: usize,
    rows_left: usize,
}

impl<W: Write> GifFrame<'_, W> {
    /// Codes whole rows of the frame's area, straight RGBA cells. A cell not in the palette is
    /// an error, and none of these rows is coded.
    pub fn rows(&mut self, cells: &[u32]) -> io::Result<()> {
        let width = self.width;
        if cells.len() % width != 0 || cells.len() / width > self.rows_left {
            return Err(invalid(
                "rows that are not whole, or more than the image has",
            ));
        }
        self.indices.clear();
        if self.gif.transparent {
            // Every clear cell is the clear entry, whatever colour is stored behind it.
            self.mapped.clear();
            self.mapped
                .extend(cells.iter().map(|&c| if c >> 24 == 0 { 0 } else { c }));
            self.gif.indexer.places(&self.mapped, &mut self.indices)?;
        } else {
            self.gif.indexer.places(cells, &mut self.indices)?;
        }
        self.rows_left -= cells.len() / width;
        self.lzw.push(&self.indices);
        self.blocks(false)
    }

    /// Moves coded bytes into sub-blocks: whole ones, or every one at the end.
    fn blocks(&mut self, last: bool) -> io::Result<()> {
        let whole = if last {
            self.lzw.out.len()
        } else {
            self.lzw.out.len() / BLOCK * BLOCK
        };
        for block in self.lzw.out[..whole].chunks(BLOCK) {
            self.gif.held.push(block.len() as u8);
            self.gif.held.extend_from_slice(block);
        }
        self.lzw.out.drain(..whole);
        if self.gif.held.len() >= FLUSH_AT {
            self.gif.flush_held()?;
        }
        Ok(())
    }

    /// Ends the frame. Every row must have been given.
    pub fn end(mut self) -> io::Result<()> {
        if self.rows_left != 0 {
            return Err(invalid("a frame ended before its last row"));
        }
        self.lzw.finish();
        self.blocks(true)?;
        self.gif.held.push(0);
        Ok(())
    }
}
