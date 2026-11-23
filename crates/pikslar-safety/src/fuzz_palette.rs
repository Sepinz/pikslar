//! The palette file targets of the fuzzer, one per format: valid files to damage, and what is
//! done with a file that was accepted.

use crate::mutate::Rng;
use pikslar_core::{rgba, ColorMode, Document, DrawCmd, Editor, LoadHow, PaletteCmd, Tool};
use pikslar_format::palette_file::{
    self, PaletteData, PaletteFormat, PaletteRead, MAX_COLORS, MAX_LINE_BYTES,
};
use pikslar_format::piks;
use std::sync::Arc;

/// The largest seed: big enough to reach the line and colour handling at scale, small enough
/// that 100,000 damaged copies stay quick.
const LARGE: usize = 4_096;

fn colors(count: usize, seed: u64) -> Vec<[u8; 4]> {
    let mut noise = Rng::new(seed);
    (0..count)
        .map(|_| {
            let n = noise.next_u64();
            [n as u8, (n >> 8) as u8, (n >> 16) as u8, 255]
        })
        .collect()
}

fn named(count: usize, seed: u64) -> PaletteData {
    PaletteData {
        name: Some("Sunset \u{e9}t\u{e9}".to_owned()),
        names: (0..count)
            .map(|i| (i % 3 != 0).then(|| format!("colour {i}")))
            .collect(),
        ..PaletteData::from_colors(colors(count, seed))
    }
}

/// Text a person or another tool might have written: comments, blank lines, a byte order mark,
/// Windows line ends, tabs, odd case and a JASC count that disagrees.
fn handmade(format: PaletteFormat) -> Vec<Vec<u8>> {
    let texts: &[&str] = match format {
        PaletteFormat::Gpl => &[
            "\u{feff}GIMP Palette\r\nName: Hand\r\nColumns: 8\r\n#\r\n255 128   0\tOrange\r\n\r\n  0\t0 0\r\n# end\r\n",
            "gimp palette\nName:\n 12 34 56 255 alpha as a name\n1 2 3\n",
            "GIMP Palette\nChannels: RGBA\n#\n  0   0   0   0 Transparent\n254  91  89 255 Red\n",
        ],
        PaletteFormat::Jasc => &[
            "JASC-PAL\r\n0100\r\n3\r\n255 0 0\r\n0 255 0\r\n\r\n0 0 255\r\n",
            "\u{feff}jasc-pal\n0100\n256\n10 20 30\n40\t50\t60\n",
        ],
        PaletteFormat::Hex => &[
            "1a1c2c\n#5d275d\nB13E53\n\n  ef7d57  \r\n",
            "\u{feff}#FFFFFF\r\n000000\r\n",
        ],
    };
    texts.iter().map(|t| t.as_bytes().to_vec()).collect()
}

/// Valid files of one format: one colour, a few, named ones, a large one, and hand-written text.
/// For GPL there are also files at the file limits, and the one with every colour a file may
/// hold comes last, as the single seed [`HEAVY_PALETTE_SEEDS`] counts.
pub fn seeds(format: PaletteFormat) -> Result<Vec<Vec<u8>>, String> {
    let palettes = [
        PaletteData::from_colors(colors(1, 1)),
        PaletteData::from_colors(colors(16, 2)),
        named(40, 3),
        named(LARGE, 4),
    ];
    let mut out = palettes
        .iter()
        .map(|p| palette_file::write(p, format).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    out.extend(handmade(format));
    if format == PaletteFormat::Gpl {
        out.push(gpl_with_line(MAX_LINE_BYTES));
    }
    let heavy = (format == PaletteFormat::Gpl).then(full_gpl);
    for seed in out.iter().chain(heavy.iter()) {
        palette_file::read(seed, format).map_err(|e| format!("a seed is refused: {e}"))?;
    }
    if let Some(heavy) = heavy {
        // Refused on purpose: damage starts from a line one byte past the most it may be.
        out.push(gpl_with_line(MAX_LINE_BYTES + 1));
        out.push(heavy);
    }
    Ok(out)
}

/// How many seeds at the end of a palette seed list are heavy ones, drawn less often.
pub const HEAVY_PALETTE_SEEDS: usize = 1;

/// A GPL file whose one colour line is `len` bytes long, not counting its line end.
fn gpl_with_line(len: usize) -> Vec<u8> {
    let head = "9 8 7 ";
    let mut line = head.to_owned();
    line.push_str(&"n".repeat(len.saturating_sub(head.len())));
    format!("GIMP Palette\n{line}\n").into_bytes()
}

/// A valid GPL file with as many colours as a file may hold, so damage steps over the limit.
/// Written by hand, without names: a name is one allocation per colour, and a read of this size
/// under many workers would be slowed by the allocator, not by the reader.
fn full_gpl() -> Vec<u8> {
    let mut text = String::from("GIMP Palette\n");
    for n in 0..MAX_COLORS {
        text.push_str(&format!("{} {} 7\n", n & 255, (n >> 8) & 255));
    }
    text.into_bytes()
}

/// A project of a palette document with swatches, two groups and a ramp: the `Pgrp` section,
/// for damage to start from.
pub fn grouped_project() -> Result<Vec<u8>, String> {
    let fail = |e: pikslar_core::LimitError| e.to_string();
    let mut editor = Editor::new(Document::new(12, 12, ColorMode::Palette).map_err(fail)?);
    for i in 0..6u8 {
        let color = rgba(i * 40, 255 - i * 30, 90, 255);
        editor
            .apply(PaletteCmd::AddSwatch { color })
            .map_err(fail)?;
    }
    for (name, ids) in [("Skin \u{e9}", vec![3, 1]), ("Hair", vec![2, 4, 5])] {
        let ids = Arc::from(ids);
        editor
            .apply(PaletteCmd::AddGroup {
                name: name.into(),
                ids,
            })
            .map_err(fail)?;
    }
    editor
        .apply(PaletteCmd::AddRamp {
            from: 1,
            to: 6,
            steps: 4,
        })
        .map_err(fail)?;
    let path = vec![(1, 1), (9, 9)];
    editor
        .apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path,
        })
        .map_err(fail)?;
    Ok(piks::save(editor.doc()))
}

/// What the program does with a palette file that was read: holds it to the reader's promises,
/// writes it and reads it back, then loads it into a document all three ways.
pub fn use_palette(read: &PaletteRead, format: PaletteFormat, rng: &mut Rng) -> Result<(), String> {
    let palette = &read.palette;
    if palette.colors.is_empty() || palette.colors.len() > MAX_COLORS {
        return Err(format!("accepted {} colours", palette.colors.len()));
    }
    if palette.colors.iter().any(|c| c[3] != 255) {
        return Err("a colour read with alpha".to_owned());
    }
    if !palette.names.is_empty() && palette.names.len() != palette.colors.len() {
        return Err("names and colours differ in number".to_owned());
    }
    let written = palette_file::write(palette, format).map_err(|e| e.to_string())?;
    let again = palette_file::read(&written, format).map_err(|e| format!("own file: {e}"))?;
    if again.palette.colors != palette.colors {
        return Err("an accepted palette did not survive being written".to_owned());
    }
    let twice = palette_file::write(&again.palette, format).map_err(|e| e.to_string())?;
    if twice != written {
        return Err("writing a palette read back gave other bytes".to_owned());
    }
    if palette_file::PaletteFormat::sniff(&written) != format {
        return Err("a written file is taken for another format".to_owned());
    }
    load_into_documents(palette, rng)
}

/// Loads the colours into a small palette document each way. Refusals for room or size are
/// allowed; a panic, or a load that moves a pixel id, is not.
fn load_into_documents(palette: &PaletteData, rng: &mut Rng) -> Result<(), String> {
    let colors: Arc<[u32]> = palette.packed().into();
    for how in [LoadHow::Append, LoadHow::Replace, LoadHow::AsColors] {
        let doc = Document::new(8, 8, ColorMode::Palette).map_err(|e| e.to_string())?;
        let mut editor = Editor::new(doc);
        editor.color = rgba(250, 7, 200, 255);
        let path = vec![(0, 0), (7, 5)];
        let tool = Tool::Pencil;
        editor
            .apply(DrawCmd::Stroke { tool, path })
            .map_err(|e| e.to_string())?;
        for i in 0..1 + rng.below(colors.len().min(12)) {
            let color = rgba(i as u8, 7, 200, 255);
            let _ = editor.apply(PaletteCmd::AddSwatch { color });
        }
        if how == LoadHow::AsColors && rng.chance(2) {
            let wanted = colors.len().min(64);
            while editor.doc().colors.swatches().len() < wanted {
                let n = editor.doc().colors.len() as u8;
                let color = rgba(n, n.wrapping_mul(3), 1, 255);
                if editor.apply(PaletteCmd::AddSwatch { color }).is_err() {
                    break;
                }
            }
        }
        let before = editor.doc().layers()[0].cel(0).clone();
        let command = PaletteCmd::Load {
            colors: colors.clone(),
            how,
        };
        let _ = editor.apply(command);
        if editor.doc().layers()[0].cel(0) != &before {
            return Err(format!("loading {how:?} changed pixels"));
        }
    }
    Ok(())
}
