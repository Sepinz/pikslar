//! The COFF object writer.
//!
//! Layout of the one `.rsrc` section, as windres writes it:
//!
//! ```text
//! root directory (types) | one directory per type (ids) | one per id (languages)
//! | data entries (16 bytes each) | padding to 8 | data, each padded to 8
//! ```
//!
//! Directory offsets are relative to the start of the section, which the linker places at the
//! start of the image's resource section. Each data entry's first field must be an image-relative
//! address, so it holds the data's offset in this section and carries an ADDR32NB relocation
//! against the section's own symbol: the linker adds the section's address to it.

use crate::bytes::{pad, put16, put32, u32_of};
use crate::Resource;

/// `IMAGE_FILE_MACHINE_AMD64`.
pub const MACHINE_AMD64: u16 = 0x8664;
/// `IMAGE_REL_AMD64_ADDR32NB`: a 32-bit address relative to the image base.
pub const REL_ADDR32NB: u16 = 3;
/// The section name windres uses; the GNU linker merges every input `.rsrc` into one tree.
pub const SECTION: [u8; 8] = *b".rsrc\0\0\0";
/// Initialised data, readable and writable: what windres marks `.rsrc` with.
const SECTION_FLAGS: u32 = 0xC000_0040;
const SUBDIRECTORY: u32 = 0x8000_0000;
const DIR_HEADER: usize = 16;
const DIR_ENTRY: usize = 8;
const DATA_ENTRY: usize = 16;
const FILE_HEADER: usize = 20;
const SECTION_HEADER: usize = 40;

/// Resources grouped by type, then id; languages in order inside.
type Types<'a> = Vec<(u16, Vec<(u16, Vec<&'a Resource>)>)>;

/// The section's bytes and the offsets of the fields that need relocating.
struct Body {
    bytes: Vec<u8>,
    relocs: Vec<u32>,
}

/// Writes the resources as a COFF object with one `.rsrc` section.
pub fn object(resources: &[Resource]) -> Result<Vec<u8>, String> {
    let body = body(&group(resources)?)?;
    let raw_at = FILE_HEADER + SECTION_HEADER;
    let rel_at = raw_at + body.bytes.len();
    let sym_at = rel_at + 10 * body.relocs.len();
    let mut o = Vec::with_capacity(sym_at + 2 * 18 + 4);
    put16(&mut o, MACHINE_AMD64);
    put16(&mut o, 1); // sections
    put32(&mut o, 0); // no time stamp
    put32(&mut o, u32_of(sym_at, "the object")?);
    put32(&mut o, 2); // the section symbol and its auxiliary record
    put16(&mut o, 0); // no optional header
    put16(&mut o, 0); // characteristics
    section_header(&mut o, &body, raw_at, rel_at)?;
    o.extend_from_slice(&body.bytes);
    for &at in &body.relocs {
        put32(&mut o, at);
        put32(&mut o, 0); // symbol 0: the section symbol
        put16(&mut o, REL_ADDR32NB);
    }
    section_symbol(&mut o, &body)?;
    put32(&mut o, 4); // an empty string table holds only its own size
    Ok(o)
}

/// Sorts and groups the resources, refusing duplicates.
fn group(resources: &[Resource]) -> Result<Types<'_>, String> {
    let mut sorted: Vec<&Resource> = resources.iter().collect();
    sorted.sort_by_key(|r| (r.ty, r.id, r.lang));
    let mut types: Types = Vec::new();
    for r in sorted {
        if types.last().map(|t| t.0) != Some(r.ty) {
            types.push((r.ty, Vec::new()));
        }
        let ids = &mut types.last_mut().ok_or("unreachable: no type")?.1;
        if ids.last().map(|i| i.0) != Some(r.id) {
            ids.push((r.id, Vec::new()));
        }
        let langs = &mut ids.last_mut().ok_or("unreachable: no id")?.1;
        if langs.last().is_some_and(|l| l.lang == r.lang) {
            return Err(format!(
                "duplicate resource: type {} id {} language {:#06x}",
                r.ty, r.id, r.lang
            ));
        }
        langs.push(r);
    }
    Ok(types)
}

fn body(types: &Types) -> Result<Body, String> {
    let ids: Vec<&(u16, Vec<&Resource>)> = types.iter().flat_map(|t| &t.1).collect();
    let leaves: Vec<&Resource> = ids.iter().flat_map(|i| i.1.iter().copied()).collect();
    let mut at = DIR_HEADER + DIR_ENTRY * types.len();
    let type_dirs: Vec<usize> = types.iter().map(|t| bump(&mut at, t.1.len())).collect();
    let id_dirs: Vec<usize> = ids.iter().map(|i| bump(&mut at, i.1.len())).collect();
    let entries = at;
    let data_at = (entries + DATA_ENTRY * leaves.len() + 7) & !7;
    let mut b = Vec::new();
    directory(
        &mut b,
        types.iter().map(|t| u32::from(t.0)).zip(type_dirs),
        SUBDIRECTORY,
    )?;
    let mut next = 0;
    for t in types {
        let dirs = id_dirs[next..next + t.1.len()].iter().copied();
        directory(
            &mut b,
            t.1.iter().map(|i| u32::from(i.0)).zip(dirs),
            SUBDIRECTORY,
        )?;
        next += t.1.len();
    }
    let mut entry = entries;
    for i in &ids {
        let langs: Vec<(u32, usize)> =
            i.1.iter()
                .map(|r| (u32::from(r.lang), bump_entry(&mut entry)))
                .collect();
        directory(&mut b, langs.into_iter(), 0)?;
    }
    data_entries(b, &leaves, data_at)
}

/// Reserves one directory table with `n` entries at `*at`, returning where it starts.
fn bump(at: &mut usize, n: usize) -> usize {
    let start = *at;
    *at += DIR_HEADER + DIR_ENTRY * n;
    start
}

fn bump_entry(at: &mut usize) -> usize {
    let start = *at;
    *at += DATA_ENTRY;
    start
}

/// Writes a directory table of numeric entries; `flag` marks subdirectory targets.
fn directory(
    b: &mut Vec<u8>,
    entries: impl ExactSizeIterator<Item = (u32, usize)>,
    flag: u32,
) -> Result<(), String> {
    put32(b, 0); // characteristics
    put32(b, 0); // no time stamp
    put32(b, 0); // version 0.0
    put16(b, 0); // named entries
    put16(
        b,
        u16::try_from(entries.len()).map_err(|_| "too many resource entries")?,
    );
    for (name, to) in entries {
        put32(b, name);
        put32(b, flag | u32_of(to, "the resource tree")?);
    }
    Ok(())
}

/// Appends the data entries and the data, recording each entry's relocation.
fn data_entries(mut b: Vec<u8>, leaves: &[&Resource], data_at: usize) -> Result<Body, String> {
    let mut data = Vec::new();
    let mut relocs = Vec::with_capacity(leaves.len());
    for r in leaves {
        relocs.push(u32_of(b.len(), "the resource tree")?);
        put32(&mut b, u32_of(data_at + data.len(), "the resource data")?);
        put32(&mut b, u32_of(r.data.len(), "a resource")?);
        put32(&mut b, 0); // code page
        put32(&mut b, 0); // reserved
        data.extend_from_slice(&r.data);
        pad(&mut data, 8);
    }
    pad(&mut b, 8);
    debug_assert_eq!(b.len(), data_at);
    b.extend_from_slice(&data);
    Ok(Body { bytes: b, relocs })
}

fn section_header(
    o: &mut Vec<u8>,
    body: &Body,
    raw_at: usize,
    rel_at: usize,
) -> Result<(), String> {
    o.extend_from_slice(&SECTION);
    put32(o, 0); // virtual size: 0 in objects
    put32(o, 0); // virtual address
    put32(o, u32_of(body.bytes.len(), "the section")?);
    put32(o, u32_of(raw_at, "the object")?);
    put32(
        o,
        if body.relocs.is_empty() {
            0
        } else {
            u32_of(rel_at, "the object")?
        },
    );
    put32(o, 0); // line numbers
    put16(
        o,
        u16::try_from(body.relocs.len()).map_err(|_| "too many resources")?,
    );
    put16(o, 0);
    put32(o, SECTION_FLAGS);
    Ok(())
}

/// The static section symbol `.rsrc` and its auxiliary section-definition record.
fn section_symbol(o: &mut Vec<u8>, body: &Body) -> Result<(), String> {
    o.extend_from_slice(&SECTION);
    put32(o, 0); // value
    put16(o, 1); // section number
    put16(o, 0); // type
    o.push(3); // IMAGE_SYM_CLASS_STATIC
    o.push(1); // one auxiliary record
    put32(o, u32_of(body.bytes.len(), "the section")?);
    put16(
        o,
        u16::try_from(body.relocs.len()).map_err(|_| "too many resources")?,
    );
    put16(o, 0); // line numbers
    put32(o, 0); // checksum
    put16(o, 0); // associated section
    o.extend_from_slice(&[0; 4]); // selection and padding
    Ok(())
}

#[cfg(test)]
#[path = "coff_tests.rs"]
mod tests;
