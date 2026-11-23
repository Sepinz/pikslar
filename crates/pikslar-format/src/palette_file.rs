//! Palette files: GIMP `.gpl`, JASC `.pal` and `.hex` (M9, phase 9.4).
//!
//! Readers turn the bytes of a file into a [`PaletteData`]; writers do the opposite. Nothing here
//! touches a document: the caller decides what to do with the colours.
//!
//! # What is accepted
//!
//! * At most [`MAX_FILE_BYTES`] bytes, [`MAX_COLORS`] colours, and [`MAX_LINE_BYTES`] bytes in a
//!   line (without its line end).
//! * A UTF-8 byte order mark at the start, and either `\n` or `\r\n` line ends. Spaces and tabs
//!   around values are ignored. A file in UTF-16 is refused with a message that says so.
//! * Every colour value is a whole number from 0 to 255 (hex files: exactly six hex digits).
//! * Anything else is refused with a [`PaletteError`] that names the line (counted from 1).
//!
//! The three formats carry no transparency, so every colour read has alpha 255 and the alpha of a
//! colour written is dropped. The one exception read is Aseprite's `Channels: RGBA` extension of
//! GIMP's format (see [`gpl`]); its alpha is read and dropped too, with a note.
//!
//! The format details are in [`gpl`], [`jasc`] and [`hex`]. Each says exactly what it reads and
//! what it writes.

use pikslar_core::LimitError;
use std::fmt;

pub mod gpl;
pub mod hex;
pub mod jasc;
pub mod table;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_channels;
#[cfg(test)]
mod tests_errors;

/// The largest palette file read, in bytes.
pub const MAX_FILE_BYTES: usize = 4 << 20;
/// The most colours one palette file may hold, read or written.
pub const MAX_COLORS: usize = 65_535;
/// The longest line read, in bytes, not counting the line end.
pub const MAX_LINE_BYTES: usize = 1_024;
/// The longest name written, in bytes. Longer names are cut at a character boundary so that the
/// line stays inside [`MAX_LINE_BYTES`].
pub const MAX_NAME_BYTES: usize = 200;

/// A palette as plain data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaletteData {
    /// The palette's own name, when the file has one (GIMP only).
    pub name: Option<String>,
    /// The colours in file order, as `[r, g, b, a]`.
    pub colors: Vec<[u8; 4]>,
    /// The name of each colour, when the file gives one (GIMP only). When not empty it has one
    /// entry per colour; writers accept it shorter, and treat the missing names as absent.
    pub names: Vec<Option<String>>,
}

impl PaletteData {
    /// A palette of unnamed colours.
    pub fn from_colors(colors: Vec<[u8; 4]>) -> Self {
        PaletteData {
            name: None,
            colors,
            names: Vec::new(),
        }
    }

    /// The name of colour `i`, if it has one.
    pub fn color_name(&self, i: usize) -> Option<&str> {
        self.names.get(i).and_then(|n| n.as_deref())
    }
}

/// A palette read, with what the reader changed or ignored that the caller may want to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteRead {
    /// The colours.
    pub palette: PaletteData,
    /// Plain-language notes, empty when the file was exactly as expected.
    pub notes: Vec<String>,
}

/// Why a palette file was refused or could not be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteError {
    /// The line the problem is on, counted from 1; 0 when it concerns the whole file.
    pub line: usize,
    /// What is wrong, in a sentence a person can act on.
    pub message: String,
}

impl PaletteError {
    pub(crate) fn at(line: usize, message: impl Into<String>) -> Self {
        PaletteError {
            line,
            message: message.into(),
        }
    }

    /// The error the document commands carry for this: the message itself is shown to the user.
    pub fn limit(&self) -> LimitError {
        LimitError::PaletteFile
    }
}

impl fmt::Display for PaletteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            f.write_str(&self.message)
        } else {
            write!(f, "Line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for PaletteError {}

/// The three palette file formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteFormat {
    /// GIMP palette, `.gpl`.
    Gpl,
    /// JASC palette, `.pal`.
    Jasc,
    /// One `RRGGBB` per line, `.hex`.
    Hex,
}

impl PaletteFormat {
    /// The format for a file extension (with or without the dot, any case).
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "gpl" => Some(PaletteFormat::Gpl),
            "pal" => Some(PaletteFormat::Jasc),
            "hex" => Some(PaletteFormat::Hex),
            _ => None,
        }
    }

    /// The extension written for this format, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            PaletteFormat::Gpl => "gpl",
            PaletteFormat::Jasc => "pal",
            PaletteFormat::Hex => "hex",
        }
    }

    /// The format a file's first line shows: `GIMP Palette`, `JASC-PAL`, or else hex.
    pub fn sniff(bytes: &[u8]) -> Self {
        let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        let first = body
            .split(|&b| b == b'\n')
            .next()
            .unwrap_or(&[])
            .trim_ascii();
        if first.eq_ignore_ascii_case(b"GIMP Palette") {
            PaletteFormat::Gpl
        } else if first.eq_ignore_ascii_case(b"JASC-PAL") {
            PaletteFormat::Jasc
        } else {
            PaletteFormat::Hex
        }
    }
}

/// Reads a palette file of the given format.
///
/// # Errors
/// Returns a [`PaletteError`] naming the line when the file breaks a rule of the format or a limit.
pub fn read(bytes: &[u8], format: PaletteFormat) -> Result<PaletteRead, PaletteError> {
    match format {
        PaletteFormat::Gpl => gpl::read(bytes),
        PaletteFormat::Jasc => jasc::read(bytes),
        PaletteFormat::Hex => hex::read(bytes),
    }
}

/// Reads a palette file, taking its format from the first line.
///
/// # Errors
/// As [`read`].
pub fn read_any(bytes: &[u8]) -> Result<PaletteRead, PaletteError> {
    read(bytes, PaletteFormat::sniff(bytes))
}

/// Writes a palette file of the given format.
///
/// # Errors
/// Returns a [`PaletteError`] when the palette is empty or holds more than [`MAX_COLORS`].
pub fn write(palette: &PaletteData, format: PaletteFormat) -> Result<Vec<u8>, PaletteError> {
    match format {
        PaletteFormat::Gpl => gpl::write(palette),
        PaletteFormat::Jasc => jasc::write(palette),
        PaletteFormat::Hex => hex::write(palette),
    }
}

// ---- shared helpers for the three formats ----

/// Checks the size, drops a byte order mark, and refuses UTF-16.
pub(crate) fn prepare(bytes: &[u8]) -> Result<&[u8], PaletteError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(PaletteError::at(
            0,
            "The file is larger than 4 MiB, which is more than any palette needs",
        ));
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(PaletteError::at(
            0,
            "The file is saved as UTF-16; save it as plain text (UTF-8 or ASCII)",
        ));
    }
    Ok(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes))
}

/// The lines of a file as `(number, bytes)`, number from 1, with `\r` before `\n` removed. A final
/// line end does not start another line. A line over [`MAX_LINE_BYTES`] is an error.
pub(crate) struct Lines<'a> {
    rest: &'a [u8],
    number: usize,
}

impl<'a> Lines<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Lines {
            rest: bytes,
            number: 0,
        }
    }
}

impl<'a> Iterator for Lines<'a> {
    type Item = Result<(usize, &'a [u8]), PaletteError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        self.number += 1;
        let end = self.rest.iter().position(|&b| b == b'\n');
        let (line, rest) = match end {
            Some(i) => (&self.rest[..i], &self.rest[i + 1..]),
            None => (self.rest, &self.rest[..0]),
        };
        self.rest = rest;
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.len() > MAX_LINE_BYTES {
            // Stop after the error so a caller that ignores it cannot loop on a bad file.
            self.rest = &[];
            return Some(Err(PaletteError::at(
                self.number,
                "The line is longer than 1,024 bytes",
            )));
        }
        Some(Ok((self.number, line)))
    }
}

/// The first word of `s` (split on spaces and tabs) and what follows it, or `None` if `s` is
/// blank.
pub(crate) fn token(s: &[u8]) -> Option<(&[u8], &[u8])> {
    let is_gap = |b: &u8| b.is_ascii_whitespace();
    let start = s.iter().position(|b| !is_gap(b))?;
    let s = &s[start..];
    let end = s.iter().position(is_gap).unwrap_or(s.len());
    Some((&s[..end], &s[end..]))
}

/// A colour value: ASCII digits only, 0 to 255.
pub(crate) fn component(word: &[u8], line: usize) -> Result<u8, PaletteError> {
    let bad = || {
        PaletteError::at(
            line,
            format!(
                "'{}' is not a colour value; use a whole number from 0 to 255",
                shown(word)
            ),
        )
    };
    if word.is_empty() || !word.iter().all(u8::is_ascii_digit) {
        return Err(bad());
    }
    let mut v = 0u32;
    for d in word {
        v = v * 10 + u32::from(d - b'0');
        if v > 255 {
            return Err(bad());
        }
    }
    u8::try_from(v).map_err(|_| bad())
}

/// Text of a file made safe to show in a message and to keep as a name: invalid UTF-8 replaced,
/// control, direction and zero-width characters turned into spaces, ends trimmed, and cut to a
/// short length.
pub(crate) fn clean(bytes: &[u8]) -> String {
    let text = pikslar_core::names::spaced(&String::from_utf8_lossy(bytes));
    cut(text.trim(), MAX_NAME_BYTES).to_string()
}

fn shown(word: &[u8]) -> String {
    let s = clean(word);
    cut(&s, 24).to_string()
}

/// `s` cut to at most `max` bytes at a character boundary.
pub(crate) fn cut(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Refuses a palette a writer cannot put in a file the readers accept.
pub(crate) fn check_writable(palette: &PaletteData) -> Result<(), PaletteError> {
    if palette.colors.is_empty() {
        return Err(PaletteError::at(0, "The palette has no colors to write"));
    }
    if palette.colors.len() > MAX_COLORS {
        return Err(PaletteError::at(
            0,
            "The palette has more than 65,535 colors, more than a palette file holds",
        ));
    }
    Ok(())
}

/// A name for writing: control characters turned to spaces, trimmed, cut; `None` if nothing is
/// left.
pub(crate) fn writable_name(name: Option<&str>) -> Option<String> {
    let name = clean(name?.as_bytes());
    (!name.is_empty()).then_some(name)
}
