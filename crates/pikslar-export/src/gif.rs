//! Animated and still GIFs: the frames chosen, in the order they play, with one palette of
//! exactly their colours. A GIF pixel is opaque or clear, so a translucent pixel is refused
//! rather than changed, and more colours than the palette holds are refused rather than merged.
//!
//! Colours are found while the export is planned, each frame drawn once at the document's own
//! size, holding one band and at most 257 colours. Then the frames are compared in play order to
//! find what each must draw (`gif_plan`). Writing draws each frame's rectangle again, a band at
//! a time. No frame is held whole.

use crate::gif_plan::{gif_plan, Step};
use crate::outputs::Output;
use crate::palette::{in_palette_order, Found};
use crate::stream::{scaled, send_area};
use crate::{io_error, ExportError, Picture};
use pikslar_core::{cumulative, Document, MAX_DURATION_MS};
use pikslar_format::gif_write::GifStream;
use pikslar_format::png_index::MAX_COLOURS;
use std::io::Write;
use std::path::PathBuf;

/// The shortest delay a GIF is given, in hundredths: most players show 0 or 1 as 100 ms.
pub const SHORTEST_DELAY: u16 = 2;

/// A GIF's colours, and whether any pixel is clear. The opaque ones are in palette order if every
/// colour is a swatch of the document, as for an indexed PNG (`Palette`), else in order of value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GifPalette {
    pub opaque: Vec<u32>,
    pub clear: bool,
}

/// The first pixel of `band`, row `first_row` on, that is neither opaque nor clear.
fn translucent(band: &[u32], width: usize, first_row: usize) -> Option<(usize, usize, u8)> {
    let at = band.iter().position(|&c| !matches!(c >> 24, 0 | 255))?;
    Some((at % width, first_row + at / width, (band[at] >> 24) as u8))
}

/// The palette of every frame in `frames`, or why a GIF cannot show them as they are. A pixel
/// no GIF can show is named before too many colours, wherever each is found, so fixing the one
/// reported never uncovers the other; once there are too many, only alpha is looked at.
#[inline(never)]
pub fn gif_palette(
    picture: &Picture<'_>,
    frames: impl IntoIterator<Item = usize>,
) -> Result<GifPalette, ExportError> {
    let width = picture.doc().width.max(1);
    let (mut found, mut clear, mut band) = (Some(Found::new()), false, Vec::new());
    for frame in frames {
        let mut first_row = 0;
        for index in 0..picture.bands() {
            picture.band(frame, index, &mut band);
            if let Some((x, y, alpha)) = translucent(&band, width, first_row) {
                return Err(ExportError::Translucent { frame, x, y, alpha });
            }
            first_row += band.len() / width;
            clear |= band.iter().any(|&c| c >> 24 == 0);
            found = found.and_then(|f| counted(f, &band, clear));
        }
    }
    match found {
        Some(found) => {
            let sorted = found.into_sorted();
            let opaque = in_palette_order(picture.doc(), &sorted).unwrap_or(sorted);
            Ok(GifPalette { opaque, clear })
        }
        None => Err(ExportError::GifColours { clear }),
    }
}

/// `found` with the opaque colours of `band` added, or None if they and a clear entry, if
/// `clear`, no longer fit one palette.
fn counted(mut found: Found, band: &[u32], clear: bool) -> Option<Found> {
    for &c in band.iter().filter(|&&c| c >> 24 != 0) {
        if !found.add(c) {
            return None;
        }
    }
    (found.len() + usize::from(clear) <= MAX_COLOURS).then_some(found)
}

// A project's longest frame, with the most rounding carried to it, fits a GIF delay, so no
// duration a project can hold is ever cut short.
const _: () = assert!((MAX_DURATION_MS as u64 + 5 + 5) / 10 <= u16::MAX as u64);

/// Each duration in milliseconds as a GIF delay in hundredths, and how many were raised to the
/// shortest. Each running total is rounded as `gif_delays_for` does, for a document timed in
/// milliseconds.
pub fn gif_delays(durations: impl IntoIterator<Item = u32>) -> (Vec<u16>, usize) {
    gif_delays_in(durations, 1000)
}

/// The delays of `order`, document frames, as `gif_delays` gives them for milliseconds: in ticks of the
/// document's frame rate when it has one, in milliseconds otherwise.
pub fn gif_delays_for(doc: &Document, order: &[usize]) -> (Vec<u16>, usize) {
    let units = order.iter().map(|&f| doc.frame_units(f));
    gif_delays_in(units, doc.units_per_second())
}

/// Each duration, `per_second` of them to a second, as a GIF delay in hundredths of a second,
/// and how many were raised to the shortest. Each running total is rounded to the nearest
/// hundredth, halves up, so the total stays within half a hundredth of the exact one however
/// long the loop. Raising a delay to the shortest is not carried: it lengthens that frame only.
/// Durations are those a project holds, at most `MAX_DURATION_MS`; a longer one could only be
/// given the longest delay.
fn gif_delays_in(durations: impl IntoIterator<Item = u32>, per_second: u32) -> (Vec<u16>, usize) {
    let mut raised = 0;
    let delays = cumulative(durations, per_second, 100)
        .into_iter()
        .map(|rounded| {
            let delay = u16::try_from(rounded).unwrap_or(u16::MAX);
            raised += usize::from(delay < SHORTEST_DELAY);
            delay.max(SHORTEST_DELAY)
        })
        .collect();
    (delays, raised)
}

/// A GIF playing `order`, document frames, `plays` times, 0 for forever, at `path`. Its
/// palette is found here, before anything is written. One frame is a still image: no loop and
/// no delay.
#[inline(never)]
pub fn gif_output<'a>(
    picture: &'a Picture<'a>,
    order: Vec<usize>,
    plays: u32,
    path: PathBuf,
) -> Result<Output<'a>, ExportError> {
    let frames = picture.doc().frames.len();
    if let Some(&frame) = order.iter().find(|&&f| f >= frames) {
        return Err(ExportError::NoSuchFrame { frame, frames });
    }
    if order.is_empty() {
        return Err(ExportError::NoFrames);
    }
    if plays > pikslar_format::gif_write::MAX_PLAYS {
        return Err(ExportError::TooManyPlays { plays });
    }
    let mut shown = order.clone();
    pikslar_core::sorting::sort_packed(&mut shown, |&f| f as u64, |key| key as usize);
    shown.dedup();
    let palette = gif_palette(picture, shown)?;
    let still = order.len() == 1;
    let delays = match still {
        true => vec![0],
        false => gif_delays_for(picture.doc(), &order).0,
    };
    let plays = if still { 1 } else { plays };
    let steps = gif_plan(picture, &order, plays != 1);
    Ok(Output {
        path,
        write: Box::new(move |out: &mut dyn Write| {
            write_gif(picture, (&order, &delays, &steps), (&palette, plays), out).map_err(io_error)
        }),
        ours_only: false,
    })
}

/// Writes the GIF: each of `order` shown for its delay, drawn as its step says.
fn write_gif(
    picture: &Picture<'_>,
    (order, delays, steps): (&[usize], &[u16], &[Step]),
    (palette, plays): (&GifPalette, u32),
    out: &mut dyn Write,
) -> Result<(), ExportError> {
    let size = (picture.width(), picture.height());
    let colours = (palette.opaque.as_slice(), palette.clear);
    let mut gif = GifStream::new(out, size, colours, plays)?;
    debug_assert!(delays.len() == order.len() && steps.len() == order.len());
    let mut held = (Vec::new(), Vec::new());
    for ((&frame, &delay), step) in order.iter().zip(delays).zip(steps) {
        let mut rows = gif.frame(delay, scaled(step.area, picture.scale()), step.after)?;
        send_area(picture, (frame, step.area), &mut held, &mut rows)?;
        rows.end()?;
    }
    gif.finish()?;
    Ok(())
}
