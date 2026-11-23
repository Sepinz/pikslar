//! Timing by frame rate as well as by milliseconds.
//!
//! Each frame keeps its time in milliseconds, which every version reads. A document timed by
//! frame rate also says how many frames a second it runs at, and each frame's milliseconds are
//! then a whole number of ticks of that rate, rounded. The ticks are worked out from the
//! milliseconds, never stored, so frame edits need not know about them. Exact times come from
//! the ticks: 24 frames a second is 41.67 ms a tick, which no whole number of milliseconds is.

use crate::animation::{MAX_DURATION_MS, MIN_DURATION_MS};
use crate::document::{Document, LimitError};

/// The fastest frame rate. A tick of it is the shortest a frame may last.
pub const MAX_FRAME_RATE: u8 = 100;

const _: () = assert!(1000 / MAX_FRAME_RATE as u32 >= MIN_DURATION_MS);

/// The whole ticks of `fps` closest to `ms`, and at least one.
pub fn ticks(ms: u32, fps: u8) -> u32 {
    let fps = u64::from(fps.max(1));
    ((u64::from(ms) * fps + 500) / 1000).max(1) as u32
}

/// How long `ticks` of `fps` last, to the nearest millisecond a frame may last.
pub fn tick_ms(ticks: u32, fps: u8) -> u32 {
    let fps = u64::from(fps.max(1));
    let ms = (u64::from(ticks) * 1000 + fps / 2) / fps;
    ms.clamp(MIN_DURATION_MS.into(), MAX_DURATION_MS.into()) as u32
}

/// Times given in `units`, `per_second` of them to a second, as times in `target` a second.
/// Each running total is the exact running total rounded, halves up, so rounding never adds up
/// over a long loop.
pub fn cumulative(units: impl IntoIterator<Item = u32>, per_second: u32, target: u32) -> Vec<u32> {
    let (per, target) = (u128::from(per_second.max(1)), u128::from(target));
    let rounded = |total: u128| (2 * total * target + per) / (2 * per);
    let (mut total, mut shown) = (0u128, 0u128);
    units
        .into_iter()
        .map(|u| {
            total += u128::from(u);
            let now = rounded(total);
            let step = now - shown;
            shown = now;
            u32::try_from(step).unwrap_or(u32::MAX)
        })
        .collect()
}

impl Document {
    /// How long `frame` shows, in units of [`Document::units_per_second`]: ticks of the frame
    /// rate, or milliseconds.
    pub fn frame_units(&self, frame: usize) -> u32 {
        let ms = self.frames.get(frame).map_or(0, |f| f.duration_ms);
        match self.frame_rate {
            Some(fps) => ticks(ms, fps),
            None => ms,
        }
    }

    /// The frame rate, or 1000 for a document timed in milliseconds.
    pub fn units_per_second(&self) -> u32 {
        self.frame_rate.map_or(1000, u32::from)
    }

    /// `ms` as a frame of this document may last: whole ticks when it is timed by frame rate.
    pub fn snapped(&self, ms: u32) -> u32 {
        match self.frame_rate {
            Some(fps) => tick_ms(ticks(ms, fps), fps),
            None => ms,
        }
    }

    /// Times the document by `fps` frames a second, or in milliseconds for `None`. A new rate
    /// keeps each frame's ticks; going from milliseconds rounds each frame to whole ticks; going
    /// back to milliseconds keeps them as they are. A frame is never longer than
    /// [`MAX_DURATION_MS`], so ticks past it at a slower rate are lost; undo brings them back.
    pub(crate) fn set_frame_rate(&mut self, fps: Option<u8>) -> Result<(), LimitError> {
        if fps.is_some_and(|f| f == 0 || f > MAX_FRAME_RATE) {
            return Err(LimitError::FrameRate);
        }
        if let Some(new) = fps {
            let old = self.frame_rate.unwrap_or(new);
            for frame in &mut self.frames {
                frame.duration_ms = tick_ms(ticks(frame.duration_ms, old), new);
            }
        }
        self.frame_rate = fps;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::ColorMode;
    use crate::editor::Editor;
    use crate::{FrameCmd, StepCmd};

    #[test]
    fn milliseconds_come_back_unchanged_in_milliseconds() {
        let ms = [100, 40, 250, 75, 10, 60_000];
        assert_eq!(cumulative(ms, 1000, 1000), ms);
    }

    #[test]
    fn three_frames_at_thirty_a_second_are_gif_delays_three_four_three() {
        assert_eq!(cumulative([1, 1, 1], 30, 100), [3, 4, 3]);
    }

    #[test]
    fn every_running_total_is_the_exact_total_rounded() {
        let units: Vec<u32> = (0..500u32).map(|i| 1 + (i * 7) % 5).collect();
        for (per_second, target) in [(24, 100), (24, 1000), (30, 100), (7, 1000), (1000, 100)] {
            let out = cumulative(units.iter().copied(), per_second, target);
            let (mut exact, mut shown) = (0u64, 0u64);
            for (u, o) in units.iter().zip(&out) {
                exact += u64::from(*u);
                shown += u64::from(*o);
                let (t, p) = (u64::from(target), u64::from(per_second));
                assert_eq!(shown, (2 * exact * t + p) / (2 * p));
            }
        }
    }

    #[test]
    fn ten_thousand_frames_at_24_a_second_stay_within_a_hundredth_of_exact() {
        let gif: u32 = cumulative([1; 10_000], 24, 100).iter().sum();
        let json: u32 = cumulative([1; 10_000], 24, 1000).iter().sum();
        // Exactly 416.67 s.
        assert_eq!((gif, json), (41_667, 416_667));
        let per_frame_rounding = 10_000 * 42;
        assert!(
            per_frame_rounding - json > 3_000,
            "what rounding each frame alone does"
        );
    }

    #[test]
    fn a_half_tick_rounds_up_and_a_frame_is_never_shorter_than_one_tick() {
        assert_eq!((ticks(150, 10), ticks(149, 10)), (2, 1));
        assert_eq!((ticks(1, 24), ticks(0, 100)), (1, 1));
    }

    #[test]
    fn ticks_come_back_from_milliseconds_at_every_rate() {
        for fps in 1..=MAX_FRAME_RATE {
            let most = MAX_DURATION_MS * u32::from(fps) / 1000;
            for t in 1..=most {
                assert_eq!(ticks(tick_ms(t, fps), fps), t, "{t} ticks at {fps}");
            }
        }
    }

    fn timed(ms: &[u32]) -> Editor {
        let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
        for at in 1..ms.len() {
            doc.add_frame(at, false).expect("frame");
        }
        for (frame, &d) in doc.frames.iter_mut().zip(ms) {
            frame.duration_ms = d;
        }
        Editor::new(doc)
    }

    fn durations(e: &Editor) -> Vec<u32> {
        e.doc().frames.iter().map(|f| f.duration_ms).collect()
    }

    #[test]
    fn a_rate_rounds_frames_to_ticks_in_one_step_that_undo_takes_back() {
        let mut e = timed(&[100, 40, 250]);
        e.apply(FrameCmd::SetRate { fps: Some(24) }).expect("set");
        assert_eq!(e.doc().frame_rate, Some(24));
        assert_eq!(durations(&e), [83, 42, 250]);
        assert_eq!(e.doc().frame_units(0), 2);
        e.apply(StepCmd::Undo).expect("undo");
        assert_eq!(
            (e.doc().frame_rate, durations(&e)),
            (None, vec![100, 40, 250])
        );
        let too_fast = FrameCmd::SetRate {
            fps: Some(MAX_FRAME_RATE + 1),
        };
        assert_eq!(e.apply(too_fast), Err(LimitError::FrameRate));
        let still = FrameCmd::SetRate { fps: Some(0) };
        assert_eq!(e.apply(still), Err(LimitError::FrameRate));
    }

    #[test]
    fn another_rate_keeps_each_frames_ticks_and_milliseconds_keep_the_times() {
        let mut e = timed(&[100, 40, 250]);
        e.apply(FrameCmd::SetRate { fps: Some(24) }).expect("24");
        let again = e.apply(FrameCmd::SetRate { fps: Some(24) }).expect("same");
        assert!(!again.changed, "the same rate is no step");
        e.apply(FrameCmd::SetRate { fps: Some(12) }).expect("12");
        let units: Vec<u32> = (0..3).map(|f| e.doc().frame_units(f)).collect();
        assert_eq!(units, [2, 1, 6]);
        assert_eq!(durations(&e), [167, 83, 500]);
        e.apply(FrameCmd::SetRate { fps: None }).expect("ms");
        assert_eq!(durations(&e), [167, 83, 500]);
        assert_eq!(e.doc().frame_units(0), 167);
    }

    #[test]
    fn retiming_by_frame_rate_lands_on_whole_ticks() {
        let mut e = timed(&[100, 100]);
        e.apply(FrameCmd::SetRate { fps: Some(24) }).expect("set");
        let durations_ms = vec![125, 50];
        e.apply(FrameCmd::Retime {
            from: 0,
            durations: durations_ms,
        })
        .expect("retimed");
        assert_eq!(durations(&e), [125, 42]);
        assert_eq!(e.doc().frame_units(1), 1);
    }

    #[test]
    fn a_slower_rate_keeps_a_long_frame_to_the_longest_a_frame_may_last() {
        let mut e = timed(&[30_000, 100]);
        e.apply(FrameCmd::SetRate { fps: Some(60) }).expect("60");
        e.apply(FrameCmd::SetRate { fps: Some(12) }).expect("12");
        assert_eq!(durations(&e), [MAX_DURATION_MS, 500], "six ticks each time");
        e.apply(StepCmd::Undo).expect("undo");
        assert_eq!(e.doc().frame_units(0), 1_800);
    }

    #[test]
    fn a_rate_ends_a_timing_drag_so_each_is_undone_alone() {
        let mut e = timed(&[100, 100]);
        let retime = |ms: u32| FrameCmd::Retime {
            from: 0,
            durations: vec![ms],
        };
        e.apply(retime(150)).expect("dragged");
        e.apply(retime(200)).expect("dragged on");
        e.apply(FrameCmd::SetRate { fps: Some(24) }).expect("rate");
        e.apply(retime(250)).expect("a new drag");
        assert_eq!(durations(&e), [250, 83]);
        e.apply(StepCmd::Undo).expect("undo the new drag");
        assert_eq!(
            (e.doc().frame_rate, durations(&e)),
            (Some(24), vec![208, 83])
        );
        e.apply(StepCmd::Undo).expect("undo the rate");
        assert_eq!((e.doc().frame_rate, durations(&e)), (None, vec![200, 100]));
        e.apply(StepCmd::Undo).expect("undo the first drag, whole");
        assert_eq!(durations(&e), [100, 100]);
    }
}
