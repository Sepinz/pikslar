//! GIMP palettes, `.gpl`.
//!
//! # Read
//!
//! ```text
//! GIMP Palette
//! Name: Sunset
//! Columns: 4
//! # a comment
//! 255 128   0  Orange
//! ```
//!
//! * The first line is `GIMP Palette` (any case, spaces around it ignored). Nothing may come
//!   before it, not even a blank line.
//! * Then, before the first colour, an optional `Name:`, an optional `Columns:` (a whole
//!   number from 0 to 256, otherwise refused; it is not kept, a palette here has no columns) and
//!   an optional `Channels:` (below).
//! * Blank lines and lines whose first non-blank character is `#` are skipped, anywhere.
//! * A colour line is three numbers 0 to 255 (red, green, blue) split by spaces or tabs, then the
//!   colour's name, which is the rest of the line and may be empty. Names are not kept by
//!   the program; when any colour has one (other than `Untitled`), a note says how many.
//! * **Alpha:** GIMP's format has none. Aseprite's extension (its `docs/gpl-palette-extension.md`)
//!   marks it with a `Channels: RGBA` line before the first colour; then every colour line has a
//!   fourth number, the alpha (0 to 255), before the name. Such a file is read, every colour comes
//!   in opaque, and a note says how many were partly clear. `Channels: RGB` is the plain format;
//!   any other value is refused. Without the line, a fourth number is read as the start of the
//!   name, since a name can be a number.
//! * Anything else (a number out of range, fewer than three numbers, a line that is none of the
//!   above) is refused with its line number. A file with no colours is refused.
//!
//! # Written
//!
//! `GIMP Palette`, `Name: <name or Untitled>`, `Columns: 0`, `#`, then one line per colour in the
//! form GIMP writes: each value right-aligned in three characters, separated by single spaces,
//! then a tab and the colour's name (`Untitled` when it has none). Line ends are `\n`. Names have
//! control characters turned to spaces and are cut at 200 bytes. Alpha is dropped.

use super::{
    check_writable, clean, component, prepare, token, writable_name, Lines, PaletteData,
    PaletteError, PaletteRead, MAX_COLORS,
};

const HEADER: &[u8] = b"GIMP Palette";

/// Reads a `.gpl` file.
///
/// # Errors
/// A [`PaletteError`] with the line, for a missing header, a bad value, or a limit.
pub fn read(bytes: &[u8]) -> Result<PaletteRead, PaletteError> {
    let body = prepare(bytes)?;
    let mut lines = Lines::new(body);
    match lines.next() {
        Some(Ok((_, first))) if first.trim_ascii().eq_ignore_ascii_case(HEADER) => {}
        Some(Err(e)) => return Err(e),
        _ => {
            return Err(PaletteError::at(
                1,
                "This is not a GIMP palette: the first line must be 'GIMP Palette'",
            ))
        }
    }
    let mut body = Body::default();
    for item in lines {
        let (number, raw) = item?;
        let line = raw.trim_ascii();
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        if body.palette.colors.is_empty() && body.header_field(line, number)? {
            continue;
        }
        body.colour_line(line, number)?;
    }
    if body.palette.colors.is_empty() {
        return Err(PaletteError::at(0, "The palette file has no colors"));
    }
    Ok(body.finish())
}

/// What has been read of a file so far.
#[derive(Default)]
struct Body {
    palette: PaletteData,
    /// `Channels: RGBA` was given: each colour line has a fourth number, its alpha.
    rgba: bool,
    /// How many colours had an alpha other than 255.
    partly_clear: usize,
}

impl Body {
    /// Reads `Name:`, `Columns:` or `Channels:`; false if the line is none of them.
    fn header_field(&mut self, line: &[u8], number: usize) -> Result<bool, PaletteError> {
        if let Some(rest) = line.strip_prefix(b"Name:") {
            let name = clean(rest);
            self.palette.name = (!name.is_empty()).then_some(name);
        } else if let Some(rest) = line.strip_prefix(b"Columns:") {
            columns(rest, number)?;
        } else if let Some(rest) = line.strip_prefix(b"Channels:") {
            self.rgba = channels(rest, number)?;
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    fn finish(self) -> PaletteRead {
        // `Untitled` is what GIMP (and this writer) put where a colour has no name.
        let real = |n: &&String| !n.eq_ignore_ascii_case("Untitled");
        let named = self.palette.names.iter().flatten().filter(real).count();
        let mut notes = Vec::new();
        if named > 0 {
            notes.push(format!(
                "{named} color name(s) in the file are not kept: swatches have no names"
            ));
        }
        if self.partly_clear > 0 {
            notes.push(format!(
                "{} color(s) in the file are partly clear; they are brought in opaque",
                self.partly_clear
            ));
        }
        PaletteRead {
            palette: self.palette,
            notes,
        }
    }
}

/// The value of Aseprite's `Channels:` field: true for `RGBA`, false for `RGB`.
fn channels(rest: &[u8], line: usize) -> Result<bool, PaletteError> {
    match rest.trim_ascii() {
        b"RGB" => Ok(false),
        b"RGBA" => Ok(true),
        _ => Err(PaletteError::at(
            line,
            "'Channels:' must be followed by RGB or RGBA",
        )),
    }
}

fn columns(rest: &[u8], line: usize) -> Result<(), PaletteError> {
    let text = rest.trim_ascii();
    let ok = !text.is_empty()
        && text.len() <= 3
        && text.iter().all(u8::is_ascii_digit)
        && std::str::from_utf8(text)
            .ok()
            .and_then(|t| t.parse::<u32>().ok())
            .is_some_and(|n| n <= 256);
    if ok {
        Ok(())
    } else {
        Err(PaletteError::at(
            line,
            "'Columns:' must be followed by a whole number from 0 to 256",
        ))
    }
}

impl Body {
    fn colour_line(&mut self, line: &[u8], number: usize) -> Result<(), PaletteError> {
        let wanted = if self.rgba { 4 } else { 3 };
        let mut rest = line;
        let mut values = [255u8; 4];
        for (i, slot) in values.iter_mut().take(wanted).enumerate() {
            let Some((word, after)) = token(rest) else {
                return Err(short_line(i, self.rgba, number));
            };
            *slot = component(word, number)?;
            rest = after;
        }
        if self.palette.colors.len() >= MAX_COLORS {
            return Err(PaletteError::at(
                number,
                "The palette has more than 65,535 colors",
            ));
        }
        self.partly_clear += usize::from(values[3] != 255);
        let name = clean(rest);
        self.palette
            .colors
            .push([values[0], values[1], values[2], 255]);
        self.palette.names.push((!name.is_empty()).then_some(name));
        Ok(())
    }
}

/// The refusal of a colour line that ends after `found` numbers.
fn short_line(found: usize, rgba: bool, number: usize) -> PaletteError {
    let what = if rgba {
        "red, green, blue and alpha (0 to 255), as 'Channels: RGBA' says"
    } else {
        "red, green and blue (0 to 255)"
    };
    let plural = if found == 1 { "" } else { "s" };
    PaletteError::at(
        number,
        format!("A color line needs {what}; found {found} number{plural}"),
    )
}

/// Writes a `.gpl` file.
///
/// # Errors
/// A [`PaletteError`] when the palette is empty or has more than 65,535 colours.
pub fn write(palette: &PaletteData) -> Result<Vec<u8>, PaletteError> {
    check_writable(palette)?;
    let name = writable_name(palette.name.as_deref()).unwrap_or_else(|| "Untitled".into());
    let mut out = format!("GIMP Palette\nName: {name}\nColumns: 0\n#\n");
    out.reserve(palette.colors.len() * 24);
    for (i, c) in palette.colors.iter().enumerate() {
        let label = writable_name(palette.color_name(i)).unwrap_or_else(|| "Untitled".into());
        out.push_str(&format!("{:3} {:3} {:3}\t{label}\n", c[0], c[1], c[2]));
    }
    Ok(out.into_bytes())
}
