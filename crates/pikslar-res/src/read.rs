//! Small readers for checking what was written: a COFF object, a resource directory tree, a PE
//! file's resources and a `VERSIONINFO` block. Strict where it matters for the checks, and
//! never trusted with anything beyond test and build inputs.

use crate::bytes::{get16, get32, slice};

/// One relocation of a COFF section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reloc {
    pub offset: u32,
    pub symbol: u32,
    pub kind: u16,
}

/// One section of a COFF object.
#[derive(Clone, Debug)]
pub struct Section {
    pub name: [u8; 8],
    pub characteristics: u32,
    pub data: Vec<u8>,
    pub relocs: Vec<Reloc>,
}

/// One symbol table record (auxiliary records are counted, not decoded).
#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: [u8; 8],
    pub value: u32,
    pub section: i16,
    pub class: u8,
    pub aux: u8,
}

/// A parsed COFF object.
#[derive(Clone, Debug)]
pub struct Coff {
    pub machine: u16,
    pub timestamp: u32,
    pub sections: Vec<Section>,
    /// Symbols by table index; auxiliary slots hold `None`.
    pub symbols: Vec<Option<Symbol>>,
}

/// Reads a COFF object file.
pub fn coff(b: &[u8]) -> Result<Coff, String> {
    let count = get16(b, 2)? as usize;
    let sym_at = get32(b, 8)? as usize;
    let sym_count = get32(b, 12)? as usize;
    if get16(b, 16)? != 0 {
        return Err("an object file has no optional header".into());
    }
    let sections = (0..count)
        .map(|i| section(b, 20 + 40 * i))
        .collect::<Result<Vec<_>, _>>()?;
    let mut symbols = Vec::with_capacity(sym_count);
    while symbols.len() < sym_count {
        let at = sym_at + 18 * symbols.len();
        let s = slice(b, at, 18)?;
        let aux = s[17];
        symbols.push(Some(Symbol {
            name: name8(s),
            value: get32(s, 8)?,
            section: get16(s, 12)? as i16,
            class: s[16],
            aux,
        }));
        symbols.extend((0..aux).map(|_| None));
    }
    Ok(Coff {
        machine: get16(b, 0)?,
        timestamp: get32(b, 4)?,
        sections,
        symbols,
    })
}

fn name8(s: &[u8]) -> [u8; 8] {
    let mut n = [0; 8];
    n.copy_from_slice(&s[..8]);
    n
}

fn section(b: &[u8], at: usize) -> Result<Section, String> {
    let h = slice(b, at, 40)?;
    let data = slice(b, get32(h, 20)? as usize, get32(h, 16)? as usize)?.to_vec();
    let rel_at = get32(h, 24)? as usize;
    let relocs = (0..get16(h, 32)? as usize)
        .map(|i| {
            let r = slice(b, rel_at + 10 * i, 10)?;
            Ok(Reloc {
                offset: get32(r, 0)?,
                symbol: get32(r, 4)?,
                kind: get16(r, 8)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Section {
        name: name8(h),
        characteristics: get32(h, 36)?,
        data,
        relocs,
    })
}

/// One resource found in a directory tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Leaf {
    pub ty: u16,
    pub id: u16,
    pub lang: u16,
    /// Where the leaf's data entry sits, from the start of the tree.
    pub entry: u32,
    /// The entry's data address field and size, as stored.
    pub address: u32,
    pub size: u32,
}

/// Walks a resource directory tree (type, id, language), numeric ids only, in stored order.
pub fn leaves(tree: &[u8]) -> Result<Vec<Leaf>, String> {
    let mut out = Vec::new();
    for (ty, types) in directory(tree, 0)? {
        for (id, ids) in directory(tree, sub(ty, types)?)? {
            for (lang, entry) in directory(tree, sub(id, ids)?)? {
                if entry & 0x8000_0000 != 0 {
                    return Err("a language entry points to a directory".into());
                }
                let e = entry as usize;
                let (address, size) = (get32(tree, e)?, get32(tree, e + 4)?);
                out.push(Leaf {
                    ty: ty as u16,
                    id: id as u16,
                    lang: lang as u16,
                    entry,
                    address,
                    size,
                });
            }
        }
    }
    Ok(out)
}

fn sub(name: u32, to: u32) -> Result<usize, String> {
    if to & 0x8000_0000 == 0 {
        return Err(format!("entry {name} should point to a subdirectory"));
    }
    Ok((to & 0x7FFF_FFFF) as usize)
}

/// The (name, target) pairs of one directory table. Named entries are refused.
fn directory(tree: &[u8], at: usize) -> Result<Vec<(u32, u32)>, String> {
    if get16(tree, at + 12)? != 0 {
        return Err("named resource entries are not used by Pikslar".into());
    }
    let ids = get16(tree, at + 14)? as usize;
    let mut out = Vec::with_capacity(ids);
    for i in 0..ids {
        let e = at + 16 + 8 * i;
        let name = get32(tree, e)?;
        if name & 0x8000_0000 != 0 || name > 0xFFFF {
            return Err(format!("bad entry id {name:#x}"));
        }
        out.push((name, get32(tree, e + 4)?));
    }
    Ok(out)
}

/// Every resource of a PE file, with its bytes.
pub fn pe_resources(b: &[u8]) -> Result<Vec<(Leaf, Vec<u8>)>, String> {
    let pe = get32(b, 0x3C)? as usize;
    if slice(b, pe, 4)? != b"PE\0\0" {
        return Err("not a PE file".into());
    }
    let count = get16(b, pe + 6)? as usize;
    let opt = pe + 24;
    let table = opt + get16(b, pe + 20)? as usize;
    if get16(b, opt)? != 0x20B {
        return Err("not a PE32+ file".into());
    }
    let rva = get32(b, opt + 112 + 2 * 8)?;
    let map = |r: u32| -> Result<usize, String> {
        for i in 0..count {
            let s = table + 40 * i;
            let (size, va, raw) = (get32(b, s + 8)?, get32(b, s + 12)?, get32(b, s + 20)?);
            if r >= va && r < va + size {
                return Ok((raw + r - va) as usize);
            }
        }
        Err(format!("address {r:#x} is in no section"))
    };
    if rva == 0 {
        return Ok(Vec::new());
    }
    let tree = slice(b, map(rva)?, b.len().saturating_sub(map(rva)?))?;
    leaves(tree)?
        .into_iter()
        .map(|l| Ok((l, slice(b, map(l.address)?, l.size as usize)?.to_vec())))
        .collect()
}

/// The fixed part (13 words) and the string pairs of a `VERSIONINFO` block.
pub type VersionStrings = (Vec<u32>, Vec<(String, String)>);

/// The string pairs of a `VERSIONINFO` block's first string table, and its fixed part.
pub fn version_strings(b: &[u8]) -> Result<VersionStrings, String> {
    let (key, value, children) = node(b, 0)?;
    if key != "VS_VERSION_INFO" {
        return Err(format!("unexpected root key {key}"));
    }
    let fixed = (0..13)
        .map(|i| get32(value, 4 * i))
        .collect::<Result<Vec<_>, _>>()?;
    let mut strings = Vec::new();
    for (k, _, kids) in children
        .iter()
        .map(|&at| node(b, at))
        .collect::<Result<Vec<_>, _>>()?
    {
        if k != "StringFileInfo" {
            continue;
        }
        let (_, _, entries) = node(b, kids[0])?;
        for at in entries {
            let (name, text, _) = node(b, at)?;
            strings.push((name, utf16(text)?.trim_end_matches('\0').to_string()));
        }
    }
    Ok((fixed, strings))
}

type Node<'a> = (String, &'a [u8], Vec<usize>);

/// One node: (key, value bytes, child offsets).
fn node(b: &[u8], at: usize) -> Result<Node<'_>, String> {
    let len = get16(b, at)? as usize;
    let vlen = get16(b, at + 2)? as usize;
    let text = get16(b, at + 4)? == 1;
    let mut p = at + 6;
    let mut key = Vec::new();
    loop {
        let c = get16(b, p)?;
        p += 2;
        if c == 0 {
            break;
        }
        key.push(c);
    }
    let align = |x: usize| (x + 3) & !3;
    p = align(p);
    let value = slice(b, p, if text { vlen * 2 } else { vlen })?;
    let mut child = align(p + value.len());
    let mut kids = Vec::new();
    while child + 6 <= at + len {
        kids.push(child);
        child = align(child + get16(b, child)?.max(1) as usize);
    }
    Ok((
        String::from_utf16(&key).map_err(|e| e.to_string())?,
        value,
        kids,
    ))
}

fn utf16(b: &[u8]) -> Result<String, String> {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&units).map_err(|e| e.to_string())
}
