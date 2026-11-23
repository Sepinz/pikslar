//! Sequences with brush strokes replay to the same states, every time and against the record.
//!
//! To change the record on purpose, set `PIKSLAR_RECORD_REPLAY` and run this test, then say in
//! the commit why the states differ.

use pikslar_safety::replay_brush::{drawn_after_undoing_all, play_all, SEQUENCES};

const RECORD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../perf/replay-brush.txt");

#[test]
fn brush_sequences_replay_to_the_recorded_states() {
    let played = play_all().expect("plays");
    assert_eq!(played, play_all().expect("plays again"));
    if std::env::var_os("PIKSLAR_RECORD_REPLAY").is_some() {
        std::fs::write(RECORD, &played).expect("recorded");
        return;
    }
    let recorded = std::fs::read_to_string(RECORD).expect("record exists");
    let recorded = recorded.replace("\r\n", "\n");
    let first = played
        .lines()
        .zip(recorded.lines())
        .find(|(now, before)| now != before);
    assert_eq!(first, None, "first state that differs (now, recorded)");
    assert_eq!(played.lines().count(), recorded.lines().count());
}

#[test]
fn undoing_every_step_leaves_a_blank_canvas() {
    for seed in 0..SEQUENCES {
        assert_eq!(drawn_after_undoing_all(seed), Ok(0), "seed {seed}");
    }
}
