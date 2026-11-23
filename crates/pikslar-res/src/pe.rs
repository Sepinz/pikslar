//! Reading a built PE32+ program for the release checks: its time stamp, its import table and
//! a scan for text such as a user or workspace path. No `dumpbin` or `objdump` is assumed.
//! Never trusted with anything beyond the program files the release script built.

use crate::bytes::{get16, get32, slice};

/// One imported DLL and what is taken from it (`#n` for an import by ordinal).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    pub dll: String,
    pub functions: Vec<String>,
}

/// The parts of the headers the readers need.
struct Headers {
    coff: usize,
    opt: usize,
    sections: Vec<(u32, u32, u32, u32)>,
}

fn headers(b: &[u8]) -> Result<Headers, String> {
    if slice(b, 0, 2)? != b"MZ" {
        return Err("not an MZ file".into());
    }
    let pe = get32(b, 0x3C)? as usize;
    if slice(b, pe, 4)? != b"PE\0\0" {
        return Err("not a PE file".into());
    }
    let coff = pe + 4;
    let opt = coff + 20;
    if get16(b, opt)? != 0x20B {
        return Err("not a PE32+ file".into());
    }
    let table = opt + get16(b, coff + 16)? as usize;
    let sections = (0..get16(b, coff + 2)? as usize)
        .map(|i| {
            let s = table + 40 * i;
            Ok((
                get32(b, s + 12)?,
                get32(b, s + 8)?,
                get32(b, s + 20)?,
                get32(b, s + 16)?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Headers {
        coff,
        opt,
        sections,
    })
}

impl Headers {
    /// The file offset of a relative virtual address.
    fn map(&self, rva: u32) -> Result<usize, String> {
        self.sections
            .iter()
            .find(|(va, size, _, raw_size)| rva >= *va && rva < va + (*size).max(*raw_size))
            .map(|(va, _, raw, _)| (raw + rva - va) as usize)
            .ok_or_else(|| format!("address {rva:#x} is in no section"))
    }

    /// The address and size of data directory `index`.
    fn directory(&self, b: &[u8], index: usize) -> Result<(u32, u32), String> {
        if index >= get32(b, self.opt + 108)? as usize {
            return Ok((0, 0));
        }
        let at = self.opt + 112 + 8 * index;
        Ok((get32(b, at)?, get32(b, at + 4)?))
    }
}

/// The time stamp in the COFF header (zero when the linker was told not to write one).
pub fn timestamp(b: &[u8]) -> Result<u32, String> {
    get32(b, headers(b)?.coff + 4)
}

/// A NUL-terminated ASCII name at file offset `at`, at most 512 bytes.
fn name_at(b: &[u8], at: usize) -> Result<String, String> {
    let rest = b.get(at..).ok_or("a name outside the file")?;
    let end = rest
        .iter()
        .take(512)
        .position(|&c| c == 0)
        .ok_or("an unterminated name")?;
    String::from_utf8(rest[..end].to_vec()).map_err(|_| "a name that is not ASCII".into())
}

fn get64(b: &[u8], at: usize) -> Result<u64, String> {
    let s = slice(b, at, 8)?;
    let mut x = [0u8; 8];
    x.copy_from_slice(s);
    Ok(u64::from_le_bytes(x))
}

/// The functions one lookup table names.
fn functions(b: &[u8], h: &Headers, table: u32) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let start = h.map(table)?;
    for i in 0..65_536 {
        let entry = get64(b, start + 8 * i)?;
        if entry == 0 {
            return Ok(out);
        }
        if entry & (1 << 63) != 0 {
            out.push(format!("#{}", entry & 0xFFFF));
        } else {
            let rva = u32::try_from(entry).map_err(|_| "a bad lookup entry")?;
            out.push(name_at(b, h.map(rva)? + 2)?);
        }
    }
    Err("an import lookup table without an end".into())
}

/// Every DLL in the import table (directory 1) and the delay-load table (directory 13), in
/// stored order.
pub fn imports(b: &[u8]) -> Result<Vec<Import>, String> {
    let h = headers(b)?;
    let mut out = Vec::new();
    // (directory, descriptor size, name field, lookup table field)
    for (dir, size, name, lookup) in [(1, 20, 12, 0), (13, 32, 4, 16)] {
        let (rva, _) = h.directory(b, dir)?;
        if rva == 0 {
            continue;
        }
        let start = h.map(rva)?;
        for i in 0..4096 {
            let d = slice(b, start + size * i, size)?;
            if d.iter().all(|&x| x == 0) {
                break;
            }
            let table = match get32(d, lookup)? {
                0 => get32(d, if dir == 1 { 16 } else { 12 })?,
                t => t,
            };
            out.push(Import {
                dll: name_at(b, h.map(get32(d, name)?)?)?,
                functions: functions(b, &h, table)?,
            });
        }
    }
    Ok(out)
}

/// Whether a DLL is part of every Windows 10 and 11 installation: the Win32 system DLLs, the
/// Universal CRT's `api-ms-win-*` sets and the system's own `msvcrt.dll`. Anything a program
/// would have to ship beside itself (the MinGW runtime DLLs, the Visual C++ runtime) is not.
pub fn is_system_dll(name: &str) -> bool {
    const SYSTEM: &[&str] = &[
        "kernel32.dll",
        "user32.dll",
        "gdi32.dll",
        "ntdll.dll",
        "advapi32.dll",
        "shell32.dll",
        "ole32.dll",
        "oleaut32.dll",
        "comdlg32.dll",
        "shlwapi.dll",
        "uxtheme.dll",
        "dwmapi.dll",
        "imm32.dll",
        "winmm.dll",
        "version.dll",
        "bcrypt.dll",
        "bcryptprimitives.dll",
        "userenv.dll",
        "ws2_32.dll",
        "shcore.dll",
        "msvcrt.dll",
        "ucrtbase.dll",
        "msimg32.dll",
        "dbghelp.dll",
    ];
    let lower = name.to_ascii_lowercase();
    lower.starts_with("api-ms-win-") || SYSTEM.contains(&lower.as_str())
}

/// Where `needle` occurs in `b`, ignoring ASCII case, as single bytes or as UTF-16LE.
pub fn find_text(b: &[u8], needle: &str) -> Vec<usize> {
    let narrow: Vec<u8> = needle.bytes().map(|c| c.to_ascii_lowercase()).collect();
    let wide: Vec<u8> = needle
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if narrow.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for pattern in [narrow, wide] {
        let first = pattern[0];
        for at in 0..b.len().saturating_sub(pattern.len() - 1) {
            if b[at].to_ascii_lowercase() == first
                && b[at..at + pattern.len()]
                    .iter()
                    .zip(&pattern)
                    .all(|(x, y)| x.to_ascii_lowercase() == *y)
            {
                out.push(at);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
#[path = "pe_tests.rs"]
mod tests;
