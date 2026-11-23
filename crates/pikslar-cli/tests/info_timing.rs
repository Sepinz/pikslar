//! `info` and timing by frame rate: the total is the exact one every export shows.

mod common;
use common::{cli, folder};
use pikslar_core::{ColorMode, Document};
use pikslar_format::piks;
use std::fs;

#[test]
fn two_hundred_forty_one_tick_frames_at_24_fps_total_ten_seconds() {
    let d = folder("info-timing-total");
    let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("size");
    for at in 1..240 {
        doc.add_frame(at, false).expect("frame");
    }
    doc.frame_rate = Some(24);
    for frame in &mut doc.frames {
        frame.duration_ms = 42;
    }
    let project = d.join("t.piks");
    fs::write(&project, piks::save(&doc)).expect("write");
    let out = cli(&[&"info", &project]).out;
    assert!(out.contains("240 frames, 10.000 s, 24 fps"), "{out}");
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn a_document_in_milliseconds_still_adds_the_milliseconds() {
    let d = folder("info-timing-ms");
    let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("size");
    doc.add_frame(1, false).expect("frame");
    let project = d.join("t.piks");
    fs::write(&project, piks::save(&doc)).expect("write");
    let out = cli(&[&"info", &project]).out;
    assert!(out.contains("2 frames, 0.200 s\n"), "{out}");
    fs::remove_dir_all(d).expect("cleanup");
}

#[test]
fn rounded_frame_times_are_told_as_rounding_not_damage() {
    let d = folder("info-timing-rounded");
    let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("size");
    doc.add_frame(1, false).expect("frame");
    doc.frame_rate = Some(24);
    doc.frames[0].duration_ms = 90;
    doc.frames[1].duration_ms = 42;
    let project = d.join("t.piks");
    fs::write(&project, piks::save(&doc)).expect("write");
    let run = cli(&[&"info", &project]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        run.err,
        "pikslar-cli: warning: frame times were rounded to whole ticks of 24 fps\n"
    );
    assert!(
        run.out.contains("2 frames, 0.125 s, 24 fps"),
        "83 + 42 ms: {}",
        run.out
    );
    let clean = d.join("clean.piks");
    doc.frames[0].duration_ms = 83;
    fs::write(&clean, piks::save(&doc)).expect("write");
    assert_eq!(cli(&[&"info", &clean]).err, "", "nothing to tell");
    fs::remove_dir_all(d).expect("cleanup");
}
