//! PNGs written straight from bands of flattened rows into a file, with no whole image held.
//!
//! A frame is a sheet with one place. At any scale, what is held is one band of document rows
//! per frame and one band of the image's width: never the enlarged image. Each enlarged row is
//! made once and sent `scale` times.

use crate::palette::Palette;
use crate::picture::{widen, Picture};
use crate::sheet_layout::Sheet;
use crate::ExportError;
use pikslar_format::apng_write::ApngFrame;
use pikslar_format::gif_write::{Area, GifFrame};
use pikslar_format::png_write::PngStream;
use pikslar_render::BAND_ROWS;
use std::io::{self, Write};
use std::ops::Range;

/// One row of frames: the document frame at its first place, and the places.
type FrameRow<'p> = (usize, &'p [(usize, usize)]);

/// Where an image's rows go, whole rows at a time, in order.
pub(crate) trait RowSink {
    fn rows(&mut self, cells: &[u32]) -> io::Result<()>;
}

impl<W: Write> RowSink for PngStream<W> {
    fn rows(&mut self, cells: &[u32]) -> io::Result<()> {
        PngStream::rows(self, cells)
    }
}

impl<W: Write> RowSink for GifFrame<'_, W> {
    fn rows(&mut self, cells: &[u32]) -> io::Result<()> {
        GifFrame::rows(self, cells)
    }
}

impl<W: Write> RowSink for ApngFrame<'_, W> {
    fn rows(&mut self, cells: &[u32]) -> io::Result<()> {
        ApngFrame::rows(self, cells)
    }
}

/// The rows held while an image is sent: one band of a frame, and one of the image's width.
pub(crate) type Held = (Vec<u32>, Vec<u32>);

/// Writes one frame as a PNG into `out`, a band of rows at a time: indexed with `palette` if
/// one is given, which must hold every colour of the frame, else 8-bit RGBA.
#[inline(never)]
pub fn write_frame_png(
    picture: &Picture<'_>,
    frame: usize,
    palette: Option<&Palette>,
    out: &mut dyn Write,
) -> Result<(), ExportError> {
    let frames = picture.doc().frames.len();
    if frame >= frames {
        return Err(ExportError::NoSuchFrame { frame, frames });
    }
    let sheet = Sheet {
        width: picture.width(),
        height: picture.height(),
        places: vec![(0, 0)],
    };
    write_sheet_png(picture, (&sheet, frame), palette, out)
}

/// Writes a sheet as a PNG into `out`. `first` is the document frame at the sheet's first
/// place, and each place is a frame's top left corner at the picture's scale. Indexed with
/// `palette` if one is given, which must hold every colour of the sheet, else 8-bit RGBA.
#[inline(never)]
pub fn write_sheet_png(
    picture: &Picture<'_>,
    (sheet, first): (&Sheet, usize),
    palette: Option<&Palette>,
    out: &mut dyn Write,
) -> Result<(), ExportError> {
    let mut png = match palette {
        None => PngStream::new(out, sheet.width, sheet.height)?,
        Some(p) => PngStream::indexed(out, sheet.width, sheet.height, p.colours())?,
    };
    let mut held = (Vec::new(), Vec::new());
    // The first row not yet sent. Rows between rows of frames, if padded, are clear.
    let mut next = 0;
    for row in rows_of(sheet) {
        let places = &sheet.places[row.clone()];
        clear_rows(
            places[0].1.saturating_sub(next),
            sheet.width,
            &mut held.1,
            &mut png,
        )?;
        for index in 0..picture.bands() {
            let frames = (first + row.start, places);
            write_band(picture, (frames, sheet.width), index, &mut held, &mut png)?;
        }
        next = places[0].1 + picture.height();
    }
    png.finish()?;
    Ok(())
}

/// Sends `count` clear rows of `width`, `line` holding one.
fn clear_rows(
    count: usize,
    width: usize,
    line: &mut Vec<u32>,
    png: &mut impl RowSink,
) -> io::Result<()> {
    if count > 0 {
        line.clear();
        line.resize(width, 0);
    }
    for _ in 0..count {
        png.rows(line)?;
    }
    Ok(())
}

/// The places of each row of frames: those that share a top edge, in order.
fn rows_of(sheet: &Sheet) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut start = 0;
    std::iter::from_fn(move || {
        let y = sheet.places.get(start)?.1;
        let count = sheet.places[start..]
            .iter()
            .take_while(|p| p.1 == y)
            .count();
        start += count;
        Some(start - count..start)
    })
}

/// Band `index` of one row of frames, the first of them document frame `frame`, enlarged into
/// rows of `width` and sent. `held` is the band of one frame and the band of the image.
fn write_band(
    picture: &Picture<'_>,
    ((frame, places), width): (FrameRow<'_>, usize),
    index: usize,
    (band, line): &mut Held,
    png: &mut impl RowSink,
) -> Result<(), ExportError> {
    let (w, scale) = (picture.doc().width, picture.scale());
    // An image as wide as one frame unscaled holds one frame a row at scale 1: send its band.
    if width == w {
        picture.band(frame, index, band);
        return Ok(png.rows(band)?);
    }
    line.clear();
    for (at, &(x, _)) in places.iter().enumerate() {
        picture.band(frame + at, index, band);
        let rows = band.len() / w.max(1);
        line.resize(width * rows, 0);
        for (r, cells) in band.chunks_exact(w).enumerate() {
            widen(cells, scale, &mut line[r * width + x..][..w * scale]);
        }
    }
    if scale == 1 {
        return Ok(png.rows(line)?);
    }
    for row in line.chunks_exact(width) {
        for _ in 0..scale {
            png.rows(row)?;
        }
    }
    Ok(())
}

/// The rows of `area` of `frame`, document pixels, enlarged, into `rows`. Only the bands it
/// crosses are drawn; `held` is one band of the frame and one enlarged row.
pub(crate) fn send_area(
    picture: &Picture<'_>,
    (frame, area): (usize, Area),
    (band, line): &mut Held,
    rows: &mut impl RowSink,
) -> Result<(), ExportError> {
    let (width, scale) = (picture.doc().width, picture.scale());
    let (top, bottom) = (area.y, area.y + area.height);
    line.resize(area.width * scale, 0);
    for index in top / BAND_ROWS..bottom.div_ceil(BAND_ROWS) {
        picture.band(frame, index, band);
        let first = index * BAND_ROWS;
        for y in top.max(first)..bottom.min(first + band.len() / width.max(1)) {
            let row = &band[(y - first) * width + area.x..][..area.width];
            widen(row, scale, line);
            for _ in 0..scale {
                rows.rows(line)?;
            }
        }
    }
    Ok(())
}

/// `area` of the document as the export's pixels.
pub(crate) fn scaled(area: Area, scale: usize) -> Area {
    Area {
        x: area.x * scale,
        y: area.y * scale,
        width: area.width * scale,
        height: area.height * scale,
    }
}
