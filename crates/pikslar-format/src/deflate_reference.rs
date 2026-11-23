//! The one-shot encoder as it was before streaming, kept only to prove the streaming one writes
//! the same bytes. Copied unchanged from commit cce03a5.
#![allow(dead_code)]

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

struct Matcher {
    head: Vec<i32>,
    prev: Vec<i32>,
}

impl Matcher {
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
}

/// Compresses bytes into a raw DEFLATE stream.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    let mut w = Writer {
        out: Vec::with_capacity(data.len() / 4 + 16),
        buffer: 0,
        count: 0,
    };
    w.bits(1, 1);
    w.bits(1, 2);
    let mut m = Matcher {
        head: vec![-1; 1 << HASH_BITS],
        prev: vec![-1; WINDOW],
    };
    let mut pos = 0;
    while pos < data.len() {
        let (len, dist) = m.longest(data, pos);
        if len >= MIN_MATCH {
            write_match(&mut w, len, dist);
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
    w.literal_or_length(256);
    w.finish()
}
