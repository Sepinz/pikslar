//! A document timed by frame rate: GIF delays, APNG delays and JSON durations come from its
//! ticks, with rounding that never adds up over a long loop.

use crate::apng_read::read as read_apng;
use crate::gif_read::read as read_gif;
use crate::gif_tests::written;
use crate::{
    apng_output, gif_delays_for, gif_output, sequence_plan, sheet_files, Picture, SHORTEST_DELAY,
};
use pikslar_core::{tick_ms, ColorMode, Document};
use std::path::Path;

/// `ticks.len()` frames timed at `fps`, each lasting its ticks.
fn timed(fps: u8, ticks: &[u32]) -> Document {
    let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
    for at in 1..ticks.len() {
        doc.add_frame(at, false).expect("frame");
    }
    for (frame, &t) in doc.frames.iter_mut().zip(ticks) {
        frame.duration_ms = tick_ms(t, fps);
    }
    doc.frame_rate = Some(fps);
    doc
}

/// Every `"duration":N` in a description.
fn durations(text: &str) -> Vec<u32> {
    text.split("\"duration\":")
        .skip(1)
        .map(|rest| {
            let end = rest.find(|c: char| !c.is_ascii_digit()).expect("ends");
            rest[..end].parse().expect("number")
        })
        .collect()
}

#[test]
fn three_frames_at_thirty_fps_become_gif_delays_three_four_three() {
    let doc = timed(30, &[1, 1, 1]);
    assert_eq!(gif_delays_for(&doc, &[0, 1, 2]), (vec![3, 4, 3], 0));
    let picture = Picture::visible(&doc);
    let output = gif_output(&picture, vec![0, 1, 2], 0, "a.gif".into()).expect("gif");
    let delays: Vec<u16> = read_gif(&written(output))
        .frames
        .iter()
        .map(|f| f.delay)
        .collect();
    assert_eq!(delays, [3, 4, 3]);
}

#[test]
fn a_gif_delay_under_the_shortest_is_raised_outside_the_carry() {
    // 100 a second is a hundredth a tick: each is raised to 2 and the rest is not carried.
    let doc = timed(100, &[1, 1, 1]);
    assert_eq!(
        gif_delays_for(&doc, &[0, 1, 2]),
        (vec![SHORTEST_DELAY; 3], 3)
    );
}

#[test]
fn a_document_in_milliseconds_has_the_delays_it_always_had() {
    let mut doc = timed(30, &[1, 1, 1]);
    doc.frame_rate = None;
    doc.frames[0].duration_ms = 33;
    doc.frames[1].duration_ms = 33;
    doc.frames[2].duration_ms = 34;
    let durations = [33, 33, 34];
    assert_eq!(
        gif_delays_for(&doc, &[0, 1, 2]),
        crate::gif_delays(durations)
    );
}

#[test]
fn apng_delays_are_exact_tick_fractions_at_the_frame_rate() {
    let doc = timed(24, &[1, 3, 2]);
    let picture = Picture::visible(&doc);
    let output = apng_output(&picture, vec![0, 1, 2], 0, "a.png".into()).expect("apng");
    let delays: Vec<(u16, u16)> = read_apng(&written(output))
        .frames
        .iter()
        .map(|f| f.delay)
        .collect();
    assert_eq!(delays, [(1, 24), (3, 24), (2, 24)]);
}

#[test]
fn an_apng_in_milliseconds_keeps_them_over_a_thousand() {
    let mut doc = timed(24, &[1, 1]);
    doc.frame_rate = None;
    doc.frames[0].duration_ms = 40;
    doc.frames[1].duration_ms = 250;
    let picture = Picture::visible(&doc);
    let output = apng_output(&picture, vec![0, 1], 0, "a.png".into()).expect("apng");
    let delays: Vec<(u16, u16)> = read_apng(&written(output))
        .frames
        .iter()
        .map(|f| f.delay)
        .collect();
    assert_eq!(delays, [(40, 1000), (250, 1000)]);
}

#[test]
fn a_sheet_at_24_fps_totals_within_a_millisecond_of_exact() {
    let frames = 240;
    let doc = timed(24, &vec![1; frames]);
    let text = sheet_files(&doc, 0..=frames - 1, ("a.png", "a"))
        .expect("made")
        .description;
    let shown = durations(&text);
    assert_eq!(shown.len(), frames);
    let total: u32 = shown.iter().sum();
    assert!(total.abs_diff(10_000) <= 1, "{total} ms");
    assert!(shown.iter().all(|&d| d == 41 || d == 42));
}

#[test]
fn a_sequence_at_24_fps_totals_within_a_millisecond_of_exact() {
    let frames = 48;
    let doc = timed(24, &vec![1; frames]);
    let picture = Picture::visible(&doc);
    let plan = sequence_plan(&picture, 0..=frames - 1, Path::new("a.png")).expect("planned");
    let total: u32 = durations(&plan.text).iter().sum();
    assert!(total.abs_diff(2_000) <= 1, "{total} ms");
}

fn apng_delays_of(doc: &Document) -> Vec<(u16, u16)> {
    let picture = Picture::visible(doc);
    let order = (0..doc.frames.len()).collect();
    let output = apng_output(&picture, order, 0, "a.png".into()).expect("apng");
    read_apng(&written(output))
        .frames
        .iter()
        .map(|f| f.delay)
        .collect()
}

#[test]
fn apng_delays_at_the_extremes_of_rate_and_length() {
    // The longest frame at the fastest rate: 6000 ticks of 100 a second.
    let fast = timed(100, &[6000, 1]);
    assert_eq!(fast.frames[0].duration_ms, 60_000);
    assert_eq!(apng_delays_of(&fast), [(6000, 100), (1, 100)]);
    // One a second: sixty ticks fill the longest a frame may last.
    let slow = timed(1, &[60, 1]);
    assert_eq!(apng_delays_of(&slow), [(60, 1), (1, 1)]);
}

#[test]
fn the_gif_note_says_the_raised_frames_play_longer_than_asked() {
    use crate::{plan, Job, Kind};
    let doc = timed(100, &[1, 1, 1]);
    let picture = Picture::visible(&doc);
    let planned = plan(&Job::new(Kind::Gif), &picture, "a.gif".into()).expect("planned");
    assert_eq!(
        planned.note.as_deref(),
        Some(
            "3 frame(s) would round to less than 20 ms, which most players show as 100 ms, so \
             in the GIF they play for 20 ms"
        )
    );
    // At 50 a second a tick is the shortest delay itself: nothing is raised, nothing said.
    let fine = timed(50, &[1, 1, 1]);
    let picture = Picture::visible(&fine);
    let planned = plan(&Job::new(Kind::Gif), &picture, "a.gif".into()).expect("planned");
    assert_eq!(planned.note, None);
}
