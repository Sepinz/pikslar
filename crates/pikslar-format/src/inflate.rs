//! DEFLATE decompression (RFC 1951) with a hard limit on output size, and a question asked
//! as it goes whether to stop.

use crate::FormatError;

const CORRUPT: FormatError = FormatError::Corrupt("bad compressed data");
const MAX_BITS: usize = 15;

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CODE_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buffer: u64,
    count: u32,
}

impl Bits<'_> {
    #[inline]
    fn refill(&mut self) {
        while self.count <= 56 && self.pos < self.data.len() {
            self.buffer |= (self.data[self.pos] as u64) << self.count;
            self.pos += 1;
            self.count += 8;
        }
    }

    #[inline]
    fn take(&mut self, n: u32) -> Result<u32, FormatError> {
        if n == 0 {
            return Ok(0);
        }
        self.refill();
        if self.count < n {
            return Err(CORRUPT);
        }
        let value = (self.buffer & ((1u64 << n) - 1)) as u32;
        self.buffer >>= n;
        self.count -= n;
        Ok(value)
    }

    /// Discards bits up to the next byte boundary.
    fn align(&mut self) {
        let extra = self.count % 8;
        self.buffer >>= extra;
        self.count -= extra;
    }
}

/// A canonical Huffman decoder, decoding one bit at a time from the code counts.
struct Huffman {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, FormatError> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        let mut left = 1i32;
        for &count in &counts[1..] {
            left = left * 2 - count as i32;
            if left < 0 {
                return Err(CORRUPT);
            }
        }
        let mut offsets = [0u16; MAX_BITS + 2];
        for i in 1..=MAX_BITS {
            offsets[i + 1] = offsets[i] + counts[i];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &l) in lengths.iter().enumerate().filter(|(_, &l)| l != 0) {
            symbols[offsets[l as usize] as usize] = symbol as u16;
            offsets[l as usize] += 1;
        }
        Ok(Huffman { counts, symbols })
    }

    #[inline]
    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, FormatError> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for length in 1..=MAX_BITS {
            code |= bits.take(1)? as i32;
            let count = self.counts[length] as i32;
            if code - first < count {
                return self
                    .symbols
                    .get((index + code - first) as usize)
                    .copied()
                    .ok_or(CORRUPT);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(CORRUPT)
    }
}

fn fixed_tables() -> Result<(Huffman, Huffman), FormatError> {
    let mut lit = [8u8; 288];
    lit[144..256].fill(9);
    lit[256..280].fill(7);
    Ok((Huffman::new(&lit)?, Huffman::new(&[5u8; 30])?))
}

fn dynamic_tables(bits: &mut Bits<'_>) -> Result<(Huffman, Huffman), FormatError> {
    let lit_count = bits.take(5)? as usize + 257;
    let dist_count = bits.take(5)? as usize + 1;
    let code_count = bits.take(4)? as usize + 4;
    if lit_count > 286 || dist_count > 30 {
        return Err(CORRUPT);
    }
    let mut code_lengths = [0u8; 19];
    for &slot in &CODE_ORDER[..code_count] {
        code_lengths[slot] = bits.take(3)? as u8;
    }
    let code = Huffman::new(&code_lengths)?;
    let total = lit_count + dist_count;
    let mut lengths = [0u8; 320];
    let mut i = 0;
    while i < total {
        let (value, repeat) = match code.decode(bits)? {
            n @ 0..=15 => (n as u8, 1),
            16 if i == 0 => return Err(CORRUPT),
            16 => (lengths[i - 1], 3 + bits.take(2)? as usize),
            17 => (0, 3 + bits.take(3)? as usize),
            18 => (0, 11 + bits.take(7)? as usize),
            _ => return Err(CORRUPT),
        };
        if i + repeat > total {
            return Err(CORRUPT);
        }
        lengths[i..i + repeat].fill(value);
        i += repeat;
    }
    Ok((
        Huffman::new(&lengths[..lit_count])?,
        Huffman::new(&lengths[lit_count..total])?,
    ))
}

/// Grows like a doubling vector, but never past `limit`: at the size of a large image, the last
/// doubling would double the memory used. When a doubling would leave less than half the length
/// again to the limit, it grows to the limit at once: a stream just past a power of two would
/// otherwise grow a last time by a few bytes, and that copy holds the whole output twice.
#[inline]
fn make_room(out: &mut Vec<u8>, limit: usize, needed: usize) {
    if out.capacity() - out.len() < needed {
        let len = out.len();
        let room = limit.saturating_sub(len);
        let more = match room <= len + len / 2 {
            true => room,
            false => len,
        };
        out.reserve_exact(more.max(needed));
    }
}

/// How many bytes of output are made between asking whether to stop. A release build makes a
/// mebibyte in a few milliseconds even from the slowest streams, so a stop waits about that.
pub const STOP_EVERY_BYTES: usize = 1 << 20;

/// Asks whether to stop each time the output passes another [`STOP_EVERY_BYTES`].
struct Stop<'s> {
    ask: &'s dyn Fn() -> bool,
    next: usize,
}

impl Stop<'_> {
    /// Asks now, whatever has been made.
    fn now(&mut self, made: usize) -> Result<(), FormatError> {
        if (self.ask)() {
            return Err(FormatError::Stopped);
        }
        self.next = made.saturating_add(STOP_EVERY_BYTES);
        Ok(())
    }

    /// Asks if `made` bytes pass the next mark.
    #[inline]
    fn check(&mut self, made: usize) -> Result<(), FormatError> {
        match made >= self.next {
            true => self.now(made),
            false => Ok(()),
        }
    }
}

fn copy_stored(bits: &mut Bits<'_>, out: &mut Vec<u8>, limit: usize) -> Result<(), FormatError> {
    bits.align();
    let len = bits.take(16)? as usize;
    let complement = bits.take(16)? as usize;
    if len != !complement & 0xffff || out.len() + len > limit {
        return Err(CORRUPT);
    }
    make_room(out, limit, len);
    for _ in 0..len {
        out.push(bits.take(8)? as u8);
    }
    Ok(())
}

fn inflate_block(
    bits: &mut Bits<'_>,
    out: &mut Vec<u8>,
    limit: usize,
    (lit, dist): (&Huffman, &Huffman),
    stop: &mut Stop<'_>,
) -> Result<(), FormatError> {
    loop {
        let symbol = lit.decode(bits)? as usize;
        match symbol {
            0..=255 if out.len() < limit => {
                make_room(out, limit, 1);
                out.push(symbol as u8);
                stop.check(out.len())?;
            }
            256 => return Ok(()),
            257..=285 => {
                let i = symbol - 257;
                let len = LENGTH_BASE[i] as usize + bits.take(LENGTH_EXTRA[i] as u32)? as usize;
                let code = dist.decode(bits)? as usize;
                if code >= 30 {
                    return Err(CORRUPT);
                }
                let back = DIST_BASE[code] as usize + bits.take(DIST_EXTRA[code] as u32)? as usize;
                if back > out.len() || out.len() + len > limit {
                    return Err(CORRUPT);
                }
                make_room(out, limit, len);
                let from = out.len() - back;
                for k in 0..len {
                    out.push(out[from + k]);
                }
                stop.check(out.len())?;
            }
            _ => return Err(CORRUPT),
        }
    }
}

/// Decompresses a raw DEFLATE stream, failing if it would exceed `limit` bytes.
pub fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, FormatError> {
    inflate_counting(data, limit).map(|(out, _)| out)
}

/// Like [`inflate`], and also says how many bytes of `data` the stream took, so that what
/// follows it (the checksum of a zlib stream) can be found.
pub fn inflate_counting(data: &[u8], limit: usize) -> Result<(Vec<u8>, usize), FormatError> {
    inflate_until(data, limit, &|| false)
}

/// Like [`inflate_counting`], asking `stop` before each block and each time the output passes
/// another [`STOP_EVERY_BYTES`]; once it answers true the stream is let go and the answer is
/// [`FormatError::Stopped`].
pub fn inflate_until(
    data: &[u8],
    limit: usize,
    stop: &dyn Fn() -> bool,
) -> Result<(Vec<u8>, usize), FormatError> {
    let mut stop = Stop { ask: stop, next: 0 };
    let mut bits = Bits {
        data,
        pos: 0,
        buffer: 0,
        count: 0,
    };
    let mut out = Vec::with_capacity(limit.min(1 << 24));
    loop {
        stop.now(out.len())?;
        let last = bits.take(1)? == 1;
        match bits.take(2)? {
            0 => copy_stored(&mut bits, &mut out, limit)?,
            1 => {
                let (lit, dist) = fixed_tables()?;
                inflate_block(&mut bits, &mut out, limit, (&lit, &dist), &mut stop)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut bits)?;
                inflate_block(&mut bits, &mut out, limit, (&lit, &dist), &mut stop)?;
            }
            _ => return Err(CORRUPT),
        }
        if last {
            // The bits left over in the last byte used are dropped; whole bytes the reader
            // fetched ahead were not used.
            let unused = (bits.count / 8) as usize;
            return Ok((out, bits.pos.saturating_sub(unused)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_memory_stops_at_the_limit() {
        let raw = vec![7u8; (1 << 24) + 100];
        let packed = crate::deflate::deflate(&raw);
        let out = inflate(&packed, raw.len()).expect("valid");
        assert_eq!(out, raw);
        assert_eq!(out.capacity(), raw.len());
    }

    /// Output a little past a power of two grows to the limit in one step from the last doubling,
    /// not by the few bytes past it, which would copy the whole output once more.
    #[test]
    fn output_just_past_a_doubling_grows_to_the_limit_at_once() {
        let limit = (2 << 20) + 8;
        let mut out = vec![0u8; 1 << 20];
        make_room(&mut out, limit, 1);
        assert_eq!(out.capacity(), limit);
        let mut far = vec![0u8; 1 << 20];
        make_room(&mut far, 8 << 20, 1);
        assert_eq!(far.capacity(), 2 << 20);
        let mut small = Vec::new();
        make_room(&mut small, 100, 3);
        assert!(small.capacity() >= 3);
    }

    /// The stop is asked once a mebibyte of output, and a true answer ends the stream there.
    #[test]
    fn the_stop_is_asked_as_the_output_grows_and_ends_it() {
        use std::cell::Cell;
        let raw = vec![3u8; 8 * STOP_EVERY_BYTES + 5];
        let packed = crate::deflate::deflate(&raw);
        let asked = Cell::new(0usize);
        let count = || {
            asked.set(asked.get() + 1);
            false
        };
        let (out, _) = inflate_until(&packed, raw.len(), &count).expect("valid");
        assert_eq!(out, raw);
        assert!(asked.get() >= 8, "asked {} times", asked.get());
        let made = Cell::new(0usize);
        let third = || {
            made.set(made.get() + 1);
            made.get() > 3
        };
        assert!(matches!(
            inflate_until(&packed, raw.len(), &third),
            Err(FormatError::Stopped)
        ));
        let stored = [1, 3, 0, 0xfc, 0xff, b'a', b'b', b'c'];
        assert!(matches!(
            inflate_until(&stored, 10, &|| true),
            Err(FormatError::Stopped)
        ));
    }

    #[test]
    fn reads_a_stored_block() {
        let data = [1, 3, 0, 0xfc, 0xff, b'a', b'b', b'c'];
        assert_eq!(inflate(&data, 10).expect("valid"), b"abc");
    }

    /// Produced by zlib at level 9. The block type bits confirm dynamic Huffman codes.
    const ZLIB_DYNAMIC: [u8; 180] = [
        0xd5, 0x8c, 0xf5, 0x5e, 0xc2, 0x40, 0x1c, 0xc0, 0x5f, 0xe5, 0x87, 0xa4, 0x38, 0x29, 0x5,
        0x5, 0x83, 0xc6, 0xa2, 0x54, 0xca, 0x84, 0xc5, 0x6d, 0xbb, 0xd5, 0xad, 0x6e, 0x65, 0x3c,
        0xbb, 0xf7, 0xe1, 0x2d, 0xfc, 0xfb, 0x1b, 0xbe, 0x8a, 0xc0, 0xa1, 0x58, 0xd4, 0x41, 0x70,
        0x49, 0x68, 0x81, 0x4c, 0x22, 0xd0, 0xa8, 0x69, 0x7b, 0x40, 0x2, 0xe4, 0x82, 0xcf, 0xb0,
        0xc1, 0x27, 0x31, 0x48, 0x44, 0xe1, 0xc0, 0xe6, 0x99, 0x67, 0xc6, 0x20, 0x30, 0x29, 0xc4,
        0xbe, 0xa, 0x32, 0xe, 0x10, 0x43, 0x9, 0xb2, 0xc0, 0xc0, 0xe, 0x25, 0x2e, 0x6b, 0x15, 0xaf,
        0xb4, 0xcf, 0xfe, 0xcb, 0x15, 0x52, 0x7, 0xe9, 0x4c, 0x36, 0x97, 0x2f, 0x1c, 0x16, 0x8f,
        0xb8, 0xe3, 0x52, 0xb9, 0x52, 0xad, 0x9d, 0x9c, 0xd6, 0x1b, 0x67, 0xe7, 0xcd, 0xd6, 0xc5,
        0xe5, 0xd5, 0x75, 0xbb, 0xd3, 0xed, 0xf5, 0x7, 0xc3, 0xd1, 0xcd, 0xed, 0xdd, 0xfd, 0xc3,
        0x78, 0x32, 0x9d, 0xcd, 0x1f, 0x9f, 0x9e, 0x17, 0xcb, 0xd5, 0x7a, 0xf3, 0xf2, 0xfa, 0xf6,
        0xfe, 0xf1, 0xb9, 0xdd, 0xf1, 0x82, 0x28, 0x21, 0x59, 0x51, 0xb1, 0xa6, 0x1b, 0xa6, 0x45,
        0x6c, 0xc7, 0xf5, 0x7c, 0x1a, 0x84, 0x51, 0x9c, 0x7c, 0x7d, 0xff, 0xfc, 0xfe, 0x1,
    ];

    fn zlib_source() -> Vec<u8> {
        let sentence = b"the quick brown fox jumps over the lazy dog, pack my box with five dozen liquor jugs. ";
        let mut data = sentence.repeat(3);
        data.extend(32u8..127);
        data
    }

    #[test]
    fn reads_a_dynamic_block_from_zlib() {
        assert_eq!((ZLIB_DYNAMIC[0] >> 1) & 3, 2);
        assert_eq!(inflate(&ZLIB_DYNAMIC, 1000).expect("valid"), zlib_source());
    }

    #[test]
    fn output_limit_stops_decompression_bombs() {
        assert!(inflate(&ZLIB_DYNAMIC, 5).is_err());
    }

    #[test]
    fn says_how_many_bytes_the_stream_took() {
        let mut data = ZLIB_DYNAMIC.to_vec();
        data.extend_from_slice(&[9, 9, 9, 9]);
        let (out, used) = inflate_counting(&data, 1000).expect("valid");
        assert_eq!(out, zlib_source());
        assert_eq!(used, ZLIB_DYNAMIC.len());
        let stored = [1, 3, 0, 0xfc, 0xff, b'a', b'b', b'c', 7, 7];
        assert_eq!(inflate_counting(&stored, 10).expect("valid").1, 8);
    }

    #[test]
    fn truncated_input_is_an_error() {
        assert!(inflate(&[0xcb, 0x48], 100).is_err());
        assert!(inflate(&[], 100).is_err());
    }

    #[test]
    fn distance_before_start_is_an_error() {
        // Fixed block whose first symbol is a match with nothing to copy from.
        assert!(inflate(&[0x03, 0x02], 100).is_err());
    }
}
