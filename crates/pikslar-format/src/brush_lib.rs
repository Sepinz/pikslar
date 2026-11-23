//! The brush library file, `brushes.pkb` (M8, phase 8.3).
//!
//! ```text
//! header  "PKBR", u32 version (1), u32 count (at most 64)
//! record  u32 length, u32 CRC-32 of the body, body        (repeated `count` times)
//! body    u16 name length, name (UTF-8, at most 256 bytes), u8 paint, u8 orientation,
//!         u8 handle, u16 width, u16 height (1 to 256), u8 encoding (0 raw, 1 deflate),
//!         the RGBA rows (4 bytes a pixel, row by row)
//! ```
//!
//! All integers are little-endian. Every record carries its own checksum, so a record whose
//! body is damaged is skipped alone and the others load. A damaged *length* loses the framing,
//! so that record and the ones after it are lost; nothing is ever read past the end of the data.
//!
//! This module has its own plain record type. It knows nothing of the editor's brush model; the
//! application converts between the two.
//!
//! Limits, so that no input can make the reader hold much: at most 64 brushes, 256 x 256 pixels
//! each, 4 MiB of decoded pixels in all, and 16 MiB of file read.

#[path = "brush_lib_read.rs"]
mod read;
#[cfg(test)]
#[path = "brush_lib_tests.rs"]
mod tests;

use crate::{crc32, deflate, write_atomic, FormatError};
use std::io::Read;
use std::path::Path;

pub use read::{read_library, Library, Report};

/// The file's first four bytes.
pub const MAGIC: [u8; 4] = *b"PKBR";
/// The version this build writes and fully understands.
pub const VERSION: u32 = 1;
/// Most brushes in a library.
pub const MAX_BRUSHES: usize = 64;
/// Longest side of a brush, in pixels.
pub const MAX_SIDE: u16 = 256;
/// Most decoded pixel bytes in a whole library.
pub const MAX_PIXEL_BYTES: usize = 4 << 20;
/// Most bytes of a file that are read.
pub const MAX_FILE_BYTES: usize = 16 << 20;
/// Longest brush name, in bytes.
pub const MAX_NAME_BYTES: usize = 256;

pub(crate) const HEADER_LEN: usize = 12;
pub(crate) const RECORD_HEAD_LEN: usize = 8;
/// Fixed part of a body after the name: paint, orientation, handle, width, height, encoding.
pub(crate) const BODY_FIXED_LEN: usize = 3 + 2 + 2 + 1;
/// Largest possible body: a full-size raw brush with the longest name.
pub(crate) const MAX_BODY_LEN: usize =
    2 + MAX_NAME_BYTES + BODY_FIXED_LEN + MAX_SIDE as usize * MAX_SIDE as usize * 4;

/// What a brush's pixels mean when painting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// The brush paints its own colours.
    OwnColors,
    /// The brush is a mask painted in the current colour.
    Shape,
}

/// Where the brush is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Centre,
    /// One of the four corners, 0 to 3.
    Corner(u8),
}

/// One brush as the file holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryBrush {
    /// At most [`MAX_NAME_BYTES`] bytes of UTF-8.
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub paint: Paint,
    /// One of the 8 orientations, 0 to 7.
    pub orientation: u8,
    pub handle: Handle,
    /// `width * height * 4` bytes, red, green, blue, alpha, row by row.
    pub rgba: Vec<u8>,
}

impl LibraryBrush {
    /// Bytes the pixels of a `width` by `height` brush take.
    pub(crate) fn pixel_bytes(width: u16, height: u16) -> usize {
        width as usize * height as usize * 4
    }

    /// Why this brush cannot be written, if it cannot.
    pub fn check(&self) -> Result<(), FormatError> {
        if self.name.len() > MAX_NAME_BYTES {
            return Err(FormatError::Corrupt("brush name too long"));
        }
        if !(1..=MAX_SIDE).contains(&self.width) || !(1..=MAX_SIDE).contains(&self.height) {
            return Err(FormatError::TooLarge);
        }
        if self.orientation > 7 {
            return Err(FormatError::Corrupt("brush orientation out of range"));
        }
        if matches!(self.handle, Handle::Corner(c) if c > 3) {
            return Err(FormatError::Corrupt("brush handle out of range"));
        }
        if self.rgba.len() != Self::pixel_bytes(self.width, self.height) {
            return Err(FormatError::Corrupt("brush pixels do not match its size"));
        }
        Ok(())
    }
}

fn handle_byte(h: Handle) -> u8 {
    match h {
        Handle::Centre => 0,
        Handle::Corner(c) => 1 + c,
    }
}

fn paint_byte(p: Paint) -> u8 {
    match p {
        Paint::OwnColors => 0,
        Paint::Shape => 1,
    }
}

/// The body of one record. Deflate is used when it is smaller.
fn encode_body(brush: &LibraryBrush) -> Vec<u8> {
    let packed = deflate::deflate(&brush.rgba);
    let (encoding, pixels) = if packed.len() < brush.rgba.len() {
        (1u8, packed)
    } else {
        (0u8, brush.rgba.clone())
    };
    let mut body = Vec::with_capacity(2 + brush.name.len() + BODY_FIXED_LEN + pixels.len());
    body.extend_from_slice(&(brush.name.len() as u16).to_le_bytes());
    body.extend_from_slice(brush.name.as_bytes());
    body.push(paint_byte(brush.paint));
    body.push(brush.orientation);
    body.push(handle_byte(brush.handle));
    body.extend_from_slice(&brush.width.to_le_bytes());
    body.extend_from_slice(&brush.height.to_le_bytes());
    body.push(encoding);
    body.extend_from_slice(&pixels);
    body
}

/// The bytes of a library file. Refuses what the reader would not accept: too many brushes,
/// more than [`MAX_PIXEL_BYTES`] of pixels, or any brush that fails [`LibraryBrush::check`].
pub fn write_library(brushes: &[LibraryBrush]) -> Result<Vec<u8>, FormatError> {
    if brushes.len() > MAX_BRUSHES {
        return Err(FormatError::Unsupported(
            "more than 64 brushes in a library",
        ));
    }
    let mut total = 0usize;
    for b in brushes {
        b.check()?;
        total += b.rgba.len();
    }
    if total > MAX_PIXEL_BYTES {
        return Err(FormatError::FileTooLarge);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(brushes.len() as u32).to_le_bytes());
    for b in brushes {
        let body = encode_body(b);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&crc32(&[&body]).to_le_bytes());
        out.extend_from_slice(&body);
    }
    Ok(out)
}

/// Writes the library to `path`, replacing any file there only once the new one is whole.
pub fn save_library(path: &Path, brushes: &[LibraryBrush]) -> Result<(), FormatError> {
    write_atomic(path, &write_library(brushes)?)
}

/// Reads the library at `path`. At most [`MAX_FILE_BYTES`] are read; a longer file is cut there
/// and the report says so.
pub fn load_library(path: &Path) -> Result<Library, FormatError> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    read_library(&bytes)
}
