//! Floating pixels in a recovery copy.
//!
//! A section of its own beside the untouched document. Masks and pasted pixels are
//! deflated, because a selection can be as large as the canvas.

use crate::deflate::deflate;
use crate::inflate::inflate;
use crate::piks::{charge, In, Out};
use crate::FormatError;
use pikslar_core::{ColorTable, FloatingSnapshot, Mask, MAX_DIMENSION};
use std::sync::Arc;

const FLIP: u8 = 1;
const COPY: u8 = 2;
const PASTED: u8 = 4;
const HAS_BEFORE: u8 = 8;
const HAS_PIXELS: u8 = 16;
const HAS_COLORS: u8 = 32;

const BAD_FLOATING: FormatError = FormatError::Corrupt("bad floating selection");

fn write_packed(out: &mut Out, raw: &[u8]) {
    let packed = deflate(raw);
    out.u32(packed.len() as u32);
    out.0.extend_from_slice(&packed);
}

fn write_mask(out: &mut Out, mask: &Mask) {
    let (x, y, w, h) = mask.bounds();
    out.i32(x);
    out.i32(y);
    out.i32(w);
    out.i32(h);
    let raw: Vec<u8> = mask.words().iter().flat_map(|w| w.to_le_bytes()).collect();
    write_packed(out, &raw);
}

fn flags(s: &FloatingSnapshot) -> u8 {
    [
        (s.flip, FLIP),
        (s.copy, COPY),
        (s.pasted, PASTED),
        (s.before.is_some(), HAS_BEFORE),
        (s.pixels.is_some(), HAS_PIXELS),
        (s.colors_before.is_some(), HAS_COLORS),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .fold(0, |all, (_, bit)| all | bit)
}

/// Serializes floating pixels for a recovery copy.
pub(crate) fn encode(s: &FloatingSnapshot) -> Vec<u8> {
    let mut out = Out(Vec::with_capacity(256));
    out.u32(s.frame as u32);
    out.u32(s.layer as u32);
    for value in [s.x, s.y, s.w, s.h] {
        out.i32(value);
    }
    out.u8(flags(s));
    out.u8(s.turns);
    write_mask(&mut out, &s.source);
    if let Some(before) = &s.before {
        write_mask(&mut out, before);
    }
    if let Some(pixels) = &s.pixels {
        let raw: Vec<u8> = pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
        write_packed(&mut out, &raw);
    }
    if let Some(colors) = &s.colors_before {
        out.u32s(colors.colors());
        out.u32s(colors.swatches());
    }
    out.0
}

/// Inflates `expected` bytes, charged to `spent` before they are made.
fn read_packed(
    input: &mut In<'_>,
    expected: usize,
    spent: &mut usize,
) -> Result<Vec<u8>, FormatError> {
    let len = input.u32()? as usize;
    let packed = input.bytes(len)?;
    charge(spent, expected)?;
    let raw = inflate(packed, expected)?;
    if raw.len() != expected {
        return Err(BAD_FLOATING);
    }
    Ok(raw)
}

fn read_mask(input: &mut In<'_>, spent: &mut usize) -> Result<Mask, FormatError> {
    let (x, y, w, h) = (input.i32()?, input.i32()?, input.i32()?, input.i32()?);
    // Checked before inflating, so a file cannot ask for more memory than an image can use.
    if w.max(h) as i64 > MAX_DIMENSION as i64 {
        return Err(BAD_FLOATING);
    }
    let words = Mask::word_count(w, h).ok_or(BAD_FLOATING)?;
    let raw = read_packed(input, words * 8, spent)?;
    // The words the mask keeps, made beside the bytes they come from.
    charge(spent, words * 8)?;
    let words = raw
        .chunks_exact(8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        .collect();
    Mask::from_words(x, y, w, h, words).ok_or(BAD_FLOATING)
}

fn read_pixels(
    input: &mut In<'_>,
    source: &Mask,
    spent: &mut usize,
) -> Result<Vec<u32>, FormatError> {
    let (_, _, w, h) = source.bounds();
    let expected = (w as usize)
        .checked_mul(h as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(BAD_FLOATING)?;
    let raw = read_packed(input, expected, spent)?;
    // The pixels kept, made beside the bytes they come from.
    charge(spent, expected)?;
    Ok(raw
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

/// Reads floating pixels, charging `spent` for what they hold as the rest of a project is
/// charged. Their fit with the document is checked when they are restored.
pub(crate) fn decode(bytes: &[u8], spent: &mut usize) -> Result<FloatingSnapshot, FormatError> {
    let mut input = In {
        data: bytes,
        pos: 0,
    };
    let (frame, layer) = (input.u32()? as usize, input.u32()? as usize);
    let (x, y, w, h) = (input.i32()?, input.i32()?, input.i32()?, input.i32()?);
    let bits = input.u8()?;
    let turns = input.u8()?;
    let has = |flag: u8| bits & flag != 0;
    let source = read_mask(&mut input, spent)?;
    let before = has(HAS_BEFORE)
        .then(|| read_mask(&mut input, spent).map(Arc::new))
        .transpose()?;
    let pixels = has(HAS_PIXELS)
        .then(|| read_pixels(&mut input, &source, spent).map(Arc::new))
        .transpose()?;
    let colors_before = if has(HAS_COLORS) {
        let colors = input.u32_list(65_536)?;
        let swatches = input.u32_list(65_536)?;
        // The lists read, and the table made of them.
        charge(spent, (colors.len() + swatches.len()) * 8)?;
        Some(Arc::new(ColorTable::from_parts(&colors, &swatches)))
    } else {
        None
    };
    if input.pos != bytes.len() {
        return Err(BAD_FLOATING);
    }
    Ok(FloatingSnapshot {
        frame,
        layer,
        x,
        y,
        w,
        h,
        flip: has(FLIP),
        turns,
        copy: has(COPY),
        pasted: has(PASTED),
        source: Arc::new(source),
        before,
        pixels,
        colors_before,
    })
}
