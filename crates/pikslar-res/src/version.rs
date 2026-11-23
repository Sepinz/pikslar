//! Builds a `VERSIONINFO` block: the fixed part, an English (US) Unicode string table and the
//! translation list, each node 32-bit aligned as the format requires.

use crate::bytes::{pad, put16, put32};

/// What goes into the block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionInfo {
    /// Four numbers for both the file and the product version.
    pub numbers: [u16; 4],
    /// Marks the build as a pre-release (`VS_FF_PRERELEASE`).
    pub prerelease: bool,
    /// String table entries, written in this order.
    pub strings: Vec<(String, String)>,
}

const SIGNATURE: u32 = 0xFEEF_04BD;
const STRUC_VERSION: u32 = 0x0001_0000;
const FLAGS_MASK: u32 = 0x3F;
const FF_PRERELEASE: u32 = 0x2;
const OS_NT_WINDOWS32: u32 = 0x0004_0004;
const FT_APP: u32 = 1;
/// English (US), Unicode: the string table's name and the translation pair.
const TABLE: &str = "040904B0";
const LANG: u16 = 0x0409;
const CODEPAGE_UNICODE: u16 = 0x04B0;

/// Reads `major.minor.patch[-pre][+build]` into four numbers (the fourth is 0) and whether it
/// is a pre-release.
pub fn parse(version: &str) -> Result<([u16; 4], bool), String> {
    let core = version.split('+').next().unwrap_or_default();
    let (numbers, pre) = match core.split_once('-') {
        Some((n, p)) if !p.is_empty() => (n, true),
        Some(_) => return Err(format!("bad version {version:?}")),
        None => (core, false),
    };
    let parts: Vec<&str> = numbers.split('.').collect();
    if parts.len() != 3 {
        return Err(format!("version {version:?} needs major.minor.patch"));
    }
    let mut out = [0; 4];
    for (slot, part) in out.iter_mut().zip(parts) {
        let ok = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
        *slot = part
            .parse()
            .ok()
            .filter(|_| ok)
            .ok_or_else(|| format!("bad version {version:?}"))?;
    }
    Ok((out, pre))
}

/// The block's bytes.
pub fn block(info: &VersionInfo) -> Vec<u8> {
    let strings: Vec<Vec<u8>> = info
        .strings
        .iter()
        .map(|(k, v)| node(k, Value::Text(v), &[]))
        .collect();
    let table = node(TABLE, Value::None, &strings);
    let string_info = node("StringFileInfo", Value::None, &[table]);
    let mut pair = Vec::new();
    put16(&mut pair, LANG);
    put16(&mut pair, CODEPAGE_UNICODE);
    let translation = node("Translation", Value::Binary(&pair), &[]);
    let var_info = node("VarFileInfo", Value::None, &[translation]);
    node(
        "VS_VERSION_INFO",
        Value::Binary(&fixed(info)),
        &[string_info, var_info],
    )
}

/// `VS_FIXEDFILEINFO`: the same version for the file and the product, no date.
fn fixed(info: &VersionInfo) -> Vec<u8> {
    let [a, b, c, d] = info.numbers.map(u32::from);
    let (ms, ls) = (a << 16 | b, c << 16 | d);
    let flags = if info.prerelease { FF_PRERELEASE } else { 0 };
    let mut v = Vec::with_capacity(52);
    for x in [
        SIGNATURE,
        STRUC_VERSION,
        ms,
        ls,
        ms,
        ls,
        FLAGS_MASK,
        flags,
        OS_NT_WINDOWS32,
        FT_APP,
        0,
        0,
        0,
    ] {
        put32(&mut v, x);
    }
    v
}

enum Value<'a> {
    None,
    Text(&'a str),
    Binary(&'a [u8]),
}

/// One node: length, value length (in characters for text), type, key, value, children. The
/// length counts everything up to the end of the last child, padded to 32 bits.
fn node(key: &str, value: Value, children: &[Vec<u8>]) -> Vec<u8> {
    let mut v = vec![0, 0];
    let (value_len, text) = match &value {
        Value::None => (0, true),
        Value::Text(t) => (t.encode_utf16().count() + 1, true),
        Value::Binary(b) => (b.len(), false),
    };
    put16(&mut v, value_len as u16);
    put16(&mut v, u16::from(text));
    wide(&mut v, key);
    pad(&mut v, 4);
    match value {
        Value::None => {}
        Value::Text(t) => wide(&mut v, t),
        Value::Binary(b) => v.extend_from_slice(b),
    }
    for child in children {
        pad(&mut v, 4);
        v.extend_from_slice(child);
    }
    pad(&mut v, 4);
    let len = v.len() as u16;
    v[..2].copy_from_slice(&len.to_le_bytes());
    v
}

fn wide(v: &mut Vec<u8>, s: &str) {
    for unit in s.encode_utf16() {
        put16(v, unit);
    }
    put16(v, 0);
}

#[cfg(test)]
#[path = "version_tests.rs"]
mod tests;
