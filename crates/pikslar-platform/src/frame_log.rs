//! Timing of input to present, for measurement runs only.
//!
//! With `PIKSLAR_FRAME_LOG` naming a file, the window notes when it takes the first input
//! message after the last frame, and, for each frame that then copies something to the
//! window, appends one line: from that input to the end of the blit, and its parts. Lines are
//! kept in a buffer reserved once and written when it fills and when the window closes, so the
//! file is not touched while frames are timed. Without the variable nothing is reserved, and
//! the loop does one test of an empty `Option` per message and per frame.

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

/// The environment variable that names the file.
pub const VAR: &str = "PIKSLAR_FRAME_LOG";
/// Frames kept before the buffer is written: more than a measurement run makes.
pub(crate) const CAPACITY: usize = 4096;
const HEADER: &str = "# total_us handled_us frame_us blit_us\n";

/// The file the variable's value names, if it names one.
pub fn path_from(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Key and pointer messages: the input a frame can follow.
pub(crate) fn is_input(msg: u32) -> bool {
    (0x0100..=0x0109).contains(&msg) || (0x0200..=0x020E).contains(&msg)
}

/// One presented frame, in microseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameTimes {
    /// From taking the input message to the return of the last blit and `GdiFlush`.
    pub total_us: u32,
    /// From taking the input to the end of the handler's events: the rest of the queue read,
    /// and the events handled.
    pub handled_us: u32,
    /// The handler's frame: the interface redraw and the canvas compositing.
    pub frame_us: u32,
    /// Copying the changed areas to the window.
    pub blit_us: u32,
}

fn micros(from: Instant, to: Instant) -> u32 {
    u32::try_from(to.saturating_duration_since(from).as_micros()).unwrap_or(u32::MAX)
}

impl FrameTimes {
    pub(crate) fn between(
        input: Instant,
        handled: Instant,
        framed: Instant,
        shown: Instant,
    ) -> FrameTimes {
        FrameTimes {
            total_us: micros(input, shown),
            handled_us: micros(input, handled),
            frame_us: micros(handled, framed),
            blit_us: micros(framed, shown),
        }
    }
}

pub(crate) struct FrameLog {
    path: PathBuf,
    lines: Vec<FrameTimes>,
    header_written: bool,
}

impl FrameLog {
    pub(crate) fn new(path: PathBuf) -> FrameLog {
        FrameLog {
            path,
            lines: Vec::with_capacity(CAPACITY),
            header_written: false,
        }
    }

    /// Keeps a frame, writing the buffer out once it is full.
    pub(crate) fn record(&mut self, times: FrameTimes) -> io::Result<()> {
        self.lines.push(times);
        if self.lines.len() >= CAPACITY {
            self.flush()?;
        }
        Ok(())
    }

    /// Appends the kept frames to the file. Writes nothing, and creates no file, without any.
    pub(crate) fn flush(&mut self) -> io::Result<()> {
        if self.lines.is_empty() {
            return Ok(());
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut out = BufWriter::new(file);
        self.write_to(&mut out)?;
        out.flush()
    }

    fn write_to(&mut self, out: &mut impl Write) -> io::Result<()> {
        if !self.header_written {
            out.write_all(HEADER.as_bytes())?;
            self.header_written = true;
        }
        for t in &self.lines {
            writeln!(
                out,
                "{} {} {} {}",
                t.total_us, t.handled_us, t.frame_us, t.blit_us
            )?;
        }
        self.lines.clear();
        Ok(())
    }
}

impl Drop for FrameLog {
    fn drop(&mut self) {
        // Nowhere is left to report a failure when the window closes; a measurement that
        // finds the file missing or short reports itself as not measured.
        let _ = self.flush();
    }
}

#[cfg(test)]
#[path = "frame_log_tests.rs"]
mod tests;
