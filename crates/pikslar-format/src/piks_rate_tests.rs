//! The frame rate section: one byte, written only when the document is timed by rate.

use crate::piks::{load_noted, save};
use crate::piks_anim::RATE;
use crate::piks_v3::encode;
use pikslar_core::{ColorMode, Document};

fn doc() -> Document {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    doc.add_frame(1, false).expect("frame");
    doc
}

fn has_rate(bytes: &[u8]) -> bool {
    bytes.windows(4).any(|w| w == RATE)
}

#[test]
fn a_rate_survives_save_and_load() {
    for fps in [1, 24, 30, 100] {
        let mut doc = doc();
        doc.frame_rate = Some(fps);
        for frame in &mut doc.frames {
            frame.duration_ms =
                pikslar_core::tick_ms(pikslar_core::ticks(frame.duration_ms, fps), fps);
        }
        let (back, notes) = load_noted(&save(&doc)).expect("loads");
        assert_eq!(back.frame_rate, Some(fps));
        assert!(notes.can_save_over());
        assert!(
            back.foreign.is_empty(),
            "the rate is not carried as foreign"
        );
    }
}

#[test]
fn no_section_is_written_without_a_rate() {
    let doc = doc();
    let bytes = save(&doc);
    assert!(!has_rate(&bytes));
    let (back, notes) = load_noted(&bytes).expect("loads");
    assert_eq!(back.frame_rate, None);
    assert!(notes.can_save_over());
}

#[test]
fn the_rate_tag_may_be_copied_by_older_versions() {
    assert!(RATE[3].is_ascii_lowercase());
    let mut doc = doc();
    doc.frame_rate = Some(12);
    let bytes = save(&doc);
    let at = bytes.windows(4).position(|w| w == RATE).expect("written");
    assert_eq!(&bytes[at + 4..at + 9], [1, 0, 0, 0, 12], "length then fps");
}

#[test]
fn a_bad_rate_loads_as_milliseconds_and_is_noted() {
    for bad in [vec![0], vec![101], vec![255], vec![], vec![24, 0]] {
        let bytes = encode(&doc(), &[(RATE, bad)]);
        let (back, notes) = load_noted(&bytes).expect("loads");
        assert_eq!(back.frame_rate, None);
        assert_eq!(notes.damaged, ["frame rate"]);
        assert!(!notes.can_save_over());
        assert!(back.foreign.is_empty());
    }
}

#[test]
fn times_off_the_ticks_are_snapped_when_loaded_and_noted() {
    // An older version timed by rate in steps of 10 ms: 90 ms at 24 a second is two ticks.
    let mut doc = doc();
    doc.frames[0].duration_ms = 90;
    doc.frames[1].duration_ms = 83;
    let (back, notes) = load_noted(&encode(&doc, &[(RATE, vec![24])])).expect("loads");
    assert_eq!(back.frame_rate, Some(24));
    let times: Vec<u32> = back.frames.iter().map(|f| f.duration_ms).collect();
    assert_eq!(times, [83, 83]);
    assert!(notes.damaged.is_empty(), "rounding is not damage");
    assert_eq!(notes.rounded_to_fps, Some(24));
    assert!(!notes.can_save_over(), "told, so it is not saved over");
    let said = notes.message().expect("told");
    assert!(
        said.starts_with("Frame times were rounded to whole ticks of 24 fps. "),
        "{said}"
    );
    assert!(
        !said.contains("damaged") && !said.contains("left out"),
        "{said}"
    );
    assert_eq!(
        notes.lines(),
        ["frame times were rounded to whole ticks of 24 fps"]
    );
}

#[test]
fn times_already_on_the_ticks_are_left_alone_without_a_note() {
    let mut doc = doc();
    doc.frames[0].duration_ms = 42;
    doc.frames[1].duration_ms = 125;
    let (back, notes) = load_noted(&encode(&doc, &[(RATE, vec![24])])).expect("loads");
    let times: Vec<u32> = back.frames.iter().map(|f| f.duration_ms).collect();
    assert_eq!(times, [42, 125]);
    assert!(notes.damaged.is_empty() && notes.can_save_over());
    assert_eq!(notes.rounded_to_fps, None);
    assert_eq!(notes.message(), None);
}
