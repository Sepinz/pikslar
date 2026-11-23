//! Animated PNGs: the frames chosen, in the order they play, each exactly the RGBA the PNG
//! export of that frame has, partial alpha and colour behind clear pixels included.
//!
//! Each frame draws the rectangle of cells that differ from the frame shown before it
//! (`apng_plan`), the first frame the whole image. It replaces the pixels it covers
//! (`blend_op` SOURCE) and leaves them for the next (`dispose_op` NONE), so a clear pixel never
//! shows a frame before it. Delays are exact: milliseconds over 1000, or ticks over the frame rate. Planning draws
//! each frame once at the document's size, a band at a time; writing draws again only the
//! bands a frame's rectangle crosses. No frame is held whole.

use crate::apng_plan::apng_areas;
use crate::outputs::Output;
use crate::stream::{scaled, send_area};
use crate::{io_error, ExportError, Picture};
use pikslar_core::document::MAX_FRAMES;
use pikslar_core::MAX_DURATION_MS;
use pikslar_format::apng_write::{ApngStream, Area, Blend, Control, Dispose};
use std::io::Write;
use std::path::PathBuf;

// A project's longest frame fits an APNG delay, in milliseconds or in ticks of a frame rate.
const _: () = assert!(MAX_DURATION_MS as u64 <= u16::MAX as u64);
// A play order, ping-pong included, is under twice a project's frames: within an APNG's count.
const _: () = assert!(2 * MAX_FRAMES <= i32::MAX as usize);

/// The most plays `--loop` asks of an animation, as for a GIF.
pub const MOST_PLAYS: u32 = pikslar_format::gif_write::MAX_PLAYS;

/// An APNG playing `order`, document frames, `plays` times, 0 for forever, at `path`. One
/// frame plays once.
#[inline(never)]
pub fn apng_output<'a>(
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
    if plays > MOST_PLAYS {
        return Err(ExportError::TooManyPlays { plays });
    }
    let plays = if order.len() == 1 { 1 } else { plays };
    Ok(Output {
        path,
        // Planned only once the destination is known to be usable, so a refusal is quick.
        write: Box::new(move |out: &mut dyn Write| {
            let areas = apng_areas(picture, &order);
            write_apng(picture, (&order, &areas), plays, out).map_err(io_error)
        }),
        ours_only: false,
    })
}

/// Writes the APNG: each of `order` drawing its area, shown for its duration.
fn write_apng(
    picture: &Picture<'_>,
    (order, areas): (&[usize], &[Area]),
    plays: u32,
    out: &mut dyn Write,
) -> Result<(), ExportError> {
    let size = (picture.width(), picture.height());
    // Too many frames for a PNG is refused by the stream, with its own message.
    let count = u32::try_from(order.len()).unwrap_or(u32::MAX);
    let mut apng = ApngStream::new(out, size, count, plays)?;
    let doc = picture.doc();
    debug_assert_eq!(order.len(), areas.len());
    let mut held = (Vec::new(), Vec::new());
    for (&frame, &area) in order.iter().zip(areas) {
        let delay = (
            u16::try_from(doc.frame_units(frame)).unwrap_or(u16::MAX),
            u16::try_from(doc.units_per_second()).unwrap_or(u16::MAX),
        );
        let control = Control {
            area: scaled(area, picture.scale()),
            delay,
            after: Dispose::Keep,
            blend: Blend::Source,
        };
        let mut rows = apng.frame(control)?;
        send_area(picture, (frame, area), &mut held, &mut rows)?;
        rows.end()?;
    }
    apng.finish()?;
    Ok(())
}
