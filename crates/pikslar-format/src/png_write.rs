//! Writing a PNG a band of rows at a time, so no copy of the whole image is ever made.

use crate::crc32;
use crate::deflate::Deflater;
use crate::png_index::{plte, trns, Indexer};
use std::io::{self, Write};

pub(crate) const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Compressed data goes out in chunks of at most this size. An image whose compressed data fits
/// in one chunk has the same bytes as when PNGs were written whole.
pub const IDAT_MAX: usize = 1 << 20;

/// The zlib checksum, which may be fed in pieces of any size.
#[derive(Clone, Copy)]
pub(crate) struct Adler {
    a: u32,
    b: u32,
}

impl Default for Adler {
    fn default() -> Self {
        Adler { a: 1, b: 0 }
    }
}

impl Adler {
    pub(crate) fn update(&mut self, data: &[u8]) {
        for block in data.chunks(5552) {
            for &byte in block {
                self.a += byte as u32;
                self.b += self.a;
            }
            self.a %= 65521;
            self.b %= 65521;
        }
    }

    pub(crate) fn value(self) -> u32 {
        (self.b << 16) | self.a
    }
}

pub(crate) fn chunk_to(out: &mut impl Write, kind: &[u8; 4], body: &[u8]) -> io::Result<()> {
    out.write_all(&(body.len() as u32).to_be_bytes())?;
    out.write_all(kind)?;
    out.write_all(body)?;
    out.write_all(&crc32(&[kind, body]).to_be_bytes())
}

/// The zlib header: deflate, 32 KiB window, fastest compression level, no dictionary.
fn zlib_start(expected: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(expected.min(IDAT_MAX + (IDAT_MAX >> 2)));
    v.extend_from_slice(&[0x78, 0x01]);
    v
}

pub(crate) fn invalid(what: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, what)
}

/// Where the compressed data of an image goes, a piece at a time: each piece becomes one chunk.
pub(crate) type Put<'a> = &'a mut dyn FnMut(&[u8]) -> io::Result<()>;

/// The compressed rows of one image, 8-bit RGBA or indexed, and what of them is not yet out.
/// Rows go in top to bottom, any number at a time; memory is the compression window and at
/// most one chunk of output.
pub(crate) struct ImageData {
    width: usize,
    height: usize,
    rows: usize,
    /// For an indexed image, its palette; for RGBA, none.
    index: Option<Indexer>,
    z: Deflater,
    adler: Adler,
    /// Compressed bytes not yet given out. Starts with the zlib header.
    pub(crate) pending: Vec<u8>,
}

impl ImageData {
    /// The data of an image `width` by `height`, a size already checked.
    pub(crate) fn new((width, height): (usize, usize), index: Option<Indexer>) -> Self {
        let row = index.as_ref().map_or(width * 4, |ix| ix.row_bytes(width));
        let raw = (row + 1).saturating_mul(height);
        ImageData {
            width,
            height,
            rows: 0,
            index,
            z: Deflater::expecting(raw),
            adler: Adler::default(),
            pending: zlib_start(raw / 4 + 64),
        }
    }

    /// Adds whole rows of straight RGBA cells, giving `put` each chunk's worth that is ready.
    /// For an indexed image a cell not in its palette is an error, and none of these rows is
    /// added.
    pub(crate) fn rows(&mut self, px: &[u32], put: Put<'_>) -> io::Result<()> {
        let count = px.len() / self.width;
        if px.len() % self.width != 0 || self.rows + count > self.height {
            return Err(invalid("rows that do not fit the image"));
        }
        let width = self.width;
        let (adler, index) = (&mut self.adler, &mut self.index);
        let mut refused = Ok(());
        self.z.write_with(
            |held| {
                let start = held.len();
                let row = index.as_ref().map_or(width * 4, |ix| ix.row_bytes(width));
                held.reserve(row * count + count);
                for row in px.chunks_exact(width) {
                    held.push(0);
                    match index {
                        None => row
                            .iter()
                            .for_each(|p| held.extend_from_slice(&p.to_le_bytes())),
                        Some(ix) => refused = ix.pack(row, held),
                    }
                    if refused.is_err() {
                        held.truncate(start);
                        return;
                    }
                }
            },
            |added| adler.update(added),
        );
        refused?;
        self.rows += count;
        self.z.take(&mut self.pending);
        while self.pending.len() > IDAT_MAX {
            self.give(IDAT_MAX, put)?;
        }
        Ok(())
    }

    fn give(&mut self, len: usize, put: Put<'_>) -> io::Result<()> {
        put(&self.pending[..len])?;
        self.pending.drain(..len);
        Ok(())
    }

    /// Ends the data once every row is in, giving `put` the rest, in at least one piece.
    pub(crate) fn finish(mut self, put: Put<'_>) -> io::Result<()> {
        if self.rows != self.height {
            return Err(invalid("the image ended before its last row"));
        }
        let tail = std::mem::take(&mut self.z).finish();
        self.pending.extend_from_slice(&tail);
        self.pending
            .extend_from_slice(&self.adler.value().to_be_bytes());
        while self.pending.len() > IDAT_MAX {
            self.give(IDAT_MAX, put)?;
        }
        self.give(self.pending.len(), put)
    }
}

/// The largest side a PNG holds: its integers are at most 2^31 - 1.
pub(crate) const MOST_SIDE: usize = i32::MAX as usize;

/// Whether an image of this size can be written: each side from 1 to what a PNG holds.
pub(crate) fn check_size((width, height): (usize, usize)) -> io::Result<()> {
    match (1..=MOST_SIDE).contains(&width) && (1..=MOST_SIDE).contains(&height) {
        true => Ok(()),
        false => Err(invalid("an image needs a width and a height")),
    }
}

/// The IHDR body of an image of colour type `kind` (6 RGBA, 3 indexed) at `depth` bits.
pub(crate) fn header((width, height): (usize, usize), [depth, kind]: [u8; 2]) -> Vec<u8> {
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    header.extend_from_slice(&[depth, kind, 0, 0, 0]);
    header
}

/// A PNG being written, 8-bit RGBA or indexed. Rows go in top to bottom, any number at a time;
/// memory is one band of rows, the compression window and at most one chunk of output.
pub struct PngStream<W: Write> {
    out: W,
    pub(crate) data: ImageData,
}

impl<W: Write> PngStream<W> {
    /// Writes the signature and header of an 8-bit RGBA image.
    pub fn new(out: W, width: usize, height: usize) -> io::Result<Self> {
        Self::start(out, (width, height), None)
    }

    /// Writes the signature, header and palette of an indexed image: colour type 3, as few bits
    /// a cell as `palette` needs, and a tRNS chunk if any entry is not opaque. Every cell given
    /// later must be one of the palette's straight RGBA colours, exactly.
    pub fn indexed(out: W, width: usize, height: usize, palette: &[u32]) -> io::Result<Self> {
        Self::start(
            out,
            (width, height),
            Some((Indexer::new(palette)?, palette)),
        )
    }

    fn start(
        mut out: W,
        size: (usize, usize),
        index: Option<(Indexer, &[u32])>,
    ) -> io::Result<Self> {
        check_size(size)?;
        let kind = match &index {
            None => [8, 6],
            Some((ix, _)) => [ix.depth(), 3],
        };
        out.write_all(&SIGNATURE)?;
        chunk_to(&mut out, b"IHDR", &header(size, kind))?;
        if let Some((_, palette)) = &index {
            chunk_to(&mut out, b"PLTE", &plte(palette))?;
            if let Some(alphas) = trns(palette) {
                chunk_to(&mut out, b"tRNS", &alphas)?;
            }
        }
        Ok(PngStream {
            out,
            data: ImageData::new(size, index.map(|(ix, _)| ix)),
        })
    }

    /// Adds whole rows of straight RGBA cells. For an indexed image a cell not in its palette
    /// is an error, and none of these rows is added.
    pub fn rows(&mut self, px: &[u32]) -> io::Result<()> {
        let out = &mut self.out;
        self.data.rows(px, &mut |part| chunk_to(out, b"IDAT", part))
    }

    /// Ends the image once every row is in, and gives back the writer.
    pub fn finish(mut self) -> io::Result<W> {
        let out = &mut self.out;
        self.data.finish(&mut |part| chunk_to(out, b"IDAT", part))?;
        chunk_to(&mut self.out, b"IEND", &[])?;
        self.out.flush()?;
        Ok(self.out)
    }
}
