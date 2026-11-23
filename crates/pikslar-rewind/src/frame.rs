//! The head and the framing of records in a log: see "Record format" in `docs/rewind.md`.
//! Every read is bounded and checked before anything is allocated for it.

use crate::hash::check64;
use crate::RewindError;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub const HEAD_LEN: usize = 96;
const HEAD_MAGIC: &[u8; 8] = b"PKRWHEAD";
const LOG_MAGIC: &[u8; 8] = b"PKRWLOG1";
const RECORD_MAGIC: &[u8; 4] = b"PRWR";
const VERSION: u8 = 1;
/// Where the first record of a log starts.
pub const LOG_START: u64 = 16;
pub const HEADER: u64 = 32;
pub const TRAILER: u64 = 16;
/// The largest record read: a project's whole budget.
pub const MOST_RECORD: u64 = 256 << 20;
/// The most a payload may inflate to: what a save writes at most, the budget a project is cut
/// to ([`crate::Budgets`] `keep`), as `plan` never encodes more than that.
pub const MOST_DECODED: u32 = 192 << 20;
/// The most a deflated payload may grow by. The writer stores a payload that would pass it as
/// it is, so only a crafted record asks for more.
pub const MOST_RATIO: u64 = 256;
pub const DEFLATED: u16 = 1;
/// The highest position a head may name: far past any real history, far below overflow.
pub const MOST_POSITION: u64 = 1 << 40;

const DAMAGED_HEAD: RewindError = RewindError::Damaged("the Rewind head is damaged");
const DAMAGED_RECORD: RewindError = RewindError::Damaged("a stored step is damaged");

/// The commit point of one project's history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Head {
    pub id: u128,
    pub gen: u64,
    /// The oldest state kept.
    pub base: u64,
    /// The state the saved file is in.
    pub tip: u64,
    /// Where the tip record ends in the log, or [`LOG_START`] when no record is kept.
    pub tip_end: u64,
    pub print: u128,
    /// When it was last written or attached, in Unix seconds.
    pub used: u64,
}

impl Head {
    pub fn encode(&self) -> [u8; HEAD_LEN] {
        let mut out = [0u8; HEAD_LEN];
        out[..8].copy_from_slice(HEAD_MAGIC);
        out[8..12].copy_from_slice(&u32::from(VERSION).to_le_bytes());
        out[16..32].copy_from_slice(&self.id.to_le_bytes());
        let words = [self.gen, self.base, self.tip, self.tip_end];
        for (n, w) in words.iter().enumerate() {
            out[32 + n * 8..40 + n * 8].copy_from_slice(&w.to_le_bytes());
        }
        out[64..80].copy_from_slice(&self.print.to_le_bytes());
        out[80..88].copy_from_slice(&self.used.to_le_bytes());
        let check = check64(&out[..88]);
        out[88..].copy_from_slice(&check.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Head, RewindError> {
        if bytes.len() != HEAD_LEN || &bytes[..8] != HEAD_MAGIC {
            return Err(DAMAGED_HEAD);
        }
        if u64_at(bytes, 88) != check64(&bytes[..88]) || u32_at(bytes, 8) != u32::from(VERSION) {
            return Err(DAMAGED_HEAD);
        }
        let head = Head {
            id: u128_at(bytes, 16),
            gen: u64_at(bytes, 32),
            base: u64_at(bytes, 40),
            tip: u64_at(bytes, 48),
            tip_end: u64_at(bytes, 56),
            print: u128_at(bytes, 64),
            used: u64_at(bytes, 80),
        };
        let sound = head.base <= head.tip
            && head.tip <= MOST_POSITION
            && head.tip_end >= LOG_START
            && (head.tip == head.base) == (head.tip_end == LOG_START);
        sound.then_some(head).ok_or(DAMAGED_HEAD)
    }

    /// The same chain, whenever it was last used.
    pub fn same_chain(&self, other: &Head) -> bool {
        Head { used: 0, ..*self } == Head { used: 0, ..*other }
    }
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    let mut w = [0u8; 4];
    w.copy_from_slice(&b[at..at + 4]);
    u32::from_le_bytes(w)
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(w)
}

fn u128_at(b: &[u8], at: usize) -> u128 {
    let mut w = [0u8; 16];
    w.copy_from_slice(&b[at..at + 16]);
    u128::from_le_bytes(w)
}

/// The first bytes of a log.
pub fn log_start(gen: u64) -> [u8; LOG_START as usize] {
    let mut out = [0u8; LOG_START as usize];
    out[..8].copy_from_slice(LOG_MAGIC);
    out[8..].copy_from_slice(&gen.to_le_bytes());
    out
}

/// What a record's header says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordHead {
    pub kind: u8,
    pub flags: u16,
    pub pos: u64,
    pub prev_end: u64,
    pub decoded: u32,
}

/// One record, framed, to be written at `start`.
pub fn encode_record(start: u64, head: RecordHead, stored: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(stored.len() + (HEADER + TRAILER) as usize);
    out.extend_from_slice(RECORD_MAGIC);
    out.push(VERSION);
    out.push(head.kind);
    out.extend_from_slice(&head.flags.to_le_bytes());
    out.extend_from_slice(&head.pos.to_le_bytes());
    out.extend_from_slice(&head.prev_end.to_le_bytes());
    out.extend_from_slice(&(stored.len() as u32).to_le_bytes());
    out.extend_from_slice(&head.decoded.to_le_bytes());
    out.extend_from_slice(stored);
    let check = check64(&out);
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&check.to_le_bytes());
    out
}

/// A record read back: its header, its stored payload and where it starts.
pub struct Found {
    pub head: RecordHead,
    pub stored: Vec<u8>,
    pub start: u64,
}

/// Reads the record that ends at `end` in a log `len` bytes long, which must be the step at
/// `pos`. Refused, reading nothing more, unless every length agrees and the check holds.
pub fn read_record(file: &mut File, len: u64, end: u64, pos: u64) -> Result<Found, RewindError> {
    if end > len || end < LOG_START + HEADER + TRAILER {
        return Err(DAMAGED_RECORD);
    }
    let mut trailer = [0u8; TRAILER as usize];
    read_at(file, end - TRAILER, &mut trailer)?;
    let start = u64_at(&trailer, 0);
    let size = end.checked_sub(start).ok_or(DAMAGED_RECORD)?;
    if start < LOG_START || !(HEADER + TRAILER..=MOST_RECORD).contains(&size) {
        return Err(DAMAGED_RECORD);
    }
    let mut body = Vec::new();
    let body_len = (size - TRAILER) as usize;
    body.try_reserve_exact(body_len)
        .map_err(|_| DAMAGED_RECORD)?;
    body.resize(body_len, 0);
    read_at(file, start, &mut body)?;
    if check64(&body) != u64_at(&trailer, 8) {
        return Err(DAMAGED_RECORD);
    }
    let head = parse_header(&body, start, pos)?;
    body.drain(..HEADER as usize);
    Ok(Found {
        head,
        stored: body,
        start,
    })
}

fn parse_header(body: &[u8], start: u64, pos: u64) -> Result<RecordHead, RewindError> {
    let stored = u32_at(body, 24) as u64;
    let head = RecordHead {
        kind: body[5],
        flags: u16::from_le_bytes([body[6], body[7]]),
        pos: u64_at(body, 8),
        prev_end: u64_at(body, 16),
        decoded: u32_at(body, 28),
    };
    let framed = &body[..4] == RECORD_MAGIC && body[4] == VERSION;
    let fits = stored + HEADER == body.len() as u64 && head.decoded <= MOST_DECODED;
    let linked = head.pos == pos && head.prev_end <= start && head.prev_end >= LOG_START;
    match framed && fits && linked {
        true => Ok(head),
        false => Err(DAMAGED_RECORD),
    }
}

fn read_at(file: &mut File, at: u64, into: &mut [u8]) -> Result<(), RewindError> {
    file.seek(SeekFrom::Start(at))?;
    file.read_exact(into)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head() -> Head {
        Head {
            id: 7,
            gen: 2,
            base: 3,
            tip: 9,
            tip_end: 400,
            print: 11,
            used: 12,
        }
    }

    #[test]
    fn a_head_reads_back_and_any_change_to_it_is_found() {
        let bytes = head().encode();
        assert_eq!(Head::decode(&bytes).expect("reads"), head());
        for at in 0..HEAD_LEN {
            let mut flipped = bytes;
            flipped[at] ^= 0x10;
            assert!(Head::decode(&flipped).is_err(), "byte {at}");
        }
        assert!(Head::decode(&bytes[..95]).is_err());
        let backwards = Head { tip: 1, ..head() };
        assert!(Head::decode(&backwards.encode()).is_err(), "tip below base");
        let empty = Head {
            tip: 3,
            tip_end: LOG_START,
            ..head()
        };
        assert_eq!(Head::decode(&empty.encode()).expect("reads"), empty);
    }
}
