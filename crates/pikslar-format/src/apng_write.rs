//! Writing an animated PNG (APNG) a band of rows at a time: 8-bit RGBA, each frame's rows
//! compressed as a PNG's are, so no frame and no copy of the animation is ever held.
//!
//! The layout is the APNG specification's: the signature, `IHDR`, `acTL` with the number of
//! frames and plays, then for each frame an `fcTL` and its data, `IEND` last. The first frame's
//! data is the `IDAT` a viewer without APNG shows; every later frame's is in `fdAT` chunks.
//! `fcTL` and `fdAT` share one sequence count from 0, in the order they are written.

use crate::crc32;
pub use crate::gif_write::Area;
use crate::png_write::{chunk_to, header, invalid, ImageData, MOST_SIDE, SIGNATURE};
use std::io::{self, Write};

/// What becomes of a frame's area after it is shown (`dispose_op`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dispose {
    /// Left as the frame drew it.
    Keep,
    /// Made fully transparent black.
    Clear,
    /// Put back as it was before the frame was drawn.
    Previous,
}

/// How a frame's pixels meet what is under them (`blend_op`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// Replace them, alpha included.
    Source,
    /// Drawn over them.
    Over,
}

/// One frame's control: where it draws, for how long, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Control {
    pub area: Area,
    /// Shown for this long, in seconds as a fraction: the numerator and the denominator. Whole
    /// ticks of a frame rate are (ticks, rate); milliseconds are (ms, 1000).
    pub delay: (u16, u16),
    pub after: Dispose,
    pub blend: Blend,
}

/// An APNG being written. `frame` starts each frame in turn; `finish` ends the file once every
/// frame announced has been written.
pub struct ApngStream<W: Write> {
    out: W,
    size: (usize, usize),
    frames: u32,
    started: u32,
    ended: u32,
    sequence: u32,
}

/// Writes a chunk whose body starts with a sequence number.
fn numbered(out: &mut impl Write, kind: &[u8; 4], sequence: u32, body: &[u8]) -> io::Result<()> {
    let number = sequence.to_be_bytes();
    let len = u32::try_from(body.len() + 4).map_err(|_| invalid("a chunk too large"))?;
    out.write_all(&len.to_be_bytes())?;
    out.write_all(kind)?;
    out.write_all(&number)?;
    out.write_all(body)?;
    out.write_all(&crc32(&[kind, &number, body]).to_be_bytes())
}

/// A number a PNG holds: at most 2^31 - 1.
fn png_u32(n: usize) -> io::Result<u32> {
    match n <= MOST_SIDE {
        true => Ok(n as u32),
        false => Err(invalid("a number larger than a PNG holds")),
    }
}

impl<W: Write> ApngStream<W> {
    /// Writes the signature, the header of an 8-bit RGBA image of `size`, and `acTL`: `frames`
    /// frames, played `plays` times, 0 for forever.
    pub fn new(mut out: W, size: (usize, usize), frames: u32, plays: u32) -> io::Result<Self> {
        crate::png_write::check_size(size)?;
        if frames == 0 {
            return Err(invalid("an animation needs a frame"));
        }
        let (frames_n, plays_n) = (png_u32(frames as usize)?, png_u32(plays as usize)?);
        out.write_all(&SIGNATURE)?;
        chunk_to(&mut out, b"IHDR", &header(size, [8, 6]))?;
        let mut actl = frames_n.to_be_bytes().to_vec();
        actl.extend_from_slice(&plays_n.to_be_bytes());
        chunk_to(&mut out, b"acTL", &actl)?;
        Ok(ApngStream {
            out,
            size,
            frames,
            started: 0,
            ended: 0,
            sequence: 0,
        })
    }

    fn next_sequence(&mut self) -> io::Result<u32> {
        let n = png_u32(self.sequence as usize)?;
        self.sequence += 1;
        Ok(n)
    }

    /// The fcTL body for `control`, once its area is checked.
    fn fctl(&self, control: &Control) -> io::Result<Vec<u8>> {
        let Area {
            x,
            y,
            width,
            height,
        } = control.area;
        let inside =
            |at: usize, len: usize, side: usize| at.checked_add(len).is_some_and(|e| e <= side);
        if width == 0
            || height == 0
            || !inside(x, width, self.size.0)
            || !inside(y, height, self.size.1)
        {
            return Err(invalid("a frame outside the image, or of no pixels"));
        }
        if self.started == 0 && (x, y, width, height) != (0, 0, self.size.0, self.size.1) {
            return Err(invalid("the first frame is the whole image"));
        }
        let mut body = Vec::with_capacity(22);
        for n in [width, height, x, y] {
            body.extend_from_slice(&(n as u32).to_be_bytes());
        }
        body.extend_from_slice(&control.delay.0.to_be_bytes());
        body.extend_from_slice(&control.delay.1.to_be_bytes());
        body.push(match control.after {
            Dispose::Keep => 0,
            Dispose::Clear => 1,
            Dispose::Previous => 2,
        });
        body.push(match control.blend {
            Blend::Source => 0,
            Blend::Over => 1,
        });
        Ok(body)
    }

    /// Writes the next frame's control and starts its rows, which go in with `rows`, top to
    /// bottom, and end with `end`.
    pub fn frame(&mut self, control: Control) -> io::Result<ApngFrame<'_, W>> {
        if self.started != self.ended || self.started >= self.frames {
            return Err(invalid(
                "a frame more than announced, or one left unfinished",
            ));
        }
        let body = self.fctl(&control)?;
        let sequence = self.next_sequence()?;
        numbered(&mut self.out, b"fcTL", sequence, &body)?;
        self.started += 1;
        let size = (control.area.width, control.area.height);
        Ok(ApngFrame {
            data: ImageData::new(size, None),
            stream: self,
        })
    }

    /// Ends the file once every frame announced is written, and gives back the writer.
    pub fn finish(mut self) -> io::Result<W> {
        if self.ended != self.frames || self.started != self.ended {
            return Err(invalid("the animation ended before its last frame"));
        }
        chunk_to(&mut self.out, b"IEND", &[])?;
        self.out.flush()?;
        Ok(self.out)
    }
}

/// One frame's rows being written.
pub struct ApngFrame<'s, W: Write> {
    stream: &'s mut ApngStream<W>,
    data: ImageData,
}

/// Sends one piece of a frame's data: `IDAT` for the first frame, else a numbered `fdAT`.
fn put<W: Write>(stream: &mut ApngStream<W>, part: &[u8]) -> io::Result<()> {
    match stream.ended {
        0 => chunk_to(&mut stream.out, b"IDAT", part),
        _ => {
            let sequence = stream.next_sequence()?;
            numbered(&mut stream.out, b"fdAT", sequence, part)
        }
    }
}

impl<W: Write> ApngFrame<'_, W> {
    /// Adds whole rows of straight RGBA cells, as wide as the frame's area.
    pub fn rows(&mut self, px: &[u32]) -> io::Result<()> {
        let stream = &mut *self.stream;
        self.data.rows(px, &mut |part| put(stream, part))
    }

    /// Ends the frame once every row of its area is in.
    pub fn end(self) -> io::Result<()> {
        let stream = self.stream;
        self.data.finish(&mut |part| put(stream, part))?;
        stream.ended += 1;
        Ok(())
    }
}
