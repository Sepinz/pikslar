//! DEFLATE compression using LZ77 and the fixed Huffman code.

const WINDOW: usize = 32_768;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const HASH_BITS: usize = 15;
const MAX_CHAIN: usize = 32;

struct Writer {
    out: Vec<u8>,
    buffer: u64,
    count: u32,
}

impl Writer {
    #[inline]
    fn bits(&mut self, value: u32, n: u32) {
        self.buffer |= (value as u64) << self.count;
        self.count += n;
        while self.count >= 8 {
            self.out.push(self.buffer as u8);
            self.buffer >>= 8;
            self.count -= 8;
        }
    }

    /// Huffman codes are packed most significant bit first.
    #[inline]
    fn code(&mut self, code: u32, n: u32) {
        self.bits(code.reverse_bits() >> (32 - n), n);
    }

    fn literal_or_length(&mut self, symbol: u32) {
        match symbol {
            0..=143 => self.code(0x30 + symbol, 8),
            144..=255 => self.code(0x190 + symbol - 144, 9),
            256..=279 => self.code(symbol - 256, 7),
            _ => self.code(0xc0 + symbol - 280, 8),
        }
    }

    /// A fixed-Huffman block, the only and last one.
    fn start(capacity: usize) -> Self {
        let mut w = Writer {
            out: Vec::with_capacity(capacity),
            buffer: 0,
            count: 0,
        };
        w.bits(1, 1);
        w.bits(1, 2);
        w
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push(self.buffer as u8);
        }
        self.out
    }
}

/// Finds the (base symbol offset, extra bits, base value) bucket for a value.
#[inline]
fn bucket(value: usize, table: &[(u16, u8)]) -> usize {
    table.partition_point(|&(base, _)| base as usize <= value) - 1
}

const LENGTHS: [(u16, u8); 29] = [
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 1),
    (13, 1),
    (15, 1),
    (17, 1),
    (19, 2),
    (23, 2),
    (27, 2),
    (31, 2),
    (35, 3),
    (43, 3),
    (51, 3),
    (59, 3),
    (67, 4),
    (83, 4),
    (99, 4),
    (115, 4),
    (131, 5),
    (163, 5),
    (195, 5),
    (227, 5),
    (258, 0),
];
const DISTANCES: [(u16, u8); 30] = [
    (1, 0),
    (2, 0),
    (3, 0),
    (4, 0),
    (5, 1),
    (7, 1),
    (9, 2),
    (13, 2),
    (17, 3),
    (25, 3),
    (33, 4),
    (49, 4),
    (65, 5),
    (97, 5),
    (129, 6),
    (193, 6),
    (257, 7),
    (385, 7),
    (513, 8),
    (769, 8),
    (1025, 9),
    (1537, 9),
    (2049, 10),
    (3073, 10),
    (4097, 11),
    (6145, 11),
    (8193, 12),
    (12289, 12),
    (16385, 13),
    (24577, 13),
];

fn write_match(w: &mut Writer, len: usize, dist: usize) {
    let i = bucket(len, &LENGTHS);
    w.literal_or_length(257 + i as u32);
    w.bits((len - LENGTHS[i].0 as usize) as u32, LENGTHS[i].1 as u32);
    let d = bucket(dist, &DISTANCES);
    w.code(d as u32, 5);
    w.bits(
        (dist - DISTANCES[d].0 as usize) as u32,
        DISTANCES[d].1 as u32,
    );
}

#[inline]
fn hash(d: &[u8]) -> usize {
    let v = d[0] as u32 | (d[1] as u32) << 8 | (d[2] as u32) << 16;
    (v.wrapping_mul(0x9e37_79b1) >> (32 - HASH_BITS)) as usize
}

/// Bytes a position needs after it before it is coded exactly as it would be with the whole
/// input in hand: the longest match, and the last position inside it to be indexed.
const LOOKAHEAD: usize = MAX_MATCH + MIN_MATCH;

/// How much coded input a [`Deflater`] lets build up before dropping it. A multiple of the
/// window, so positions keep their place in `prev`.
const REBASE: usize = 32 * WINDOW;

/// Positions index the input held, as `i32`. Input past 2 GiB in one piece gets no matches
/// for those positions, so the stream stays valid and only compresses less.
struct Matcher {
    head: Vec<i32>,
    prev: Vec<i32>,
}

impl Matcher {
    fn new() -> Self {
        Matcher {
            head: vec![-1; 1 << HASH_BITS],
            prev: vec![-1; WINDOW],
        }
    }

    fn insert(&mut self, data: &[u8], pos: usize) {
        if pos + MIN_MATCH <= data.len() {
            let h = hash(&data[pos..]);
            self.prev[pos & (WINDOW - 1)] = self.head[h];
            self.head[h] = pos as i32;
        }
    }

    fn longest(&self, data: &[u8], pos: usize) -> (usize, usize) {
        if pos + MIN_MATCH > data.len() {
            return (0, 0);
        }
        let limit = (data.len() - pos).min(MAX_MATCH);
        let (mut best_len, mut best_dist) = (0, 0);
        let mut candidate = self.head[hash(&data[pos..])];
        for _ in 0..MAX_CHAIN {
            if candidate < 0 || pos - candidate as usize > WINDOW - 1 {
                break;
            }
            let from = candidate as usize;
            let len = data[from..]
                .iter()
                .zip(&data[pos..pos + limit])
                .take_while(|(a, b)| a == b)
                .count();
            if len > best_len {
                (best_len, best_dist) = (len, pos - from);
                if len == limit {
                    break;
                }
            }
            candidate = self.prev[from & (WINDOW - 1)];
        }
        (best_len, best_dist)
    }

    /// The first `shift` bytes are gone. Anything that pointed there was already out of reach.
    fn rebase(&mut self, shift: usize) {
        let shift = shift as i32;
        for p in self.head.iter_mut().chain(self.prev.iter_mut()) {
            *p = if *p >= shift { *p - shift } else { -1 };
        }
    }
}

/// Codes from `pos` while each position has [`LOOKAHEAD`] bytes after it, or to the end when
/// `LAST`. Returns where it stopped.
#[inline(always)]
fn code<const LAST: bool>(w: &mut Writer, m: &mut Matcher, data: &[u8], mut pos: usize) -> usize {
    while pos < data.len() && (LAST || pos + LOOKAHEAD <= data.len()) {
        let (len, dist) = m.longest(data, pos);
        if len >= MIN_MATCH {
            write_match(w, len, dist);
            for p in pos..pos + len {
                m.insert(data, p);
            }
            pos += len;
        } else {
            w.literal_or_length(data[pos] as u32);
            m.insert(data, pos);
            pos += 1;
        }
    }
    pos
}

/// Compresses bytes into a raw DEFLATE stream.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut w = Writer::start(data.len() / 4 + 16);
    let mut m = Matcher::new();
    code::<true>(&mut w, &mut m, data, 0);
    w.literal_or_length(256);
    w.finish()
}

/// Compresses input that arrives in pieces, holding the 32 KiB window, at most [`REBASE`] of
/// coded input and what is not yet coded. The stream is the same, byte for byte, as
/// [`deflate`] of all the pieces joined.
pub struct Deflater {
    w: Writer,
    m: Matcher,
    held: Vec<u8>,
    pos: usize,
}

impl Default for Deflater {
    fn default() -> Self {
        Deflater::expecting(0)
    }
}

impl Deflater {
    /// A deflater for about `total` bytes of input, with its buffers sized once: at most what
    /// it can hold, however large the input.
    pub fn expecting(total: usize) -> Self {
        let held = total.min(REBASE + 2 * WINDOW);
        Deflater {
            w: Writer::start(held / 4 + 16),
            m: Matcher::new(),
            held: Vec::with_capacity(held),
            pos: 0,
        }
    }

    /// Takes the next piece of input. Output becomes available as it is coded.
    pub fn write(&mut self, input: &[u8]) {
        self.write_with(|held| held.extend_from_slice(input), |_| {});
    }

    /// Takes the next piece of input, which `fill` appends to the deflater's own buffer, so a
    /// caller that builds its input saves a copy. `seen` is shown the bytes added, before they
    /// are coded.
    pub fn write_with(&mut self, fill: impl FnOnce(&mut Vec<u8>), seen: impl FnOnce(&[u8])) {
        let start = self.held.len();
        fill(&mut self.held);
        seen(&self.held[start..]);
        self.pos = code::<false>(&mut self.w, &mut self.m, &self.held, self.pos);
        if self.pos >= REBASE + WINDOW {
            let shift = (self.pos - WINDOW) & !(WINDOW - 1);
            self.held.drain(..shift);
            self.m.rebase(shift);
            self.pos -= shift;
        }
    }

    /// Moves the compressed bytes ready so far onto the end of `out`.
    pub fn take(&mut self, out: &mut Vec<u8>) {
        out.append(&mut self.w.out);
    }

    /// Codes what is left and returns the rest of the stream.
    pub fn finish(mut self) -> Vec<u8> {
        code::<true>(&mut self.w, &mut self.m, &self.held, self.pos);
        self.w.literal_or_length(256);
        self.w.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inflate::inflate;

    fn round_trip(data: &[u8]) -> usize {
        let packed = deflate(data);
        assert_eq!(inflate(&packed, data.len()).expect("valid stream"), data);
        packed.len()
    }

    #[test]
    fn empty_input() {
        round_trip(b"");
    }

    fn streamed(data: &[u8], piece: usize) -> Vec<u8> {
        let mut d = Deflater::default();
        let mut out = Vec::new();
        for part in data.chunks(piece.max(1)) {
            d.write(part);
            d.take(&mut out);
        }
        out.extend(d.finish());
        out
    }

    /// Mixed input: noise, long runs, and repeats near and far, the way image rows look.
    fn mixed(len: usize, seed: u32) -> Vec<u8> {
        let mut x = seed;
        let mut data = Vec::with_capacity(len);
        while data.len() < len {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            match x >> 30 {
                0 => data.extend((0..(x >> 20 & 63)).map(|i| (x >> (i % 24)) as u8)),
                1 => data.extend(std::iter::repeat_n(
                    (x >> 8) as u8,
                    (x >> 16 & 511) as usize,
                )),
                _ if data.len() > 300 => {
                    let back = 1 + (x >> 4) as usize % data.len().min(40_000);
                    let from = data.len() - back;
                    data.extend_from_within(
                        from..(from + (x >> 22 & 300) as usize).min(data.len()),
                    );
                }
                _ => data.push(x as u8),
            }
        }
        data.truncate(len);
        data
    }

    #[test]
    fn the_one_shot_encoder_writes_what_it_wrote_before() {
        for (len, seed) in [(0, 1), (1, 2), (300, 3), (70_000, 4), (300_000, 5)] {
            let data = mixed(len, seed);
            assert_eq!(
                deflate(&data),
                crate::deflate_reference::deflate(&data),
                "{len}"
            );
        }
    }

    #[test]
    fn streaming_in_any_pieces_writes_the_same_bytes() {
        // Long enough to be rebased several times, whatever the piece size.
        let len = 3 * REBASE + 12_345;
        let data = mixed(len, 9);
        let whole = crate::deflate_reference::deflate(&data);
        for piece in [
            1,
            2,
            3,
            257,
            258,
            259,
            261,
            262,
            4_096,
            32_767,
            32_768,
            65_537,
            REBASE - 1,
            REBASE + WINDOW + 1,
            len,
        ] {
            assert_eq!(streamed(&data, piece), whole, "pieces of {piece}");
        }
        let runs = vec![0u8; 100_000];
        assert_eq!(
            streamed(&runs, 1_000),
            crate::deflate_reference::deflate(&runs)
        );
        assert_eq!(streamed(b"", 10), crate::deflate_reference::deflate(b""));
    }

    #[test]
    fn streaming_holds_a_bounded_amount_of_input() {
        let data = mixed(4 * REBASE, 11);
        let mut d = Deflater::expecting(data.len());
        let mut out = Vec::new();
        for part in data.chunks(10_000) {
            d.write(part);
            d.take(&mut out);
            assert!(
                d.held.len() < REBASE + 2 * WINDOW + 10_000,
                "{}",
                d.held.len()
            );
        }
        out.extend(d.finish());
        assert_eq!(out, crate::deflate_reference::deflate(&data));
        assert_eq!(inflate(&out, data.len()).expect("valid"), data);
    }

    #[test]
    fn every_byte_value() {
        let data: Vec<u8> = (0..=255).collect();
        round_trip(&data);
    }

    #[test]
    fn repeats_compress_well() {
        assert!(round_trip(&[7u8; 100_000]) < 1_000);
    }

    #[test]
    fn long_distances_and_lengths() {
        let mut data = Vec::new();
        let mut x = 1u32;
        for _ in 0..50_000 {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            data.push((x >> 24) as u8);
        }
        let copy = data[..40_000].to_vec();
        data.extend_from_slice(&copy);
        round_trip(&data);
    }

    #[test]
    fn every_match_length_bucket() {
        for len in MIN_MATCH..=MAX_MATCH {
            let mut data = vec![b'x'; 300];
            data.extend((0..len).map(|i| (i % 251) as u8));
            data.push(255);
            data.extend((0..len).map(|i| (i % 251) as u8));
            round_trip(&data);
        }
    }
}
