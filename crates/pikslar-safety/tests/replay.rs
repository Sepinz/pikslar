//! The fixed sequences must keep producing the recorded states.
//!
//! To change the record on purpose, set `PIKSLAR_RECORD_REPLAY` and run this test, then say in
//! the commit why the states differ.

use pikslar_safety::replay::play_all;

const RECORD: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../perf/replay-core.txt");

#[test]
fn edits_replay_to_the_recorded_states() {
    let played = play_all().expect("plays");
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
