//! Little-endian writing and reading helpers.

pub(crate) fn put16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

pub(crate) fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// Appends zero bytes until the length is a multiple of `to`.
pub(crate) fn pad(v: &mut Vec<u8>, to: usize) {
    while v.len() % to != 0 {
        v.push(0);
    }
}

/// Converts a length or offset to the 32-bit field the formats use.
pub(crate) fn u32_of(n: usize, what: &str) -> Result<u32, String> {
    u32::try_from(n).map_err(|_| format!("{what} is too large ({n} bytes)"))
}

pub(crate) fn get16(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| format!("truncated: no 16-bit value at {at}"))
}

pub(crate) fn get32(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| format!("truncated: no 32-bit value at {at}"))
}

pub(crate) fn slice(b: &[u8], at: usize, len: usize) -> Result<&[u8], String> {
    b.get(at..at.saturating_add(len))
        .ok_or_else(|| format!("truncated: {len} bytes at {at}"))
}
