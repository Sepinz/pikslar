//! The colours an indexed export is written with: those of the image itself, found by drawing
//! it once before anything is written. Nothing is changed to make an image fit a palette.
//!
//! Colours are found at the document's own size: enlarging by whole pixels adds none. What is
//! held is one band of rows and at most 257 colours, whatever the image's size.
//!
//! The order is the document's palette order when every colour the image uses is a swatch (see
//! [`in_palette_order`]); otherwise it is the order of value described on [`Palette`].

use crate::{ExportError, Picture};
use pikslar_core::Document;
use pikslar_format::png_index::{depth_for, MAX_COLOURS};
use std::collections::HashMap;
use std::fmt::Write;

/// The colours of an indexed image, each exactly as the RGBA export would write it. When every
/// colour the image uses, apart from fully clear pixels, is one of the document's swatches, the
/// palette is in palette order: the clear colour first if the image has it, then the colours the
/// image uses in the order of the swatches (a colour two swatches share goes where the first is;
/// swatches the image does not use are left out, since a palette holds exactly its image's
/// colours). Otherwise, and for every document with no swatches, those not opaque come first,
/// then the opaque, each group in order of value. Either way the same image always gets the same
/// palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    colours: Vec<u32>,
}

impl Palette {
    /// The straight RGBA colours, in the order written.
    pub fn colours(&self) -> &[u32] {
        &self.colours
    }

    /// The bits a pixel the image is written with.
    pub fn depth(&self) -> u8 {
        depth_for(self.colours.len())
    }
}

/// `sorted`, the distinct colours an image uses in order of value, in the order of `doc`'s
/// swatches, or None if the image uses a colour that is not a swatch. Fully clear (0) is not a
/// swatch and may be present: it goes first, as the clear entry. A clear colour with anything
/// stored behind it is a colour no swatch can be, so it makes this None.
pub(crate) fn in_palette_order(doc: &Document, sorted: &[u32]) -> Option<Vec<u32>> {
    let table = &doc.colors;
    let mut rank: HashMap<u32, usize> = HashMap::new();
    for (at, &id) in table.swatches().iter().enumerate() {
        rank.entry(table.color(id)).or_insert(at);
    }
    let mut clear = Vec::new();
    let mut swatches = Vec::with_capacity(sorted.len());
    for &colour in sorted {
        if colour == 0 {
            clear.push(0);
        } else {
            swatches.push((*rank.get(&colour)?, colour));
        }
    }
    pikslar_core::sorting::sort_packed_wide(
        &mut swatches,
        |&(rank, colour)| ((rank as u128) << 64) | u128::from(colour),
        |key| ((key >> 64) as u64 as usize, key as u32),
    );
    clear.extend(swatches.into_iter().map(|(_, colour)| colour));
    Some(clear)
}

/// What a sheet's description says of its pixels: `RGBA8888`, as before indexed exports
/// existed, or `INDEXED` with the bits a pixel and the palette's size.
pub(crate) fn push_format(out: &mut String, palette: Option<&Palette>) {
    match palette {
        None => out.push_str("\"format\":\"RGBA8888\""),
        Some(p) => {
            out.push_str("\"format\":\"INDEXED\",");
            push_palette_fields(out, p);
        }
    }
}

/// The bits a pixel and the palette's size of one indexed image.
pub(crate) fn push_palette_fields(out: &mut String, palette: &Palette) {
    let _ = write!(
        out,
        "\"bitDepth\":{},\"paletteSize\":{}",
        palette.depth(),
        palette.colours.len()
    );
}

/// For an indexed picture, the palette of `frames` and, if `gaps`, the space between them:
/// found while the export is planned, so one with too many colours writes nothing. `frame`
/// names the frame a refusal is about, or none for several together.
pub(crate) fn palette_for(
    picture: &Picture<'_>,
    frames: impl IntoIterator<Item = usize>,
    (gaps, frame): (bool, Option<usize>),
) -> Result<Option<Palette>, ExportError> {
    if !picture.indexed() {
        return Ok(None);
    }
    palette_of(picture, frames, gaps)
        .map(Some)
        .map_err(|hidden| ExportError::TooManyColours { frame, hidden })
}

/// Colours seen so far, sorted, and the last one seen.
pub(crate) struct Found {
    sorted: Vec<u32>,
    last: Option<u32>,
}

impl Found {
    pub(crate) fn new() -> Self {
        Found {
            sorted: Vec::with_capacity(MAX_COLOURS + 1),
            last: None,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.sorted.len()
    }

    pub(crate) fn into_sorted(self) -> Vec<u32> {
        self.sorted
    }

    /// Adds `colour`. False once there are more than a palette holds.
    pub(crate) fn add(&mut self, colour: u32) -> bool {
        if self.last == Some(colour) {
            return true;
        }
        self.last = Some(colour);
        if let Err(at) = self.sorted.binary_search(&colour) {
            self.sorted.insert(at, colour);
        }
        self.sorted.len() <= MAX_COLOURS
    }
}

/// The palette of an image showing `frames` of `picture`, and, if `gaps`, the clear space
/// between them. If it has more colours than a palette holds, the error is how many of the
/// colours found are clear with a colour other than black stored behind them: an export keeps
/// those as they are, so they count. Colours past the 257th are not looked at.
#[inline(never)]
pub fn palette_of(
    picture: &Picture<'_>,
    frames: impl IntoIterator<Item = usize>,
    gaps: bool,
) -> Result<Palette, usize> {
    let mut found = Found::new();
    if gaps {
        found.add(0);
    }
    let mut band = Vec::new();
    for frame in frames {
        for index in 0..picture.bands() {
            picture.band(frame, index, &mut band);
            if !band.iter().all(|&c| found.add(c)) {
                return Err(found
                    .sorted
                    .iter()
                    .filter(|&&c| c >> 24 == 0 && c != 0)
                    .count());
            }
        }
    }
    let ordered = in_palette_order(picture.doc(), &found.sorted);
    Ok(Palette {
        colours: ordered.unwrap_or_else(|| by_value(found.sorted)),
    })
}

/// The order of value: those not opaque first, then the opaque.
fn by_value(sorted: Vec<u32>) -> Vec<u32> {
    let (mut colours, opaque): (Vec<u32>, Vec<u32>) =
        sorted.into_iter().partition(|c| c >> 24 != 255);
    colours.extend(opaque);
    colours
}

#[cfg(test)]
#[path = "palette_order_tests.rs"]
mod order_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::{ColorMode, Document, Grid};

    fn doc(cells: &[u32], w: usize) -> Document {
        let h = cells.len() / w;
        let mut d = Document::new(w, h, ColorMode::Rgba).expect("valid");
        d.set_cel(0, 0, Grid::from_cells(w, h, cells)).expect("cel");
        d
    }

    #[test]
    fn colours_not_opaque_come_first_then_the_opaque_each_in_order() {
        let cells = [
            0xff00_0002,
            0x8000_0000,
            0,
            0xff00_0001,
            0x8000_0000,
            0x0100_0000,
        ];
        let d = doc(&cells, 3);
        let palette = palette_of(&Picture::visible(&d), [0], false).expect("fits");
        assert_eq!(
            palette.colours(),
            [0, 0x0100_0000, 0x8000_0000, 0xff00_0001, 0xff00_0002]
        );
    }

    #[test]
    fn the_space_between_frames_is_a_colour_only_if_asked() {
        let d = doc(&[0xff00_0001; 4], 2);
        let picture = Picture::visible(&d);
        let alone = palette_of(&picture, [0], false).expect("fits");
        assert_eq!(alone.colours(), [0xff00_0001]);
        let spaced = palette_of(&picture, [0], true).expect("fits");
        assert_eq!(spaced.colours(), [0, 0xff00_0001]);
    }

    #[test]
    fn exactly_256_colours_fit_and_257_do_not() {
        let mut cells: Vec<u32> = (0..256).map(|i| 0xff00_0000 | i).collect();
        let d = doc(&cells, 16);
        let palette = palette_of(&Picture::visible(&d), [0], false).expect("fits");
        assert_eq!(palette.colours().len(), 256);
        assert!(palette_of(&Picture::visible(&d), [0], true).is_err());
        cells[255] = 0xff00_0100;
        cells.extend(std::iter::repeat_n(0xff00_00ff, 16));
        let d = doc(&cells, 16);
        assert_eq!(palette_of(&Picture::visible(&d), [0], false), Err(0));
        let hidden: Vec<u32> = (0..257).map(|i| i << 8).collect();
        let d = doc(&hidden, 257);
        assert_eq!(
            palette_of(&Picture::visible(&d), [0], false),
            Err(256),
            "the clear black is not counted"
        );
    }

    #[test]
    fn every_frame_named_adds_its_colours() {
        let mut d = doc(&[0xff00_0001; 4], 2);
        d.add_frame(1, false).expect("room");
        d.set_cel(0, 1, Grid::from_cells(2, 2, &[0xff00_0002; 4]))
            .expect("cel");
        let picture = Picture::visible(&d);
        let both = palette_of(&picture, [0, 1], false).expect("fits");
        assert_eq!(both.colours(), [0xff00_0001, 0xff00_0002]);
        let second = palette_of(&picture, [1], false).expect("fits");
        assert_eq!(second.colours(), [0xff00_0002]);
    }
}
