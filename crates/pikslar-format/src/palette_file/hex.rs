//! Hex palettes, `.hex`: one colour per line, as Lospec exports them.
//!
//! # Read
//!
//! ```text
//! 1a1c2c
//! #5d275d
//! B13E53
//! ```
//!
//! * Each non-blank line is exactly six hex digits, in either case, with an optional `#` before
//!   them. Spaces and tabs around the line are ignored. Blank lines are skipped.
//! * Everything else is refused with its line number: three or eight digits, a comment, extra
//!   words. A file with no colours is refused.
//!
//! # Written
//!
//! Six lower-case hex digits per line, no `#`, each line ended by `\n`. Lospec's own `.hex`
//! downloads (checked 2026-11-15) use upper case and `\r\n`; both are the same format, and this
//! reader takes either. Alpha and names are dropped.

use super::{check_writable, prepare, Lines, PaletteData, PaletteError, PaletteRead, MAX_COLORS};

/// Reads a `.hex` file.
///
/// # Errors
/// A [`PaletteError`] with the line, for a line that is not a colour, or a limit.
pub fn read(bytes: &[u8]) -> Result<PaletteRead, PaletteError> {
    let body = prepare(bytes)?;
    let mut colors: Vec<[u8; 4]> = Vec::new();
    for item in Lines::new(body) {
        let (number, raw) = item?;
        let line = raw.trim_ascii();
        if line.is_empty() {
            continue;
        }
        let digits = line.strip_prefix(b"#").unwrap_or(line);
        let color = parse(digits).ok_or_else(|| {
            PaletteError::at(
                number,
                "Expected a color of six hex digits, like 1a2b3c or #1a2b3c",
            )
        })?;
        if colors.len() >= MAX_COLORS {
            return Err(PaletteError::at(
                number,
                "The palette has more than 65,535 colors",
            ));
        }
        colors.push(color);
    }
    if colors.is_empty() {
        return Err(PaletteError::at(0, "The palette file has no colors"));
    }
    Ok(PaletteRead {
        palette: PaletteData::from_colors(colors),
        notes: Vec::new(),
    })
}

fn parse(digits: &[u8]) -> Option<[u8; 4]> {
    if digits.len() != 6 {
        return None;
    }
    let mut v = [0u8; 6];
    for (slot, d) in v.iter_mut().zip(digits) {
        *slot = u8::try_from((*d as char).to_digit(16)?).ok()?;
    }
    Some([v[0] << 4 | v[1], v[2] << 4 | v[3], v[4] << 4 | v[5], 255])
}

/// Writes a `.hex` file.
///
/// # Errors
/// A [`PaletteError`] when the palette is empty or has more than 65,535 colours.
pub fn write(palette: &PaletteData) -> Result<Vec<u8>, PaletteError> {
    check_writable(palette)?;
    let mut out = String::with_capacity(palette.colors.len() * 7);
    for c in &palette.colors {
        out.push_str(&format!("{:02x}{:02x}{:02x}\n", c[0], c[1], c[2]));
    }
    Ok(out.into_bytes())
}
