//! PNG reading and writing. Images come in and go out as straight-alpha RGBA cells.

use crate::inflate::{inflate_until, STOP_EVERY_BYTES};
use crate::png_write::{PngStream, SIGNATURE};
use crate::{crc32, FormatError};
use pikslar_core::{rgba, MAX_DIMENSION};
const CORRUPT_CHUNK: FormatError = FormatError::Corrupt("bad PNG chunk");

#[derive(Debug, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub px: Vec<u32>,
}

#[derive(Clone, Copy, Default)]
struct Header {
    width: usize,
    height: usize,
    depth: u8,
    color: u8,
    interlaced: bool,
}

/// Start column, start row, column step and row step of the seven Adam7 passes.
const ADAM7: [(usize, usize, usize, usize); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

impl Header {
    fn parse(d: &[u8]) -> Result<Header, FormatError> {
        if d.len() != 13 {
            return Err(CORRUPT_CHUNK);
        }
        let dim = |i: usize| u32::from_be_bytes([d[i], d[i + 1], d[i + 2], d[i + 3]]) as usize;
        let header = Header {
            width: dim(0),
            height: dim(4),
            depth: d[8],
            color: d[9],
            interlaced: d[12] == 1,
        };
        if header.width == 0 || header.height == 0 {
            return Err(FormatError::Corrupt("image has no pixels"));
        }
        if header.width > MAX_DIMENSION || header.height > MAX_DIMENSION {
            return Err(FormatError::TooLarge);
        }
        if d[10] != 0 || d[11] != 0 {
            return Err(FormatError::Corrupt("unknown PNG compression"));
        }
        if d[12] > 1 {
            return Err(FormatError::Corrupt("unknown PNG interlace method"));
        }
        let valid = matches!(
            (header.color, header.depth),
            (0, 1 | 2 | 4 | 8 | 16) | (3, 1 | 2 | 4 | 8) | (2 | 4 | 6, 8 | 16)
        );
        if !valid {
            return Err(FormatError::Corrupt("invalid PNG color format"));
        }
        Ok(header)
    }

    fn channels(&self) -> usize {
        match self.color {
            2 => 3,
            4 => 2,
            6 => 4,
            _ => 1,
        }
    }

    fn row_bytes(&self) -> usize {
        (self.width * self.channels() * self.depth as usize).div_ceil(8)
    }

    fn pixel_bytes(&self) -> usize {
        (self.channels() * self.depth as usize).div_ceil(8)
    }

    /// Bytes of filtered image data for this (sub)image, including filter bytes.
    fn stream_bytes(&self) -> usize {
        (self.row_bytes() + 1) * self.height
    }

    /// The reduced image of one interlace pass, or `None` if the pass is empty.
    fn pass(&self, pass: (usize, usize, usize, usize)) -> Option<Header> {
        let (x0, y0, dx, dy) = pass;
        let width = self.width.saturating_sub(x0).div_ceil(dx);
        let height = self.height.saturating_sub(y0).div_ceil(dy);
        (width > 0 && height > 0).then_some(Header {
            width,
            height,
            ..*self
        })
    }
}

#[derive(Default)]
struct Chunks {
    header: Option<Header>,
    palette: Vec<[u8; 3]>,
    transparency: Vec<u8>,
    data: Vec<u8>,
}

fn read_chunks(bytes: &[u8]) -> Result<Chunks, FormatError> {
    if bytes.len() < 8 || bytes[..8] != SIGNATURE {
        return Err(FormatError::Corrupt("not a PNG file"));
    }
    let mut chunks = Chunks::default();
    let mut pos = 8;
    loop {
        let head = bytes.get(pos..pos + 8).ok_or(CORRUPT_CHUNK)?;
        let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let kind = &head[4..8];
        let end = pos
            .checked_add(12)
            .and_then(|p| p.checked_add(len))
            .ok_or(CORRUPT_CHUNK)?;
        let body = bytes.get(pos + 8..pos + 8 + len).ok_or(CORRUPT_CHUNK)?;
        let stored = bytes.get(end - 4..end).ok_or(CORRUPT_CHUNK)?;
        if u32::from_be_bytes([stored[0], stored[1], stored[2], stored[3]]) != crc32(&[kind, body])
        {
            return Err(FormatError::Corrupt("PNG checksum mismatch"));
        }
        match kind {
            b"IHDR" => chunks.header = Some(Header::parse(body)?),
            b"PLTE" => chunks.palette = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
            b"tRNS" => chunks.transparency = body.to_vec(),
            b"IDAT" => chunks.data.extend_from_slice(body),
            b"IEND" => return Ok(chunks),
            _ => {}
        }
        pos = end;
    }
}

#[inline]
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (a as i32, b as i32, c as i32);
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Asks whether to stop each time the rows worked through pass another
/// [`STOP_EVERY_BYTES`].
struct RowStop<'s> {
    ask: &'s dyn Fn() -> bool,
    done: usize,
    next: usize,
}

impl<'s> RowStop<'s> {
    fn new(ask: &'s dyn Fn() -> bool) -> Self {
        RowStop {
            ask,
            done: 0,
            next: STOP_EVERY_BYTES,
        }
    }

    /// Counts a row of `bytes` about to be worked through.
    #[inline]
    fn row(&mut self, bytes: usize) -> Result<(), FormatError> {
        self.done = self.done.saturating_add(bytes);
        if self.done < self.next {
            return Ok(());
        }
        self.next = self.done.saturating_add(STOP_EVERY_BYTES);
        match (self.ask)() {
            true => Err(FormatError::Stopped),
            false => Ok(()),
        }
    }
}

/// Reverses PNG scanline filtering in place. Each row keeps its filter byte in front, so no
/// second copy of the image is made: at 8192 by 8192 in 16-bit colour that copy was 512 MiB.
fn unfilter(raw: &mut [u8], header: &Header, stop: &mut RowStop<'_>) -> Result<(), FormatError> {
    let stride = header.row_bytes() + 1;
    let bpp = header.pixel_bytes();
    for y in 0..header.height {
        stop.row(stride)?;
        let (before, rest) = raw.split_at_mut(y * stride);
        let above = match y {
            0 => &[][..],
            _ => &before[(y - 1) * stride + 1..],
        };
        let (filter, current) = rest[..stride].split_at_mut(1);
        unfilter_row(filter[0], current, above, bpp)?;
    }
    Ok(())
}

fn unfilter_row(
    filter: u8,
    current: &mut [u8],
    above: &[u8],
    bpp: usize,
) -> Result<(), FormatError> {
    for x in 0..current.len() {
        let a = if x >= bpp { current[x - bpp] } else { 0 };
        let b = above.get(x).copied().unwrap_or(0);
        let c = if x >= bpp {
            above.get(x - bpp).copied().unwrap_or(0)
        } else {
            0
        };
        let predicted = match filter {
            0 => 0,
            1 => a,
            2 => b,
            3 => ((a as u16 + b as u16) / 2) as u8,
            4 => paeth(a, b, c),
            _ => return Err(FormatError::Corrupt("bad PNG filter")),
        };
        current[x] = current[x].wrapping_add(predicted);
    }
    Ok(())
}

/// The unfiltered rows of a stream `unfilter` has been through, without their filter bytes.
fn rows<'a>(raw: &'a [u8], header: &Header) -> impl Iterator<Item = &'a [u8]> {
    raw.chunks_exact(header.row_bytes() + 1)
        .map(|line| &line[1..])
}

/// Reads sample `i` of a row, scaled to 8 bits unless `raw` is set.
#[inline]
fn sample(row: &[u8], i: usize, depth: u8, raw: bool) -> u8 {
    match depth {
        8 => row[i],
        16 => row[i * 2],
        d => {
            let bit = i * d as usize;
            let shift = 8 - d as usize - bit % 8;
            let value = (row[bit / 8] >> shift) & ((1 << d) - 1);
            if raw {
                value
            } else {
                (value as u32 * 255 / ((1u32 << d) - 1)) as u8
            }
        }
    }
}

fn transparent_key(chunks: &Chunks, index: usize) -> Option<u8> {
    chunks.transparency.get(index * 2 + 1).copied()
}

fn convert_row(
    row: &[u8],
    h: &Header,
    chunks: &Chunks,
    out: &mut [u32],
) -> Result<(), FormatError> {
    let (depth, trns) = (h.depth, !chunks.transparency.is_empty());
    for (x, px) in out.iter_mut().enumerate() {
        *px = match h.color {
            0 => {
                let v = sample(row, x, depth, false);
                let raw = sample(row, x, depth, true);
                let hidden = trns && transparent_key(chunks, 0) == Some(raw);
                rgba(v, v, v, if hidden { 0 } else { 255 })
            }
            2 => {
                let [r, g, b] = [0, 1, 2].map(|c| sample(row, x * 3 + c, depth, false));
                let t = &chunks.transparency;
                let hidden = t.len() >= 6 && [t[1], t[3], t[5]] == [r, g, b];
                rgba(r, g, b, if hidden { 0 } else { 255 })
            }
            3 => {
                let i = sample(row, x, depth, true) as usize;
                let [r, g, b] = *chunks
                    .palette
                    .get(i)
                    .ok_or(FormatError::Corrupt("bad palette index"))?;
                rgba(r, g, b, chunks.transparency.get(i).copied().unwrap_or(255))
            }
            4 => {
                let v = sample(row, x * 2, depth, false);
                rgba(v, v, v, sample(row, x * 2 + 1, depth, false))
            }
            _ => {
                let [r, g, b, a] = [0, 1, 2, 3].map(|c| sample(row, x * 4 + c, depth, false));
                rgba(r, g, b, a)
            }
        };
    }
    Ok(())
}

fn decode_progressive(
    raw: &mut [u8],
    h: &Header,
    chunks: &Chunks,
    stop: &mut RowStop<'_>,
) -> Result<Vec<u32>, FormatError> {
    let mut px = vec![0u32; h.width * h.height];
    let mut offset = 0;
    for pass in ADAM7 {
        let Some(sub) = h.pass(pass) else { continue };
        let (x0, y0, dx, dy) = pass;
        let pass_bytes = &mut raw[offset..offset + sub.stream_bytes()];
        unfilter(pass_bytes, &sub, stop)?;
        offset += sub.stream_bytes();
        let mut line = vec![0u32; sub.width];
        for (y, row) in rows(pass_bytes, &sub).enumerate() {
            stop.row(row.len())?;
            convert_row(row, &sub, chunks, &mut line)?;
            let target = (y0 + y * dy) * h.width;
            for (x, &value) in line.iter().enumerate() {
                px[target + x0 + x * dx] = value;
            }
        }
    }
    Ok(px)
}

fn decode_sequential(
    raw: &mut [u8],
    h: &Header,
    chunks: &Chunks,
    stop: &mut RowStop<'_>,
) -> Result<Vec<u32>, FormatError> {
    unfilter(raw, h, stop)?;
    let mut px = vec![0u32; h.width * h.height];
    for (row, out) in rows(raw, h).zip(px.chunks_exact_mut(h.width)) {
        stop.row(row.len())?;
        convert_row(row, h, chunks, out)?;
    }
    Ok(px)
}

/// The width and height a PNG says it has, read from its header alone, so a caller can refuse an
/// image before decoding it. The header is checked as [`decode`] checks it.
pub fn dimensions(bytes: &[u8]) -> Result<(usize, usize), FormatError> {
    if bytes.len() < 8 || bytes[..8] != SIGNATURE {
        return Err(FormatError::Corrupt("not a PNG file"));
    }
    // The header must be the first chunk: its length, its name, 13 bytes and a checksum.
    let chunk = bytes.get(8..33).ok_or(CORRUPT_CHUNK)?;
    let (kind, body) = (&chunk[4..8], &chunk[8..21]);
    let stored = u32::from_be_bytes([chunk[21], chunk[22], chunk[23], chunk[24]]);
    if chunk[..4] != 13u32.to_be_bytes() || kind != b"IHDR" || stored != crc32(&[kind, body]) {
        return Err(CORRUPT_CHUNK);
    }
    let header = Header::parse(body)?;
    Ok((header.width, header.height))
}

/// Decodes a PNG file.
pub fn decode(bytes: &[u8]) -> Result<Image, FormatError> {
    decode_until(bytes, &|| false)
}

/// As [`decode`], asking `stop` each mebibyte of image data unpacked
/// ([`crate::inflate::STOP_EVERY_BYTES`]), unfiltered and turned into pixels. Once it answers
/// true the decode ends with [`FormatError::Stopped`] and what was made is let go.
pub fn decode_until(bytes: &[u8], stop: &dyn Fn() -> bool) -> Result<Image, FormatError> {
    let chunks = read_chunks(bytes)?;
    let h = chunks.header.ok_or(CORRUPT_CHUNK)?;
    if chunks.data.len() < 6 {
        return Err(FormatError::Corrupt("PNG has no image data"));
    }
    let expected = if h.interlaced {
        ADAM7
            .iter()
            .filter_map(|&p| h.pass(p))
            .map(|s| s.stream_bytes())
            .sum()
    } else {
        h.stream_bytes()
    };
    let (mut raw, _) = inflate_until(&chunks.data[2..], expected, stop)?;
    if raw.len() != expected {
        return Err(FormatError::Corrupt("PNG image data is the wrong size"));
    }
    let mut asker = RowStop::new(stop);
    let px = if h.interlaced {
        decode_progressive(&mut raw, &h, &chunks, &mut asker)?
    } else {
        decode_sequential(&mut raw, &h, &chunks, &mut asker)?
    };
    Ok(Image {
        width: h.width,
        height: h.height,
        px,
    })
}

/// Rows handed to the encoder at a time, so the whole image is never copied.
const BAND_ROWS: usize = 64;

/// Encodes straight-alpha RGBA as an 8-bit RGBA PNG.
pub fn encode(image: &Image) -> Result<Vec<u8>, FormatError> {
    if image.px.len() != image.width * image.height {
        return Err(FormatError::Corrupt("image cells do not match its size"));
    }
    let mut png = PngStream::new(Vec::new(), image.width, image.height)?;
    for band in image.px.chunks(image.width * BAND_ROWS) {
        png.rows(band)?;
    }
    Ok(png.finish()?)
}

#[cfg(test)]
#[path = "png_stop_tests.rs"]
mod stop_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deflate::deflate;
    use crate::png_write::IDAT_MAX;

    pub(super) fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out.extend_from_slice(&crc32(&[kind, body]).to_be_bytes());
    }

    fn adler32(data: &[u8]) -> u32 {
        let mut a = crate::png_write::Adler::default();
        a.update(data);
        a.value()
    }

    /// The encoder as it was before it streamed: one IDAT holding the whole stream.
    fn encode_whole(image: &Image) -> Vec<u8> {
        let mut raw = Vec::with_capacity((image.width * 4 + 1) * image.height);
        for row in image.px.chunks_exact(image.width) {
            raw.push(0);
            for px in row {
                raw.extend_from_slice(&px.to_le_bytes());
            }
        }
        let mut stream = vec![0x78, 0x01];
        stream.extend_from_slice(&deflate(&raw));
        stream.extend_from_slice(&adler32(&raw).to_be_bytes());
        let mut out = SIGNATURE.to_vec();
        let mut header = Vec::with_capacity(13);
        header.extend_from_slice(&(image.width as u32).to_be_bytes());
        header.extend_from_slice(&(image.height as u32).to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        write_chunk(&mut out, b"IHDR", &header);
        write_chunk(&mut out, b"IDAT", &stream);
        write_chunk(&mut out, b"IEND", &[]);
        out
    }

    fn build(
        w: u32,
        h: u32,
        depth: u8,
        color: u8,
        extra: &[(&[u8; 4], &[u8])],
        rows: &[u8],
    ) -> Vec<u8> {
        let mut out = SIGNATURE.to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[depth, color, 0, 0, 0]);
        write_chunk(&mut out, b"IHDR", &ihdr);
        for (kind, body) in extra {
            write_chunk(&mut out, kind, body);
        }
        let mut stream = vec![0x78, 0x01];
        stream.extend_from_slice(&deflate(rows));
        stream.extend_from_slice(&adler32(rows).to_be_bytes());
        write_chunk(&mut out, b"IDAT", &stream);
        write_chunk(&mut out, b"IEND", &[]);
        out
    }

    const RED: u32 = rgba(255, 0, 0, 255);
    const BLUE: u32 = rgba(0, 0, 255, 255);

    #[test]
    fn our_output_reads_back_exactly() {
        let px = vec![RED, 0, BLUE, rgba(9, 8, 7, 100), RED, RED];
        let image = Image {
            width: 3,
            height: 2,
            px,
        };
        assert_eq!(
            decode(&encode(&image).expect("encodes")).expect("valid"),
            image
        );
    }

    #[test]
    fn indexed_with_transparency() {
        let plte: &[u8] = &[255, 0, 0, 0, 0, 255];
        let trns: &[u8] = &[255, 0];
        let png = build(
            2,
            1,
            1,
            3,
            &[(b"PLTE", plte), (b"tRNS", trns)],
            &[0, 0b0100_0000],
        );
        assert_eq!(decode(&png).expect("valid").px, vec![RED, 0]);
    }

    #[test]
    fn grayscale_scales_low_bit_depths() {
        let png = build(2, 1, 2, 0, &[], &[0, 0b0011_0000]);
        assert_eq!(
            decode(&png).expect("valid").px,
            vec![rgba(0, 0, 0, 255), rgba(255, 255, 255, 255)]
        );
    }

    #[test]
    fn sixteen_bit_keeps_the_high_byte() {
        let png = build(1, 1, 16, 2, &[], &[0, 0x12, 0x99, 0x34, 0x99, 0x56, 0x99]);
        assert_eq!(
            decode(&png).expect("valid").px,
            vec![rgba(0x12, 0x34, 0x56, 255)]
        );
    }

    #[test]
    fn all_filters_reverse_correctly() {
        // Two RGB pixels per row; expected row is [10,20,30, 40,50,60] each time.
        let rows = [
            0u8, 10, 20, 30, 40, 50, 60, // none
            1, 10, 20, 30, 30, 30, 30, // sub
            2, 0, 0, 0, 0, 0, 0, // up
            3, 5, 10, 15, 15, 15, 15, // average
            4, 0, 0, 0, 0, 0, 0, // paeth
        ];
        let png = build(2, 5, 8, 2, &[], &rows);
        let px = decode(&png).expect("valid").px;
        assert!(
            px.chunks(2)
                .all(|r| r == [rgba(10, 20, 30, 255), rgba(40, 50, 60, 255)]),
            "{px:x?}"
        );
    }

    #[test]
    fn rejects_bad_files_with_errors() {
        assert!(decode(b"not a png").is_err());
        let good = encode(&Image {
            width: 1,
            height: 1,
            px: vec![RED],
        })
        .expect("encodes");
        for cut in 8..good.len() {
            assert!(decode(&good[..cut]).is_err(), "truncated at {cut}");
        }
        let mut flipped = good.clone();
        flipped[20] ^= 1;
        assert!(matches!(decode(&flipped), Err(FormatError::Corrupt(_))));
    }

    #[test]
    fn rejects_oversized_images() {
        let big = build(MAX_DIMENSION as u32 + 1, 1, 8, 6, &[], &[]);
        assert!(matches!(decode(&big), Err(FormatError::TooLarge)));
    }

    #[test]
    fn interlaced_passes_land_on_the_right_pixels() {
        // In a 2x1 image, pixel 0 arrives in pass 1 and pixel 1 in pass 6.
        let rows = [0u8, 255, 0, 0, 255, 0, 0, 0, 255, 255];
        let mut png = build(2, 1, 8, 6, &[], &rows);
        png[8 + 8 + 12] = 1;
        let crc = crc32(&[&png[12..29]]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());
        assert_eq!(decode(&png).expect("valid").px, vec![RED, BLUE]);
    }

    #[test]
    fn palette_index_out_of_range_is_an_error() {
        let plte: &[u8] = &[255, 0, 0];
        let png = build(1, 1, 8, 3, &[(b"PLTE", plte)], &[0, 5]);
        assert!(decode(&png).is_err());
    }

    /// Noise in most rows, flat colour in some, so the compressed data runs past one chunk.
    fn picture(width: usize, height: usize, seed: u32) -> Image {
        let mut x = seed;
        let px = (0..width * height)
            .map(|i| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                if i / width % 7 == 0 {
                    rgba(10, 20, 30, 255)
                } else {
                    x
                }
            })
            .collect();
        Image { width, height, px }
    }

    /// The chunks after the signature, as (kind, body).
    fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        let mut at = 8;
        let mut out = Vec::new();
        while at < png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().expect("len")) as usize;
            let kind = png[at + 4..at + 8].try_into().expect("kind");
            out.push((kind, png[at + 8..at + 8 + len].to_vec()));
            at += 12 + len;
        }
        out
    }

    #[test]
    fn images_that_fit_one_chunk_keep_their_old_bytes() {
        for (w, h) in [(1, 1), (3, 2), (64, 65), (200, 130)] {
            let image = picture(w, h, w as u32);
            assert_eq!(
                encode(&image).expect("encodes"),
                encode_whole(&image),
                "{w}x{h}"
            );
        }
    }

    #[test]
    fn larger_images_split_the_same_stream_across_chunks() {
        let image = picture(700, 600, 3);
        let new = encode(&image).expect("encodes");
        let parts = chunks(&new);
        let data: Vec<_> = parts.iter().filter(|(k, _)| k == b"IDAT").collect();
        assert!(data.len() > 1, "the test needs more than one chunk");
        assert!(data.iter().all(|(_, b)| b.len() <= IDAT_MAX));
        assert!(data[..data.len() - 1]
            .iter()
            .all(|(_, b)| b.len() == IDAT_MAX));
        let joined: Vec<u8> = data.iter().flat_map(|(_, b)| b.clone()).collect();
        let old = chunks(&encode_whole(&image));
        assert_eq!(joined, old[1].1, "one stream, cut into chunks");
        let back = decode(&new).expect("valid");
        assert_eq!(back, decode(&encode_whole(&image)).expect("valid"));
        assert_eq!((back.width, back.height), (700, 600));
    }

    #[test]
    fn chunks_are_written_while_rows_arrive_not_held_to_the_end() {
        let image = picture(700, 600, 3);
        let mut png = PngStream::new(Vec::new(), 700, 600).expect("start");
        let mut most_held = 0;
        for band in image.px.chunks(700 * 16) {
            png.rows(band).expect("rows");
            most_held = most_held.max(png.data.pending.len());
        }
        assert!(most_held <= IDAT_MAX, "{most_held} bytes held back");
        png.finish().expect("finish");
    }

    #[test]
    fn the_checksum_holds_over_long_runs_of_the_largest_byte() {
        let data = vec![0xffu8; 100_000];
        let (mut a, mut b) = (1u64, 0u64);
        for &x in &data {
            a = (a + x as u64) % 65521;
            b = (b + a) % 65521;
        }
        let mut sum = crate::png_write::Adler::default();
        sum.update(&data);
        assert_eq!(sum.value() as u64, (b << 16) | a);
    }

    #[test]
    fn rows_may_arrive_in_any_number_at_a_time() {
        let image = picture(37, 50, 5);
        let whole = encode(&image).expect("encodes");
        for band in [1, 2, 16, 49, 50] {
            let mut png = PngStream::new(Vec::new(), 37, 50).expect("start");
            for rows in image.px.chunks(37 * band) {
                png.rows(rows).expect("rows");
            }
            assert_eq!(
                png.finish().expect("finish"),
                whole,
                "{band} rows at a time"
            );
        }
    }

    #[test]
    fn a_stream_refuses_rows_that_do_not_fit() {
        assert!(PngStream::new(Vec::new(), 0, 1).is_err());
        assert!(PngStream::new(Vec::new(), 1, 0).is_err());
        let mut png = PngStream::new(Vec::new(), 2, 2).expect("start");
        assert!(png.rows(&[RED; 3]).is_err(), "half a row");
        png.rows(&[RED; 2]).expect("one row");
        assert!(png.rows(&[RED; 4]).is_err(), "one row too many");
        assert!(png.finish().is_err(), "a row missing");
        let wrong = Image {
            width: 2,
            height: 2,
            px: vec![RED; 3],
        };
        assert!(encode(&wrong).is_err());
    }
}
