//! The brush library target of the fuzzer: valid libraries to damage, the checksums put right
//! again after damage, and what is done with a library that was accepted.

use crate::craft::crc32;
use crate::mutate::Rng;
use pikslar_core::custom_brush::{Brush, BrushChoice, BrushPaint, Handle};
use pikslar_core::orient::Orientation;
use pikslar_core::{rgba, ColorMode, Document, DrawCmd, Editor, Tool};
use pikslar_format::brush_lib::{self, LibraryBrush, Paint};

/// Where the first record starts, and how long a record's length and checksum are.
const HEADER_LEN: usize = 12;
const RECORD_HEAD_LEN: usize = 8;

fn brush(name: &str, side: (u16, u16), mut pixel: impl FnMut(usize) -> u32) -> LibraryBrush {
    let (width, height) = side;
    let count = usize::from(width) * usize::from(height);
    LibraryBrush {
        name: name.to_owned(),
        width,
        height,
        paint: Paint::OwnColors,
        orientation: 3,
        handle: brush_lib::Handle::Corner(2),
        rgba: (0..count).flat_map(|i| pixel(i).to_le_bytes()).collect(),
    }
}

/// Libraries that are valid: one small brush, a few of mixed kinds with deflated and raw
/// records, a large one, and a full library of 64.
pub fn seeds() -> Result<Vec<Vec<u8>>, String> {
    let mut noise = Rng::new(11);
    let flat = |i: usize| if i % 3 == 0 { 0xff30_60c0 } else { 0 };
    let random = |_: usize| noise.next_u64() as u32 | 0xff00_0000;
    let small = brush("dot", (2, 2), |i| 0xff00_0000 | i as u32);
    let mixed = vec![
        small.clone(),
        brush("grain", (16, 16), random),
        LibraryBrush {
            paint: Paint::Shape,
            handle: brush_lib::Handle::Centre,
            ..brush("stripes", (9, 7), flat)
        },
        brush("wide", (256, 3), flat),
    ];
    let large = vec![brush("large", (256, 256), flat)];
    let full: Vec<LibraryBrush> = (0..brush_lib::MAX_BRUSHES)
        .map(|n| {
            brush(&format!("b{n}"), (8, 8), move |i| {
                (i * n) as u32 | 0xff00_0000
            })
        })
        .collect();
    let mut out = [vec![small], mixed, large, full]
        .iter()
        .map(|set| brush_lib::write_library(set).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    out.push(over_budget()?);
    Ok(out)
}

/// How many full-size brushes the over-budget seed holds: the pixels of 16 fill the budget
/// (4 MiB), so the 17th is refused by the reader as over it. This seed is the last of the list
/// ([`HEAVY_LIBRARY_SEEDS`]).
const OVER_BUDGET_BRUSHES: usize = 17;

/// How many seeds at the end of the list are heavy ones, drawn less often.
pub const HEAVY_LIBRARY_SEEDS: usize = 1;

/// A library the reader must cut short: more full-size (256 by 256) brushes than the pixel
/// budget holds, each a real deflate stream. The writer refuses such a set, so the file is
/// framed by hand from one-brush libraries; the pixels compress well, so the file stays small.
fn over_budget() -> Result<Vec<u8>, String> {
    let pixel = |n: usize| move |i: usize| if (i / 7 + n) % 3 == 0 { 0xff30_60c0 } else { 0 };
    let mut out = Vec::new();
    for n in 0..OVER_BUDGET_BRUSHES {
        let one = brush_lib::write_library(&[brush(&format!("full{n}"), (256, 256), pixel(n))])
            .map_err(|e| e.to_string())?;
        if out.is_empty() {
            out.extend_from_slice(&one[..HEADER_LEN]);
        }
        out.extend_from_slice(&one[HEADER_LEN..]);
    }
    // The header's count of records is the number put in.
    out[8..HEADER_LEN].copy_from_slice(&(OVER_BUDGET_BRUSHES as u32).to_le_bytes());
    let read = brush_lib::read_library(&out).map_err(|e| e.to_string())?;
    if read.brushes.len() >= OVER_BUDGET_BRUSHES || !read.report.over_limit {
        return Err("the over-budget seed does not go over budget".to_owned());
    }
    Ok(out)
}

/// Puts every record's checksum right for whatever its body now says, as far as the framing
/// still reads, so the damage reaches the code behind the checksum.
pub fn resealed(mut bytes: Vec<u8>) -> Vec<u8> {
    let mut pos = HEADER_LEN;
    while let Some(head_end) = pos
        .checked_add(RECORD_HEAD_LEN)
        .filter(|&e| e <= bytes.len())
    {
        let len = u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]);
        let Some(end) = head_end
            .checked_add(len as usize)
            .filter(|&e| e <= bytes.len())
        else {
            break;
        };
        let crc = crc32(&bytes[head_end..end]);
        bytes[pos + 4..pos + 8].copy_from_slice(&crc.to_le_bytes());
        pos = end;
    }
    bytes
}

/// What the program does with a library that was read: checks each brush against the writer's
/// rules, writes the set and reads it back, then turns one into a brush and paints with it in
/// a palette document and in an RGBA one.
pub fn use_library(library: &brush_lib::Library, rng: &mut Rng) -> Result<(), String> {
    for entry in &library.brushes {
        entry
            .check()
            .map_err(|e| format!("an accepted brush: {e}"))?;
    }
    let written = brush_lib::write_library(&library.brushes).map_err(|e| e.to_string())?;
    let again = brush_lib::read_library(&written).map_err(|e| e.to_string())?;
    if again.brushes != library.brushes || !again.report.is_clean() {
        return Err("an accepted library did not survive being written".to_owned());
    }
    match library.brushes.get(rng.below(library.brushes.len().max(1))) {
        Some(entry) => paint_with(entry, rng),
        None => Ok(()),
    }
}

/// The brush a library entry describes, as the program makes it.
fn to_brush(entry: &LibraryBrush) -> Option<Brush> {
    let colors: Vec<u32> = entry
        .rgba
        .chunks_exact(4)
        .map(|p| rgba(p[0], p[1], p[2], p[3]))
        .collect();
    let paint = match entry.paint {
        Paint::OwnColors => BrushPaint::OwnColors,
        Paint::Shape => BrushPaint::Shape,
    };
    let size = (entry.width.into(), entry.height.into());
    let brush = Brush::from_parts(&entry.name, size.0, size.1, &colors, None, paint).ok()?;
    let handle = match entry.handle {
        brush_lib::Handle::Centre => Handle::Centre,
        brush_lib::Handle::Corner(c) => Handle::Corner(c.min(3)),
    };
    let turned = brush.turned(Orientation::from_index(entry.orientation)?);
    Some(turned.with_handle(handle))
}

fn paint_with(entry: &LibraryBrush, rng: &mut Rng) -> Result<(), String> {
    let Some(brush) = to_brush(entry) else {
        return Ok(());
    };
    for mode in [ColorMode::Palette, ColorMode::Rgba] {
        let doc = Document::new(24, 24, mode).map_err(|e| e.to_string())?;
        let mut editor = Editor::new(doc);
        editor.set_brush(BrushChoice::Custom(brush.clone().into()));
        editor.color = rgba(200, 40, 90, 255);
        let (x, y) = (rng.between(0, 24), rng.between(0, 24));
        let begin = DrawCmd::Begin {
            tool: Tool::Pencil,
            x,
            y,
        };
        // Refused for room is allowed; a panic is not.
        if editor.apply(begin).is_ok() {
            let _ = editor.apply(DrawCmd::To {
                x: 23 - x,
                y: 23 - y,
            });
            let _ = editor.apply(DrawCmd::Commit);
        }
    }
    Ok(())
}
