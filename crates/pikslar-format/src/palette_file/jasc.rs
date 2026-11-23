//! JASC palettes (Paint Shop Pro), `.pal`.
//!
//! # Read
//!
//! ```text
//! JASC-PAL
//! 0100
//! 3
//! 255 0 0
//! 0 255 0
//! 0 0 255
//! ```
//!
//! * Line 1 is `JASC-PAL`, line 2 is `0100`, line 3 is the colour count, a whole number. Spaces
//!   around them are ignored, `JASC-PAL` in any case. Any other version is refused.
//! * Then one `R G B` line per colour: exactly three numbers 0 to 255 split by spaces or tabs.
//!   Blank lines are skipped. A fourth number is refused, since the format has no alpha.
//! * If the count disagrees with the colours present, the colours present are kept and a note says
//!   so (a count of 256 over 16 colours is common in files from other tools). A file with no
//!   colours is refused.
//!
//! # Written
//!
//! `JASC-PAL`, `0100`, the count, then `R G B` per colour (plain numbers, single spaces). Line
//! ends are `\r\n`, as Paint Shop Pro writes them. Alpha is dropped, names are not written.

use super::{
    check_writable, component, prepare, token, Lines, PaletteData, PaletteError, PaletteRead,
    MAX_COLORS,
};

/// Reads a `.pal` file.
///
/// # Errors
/// A [`PaletteError`] with the line, for a bad header, count or value, or a limit.
pub fn read(bytes: &[u8]) -> Result<PaletteRead, PaletteError> {
    let body = prepare(bytes)?;
    let mut lines = Lines::new(body);
    let mut head = |n: usize, what: &str| -> Result<Vec<u8>, PaletteError> {
        match lines.next() {
            Some(Ok((_, l))) => Ok(l.trim_ascii().to_vec()),
            Some(Err(e)) => Err(e),
            None => Err(PaletteError::at(n, format!("The file ends before {what}"))),
        }
    };
    if !head(1, "'JASC-PAL'")?.eq_ignore_ascii_case(b"JASC-PAL") {
        return Err(PaletteError::at(
            1,
            "This is not a JASC palette: the first line must be 'JASC-PAL'",
        ));
    }
    if head(2, "the version '0100'")? != b"0100" {
        return Err(PaletteError::at(
            2,
            "Only JASC palette version 0100 is supported",
        ));
    }
    let declared = count(&head(3, "the color count")?)?;
    let colors = colours(lines)?;
    let mut notes = Vec::new();
    if colors.len() != declared {
        notes.push(format!(
            "The file says {declared} colors but holds {}; the {} found were kept",
            colors.len(),
            colors.len()
        ));
    }
    Ok(PaletteRead {
        palette: PaletteData::from_colors(colors),
        notes,
    })
}

fn count(text: &[u8]) -> Result<usize, PaletteError> {
    let value = std::str::from_utf8(text)
        .ok()
        .filter(|t| !t.is_empty() && t.len() <= 9 && t.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|t| t.parse::<usize>().ok());
    value.ok_or_else(|| PaletteError::at(3, "The third line must be the number of colors"))
}

fn colours(lines: Lines<'_>) -> Result<Vec<[u8; 4]>, PaletteError> {
    let mut colors: Vec<[u8; 4]> = Vec::new();
    for item in lines {
        let (number, raw) = item?;
        let mut rest = raw.trim_ascii();
        if rest.is_empty() {
            continue;
        }
        let mut rgb = [0u8; 3];
        for (i, slot) in rgb.iter_mut().enumerate() {
            let (word, after) = token(rest).ok_or_else(|| {
                PaletteError::at(
                    number,
                    format!("A color line needs three numbers (0 to 255); found {i}"),
                )
            })?;
            *slot = component(word, number)?;
            rest = after;
        }
        if !rest.trim_ascii().is_empty() {
            return Err(PaletteError::at(
                number,
                "A color line has exactly three numbers; found more",
            ));
        }
        if colors.len() >= MAX_COLORS {
            return Err(PaletteError::at(
                number,
                "The palette has more than 65,535 colors",
            ));
        }
        colors.push([rgb[0], rgb[1], rgb[2], 255]);
    }
    if colors.is_empty() {
        return Err(PaletteError::at(0, "The palette file has no colors"));
    }
    Ok(colors)
}

/// Writes a `.pal` file.
///
/// # Errors
/// A [`PaletteError`] when the palette is empty or has more than 65,535 colours.
pub fn write(palette: &PaletteData) -> Result<Vec<u8>, PaletteError> {
    check_writable(palette)?;
    let mut out = format!("JASC-PAL\r\n0100\r\n{}\r\n", palette.colors.len());
    out.reserve(palette.colors.len() * 13);
    for c in &palette.colors {
        out.push_str(&format!("{} {} {}\r\n", c[0], c[1], c[2]));
    }
    Ok(out.into_bytes())
}
