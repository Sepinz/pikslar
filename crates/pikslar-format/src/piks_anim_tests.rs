//! Tag repeat counts: a block at the end of the tags section, written only when a tag has one,
//! which a version that does not know them reads past.

use crate::piks::{load, load_noted, save};
use crate::piks_anim::{apply_tags, tags_section, TAGS};
use crate::piks_sections::LoadNotes;
use pikslar_core::{ColorMode, Direction, Document, MAX_TAG_REPEAT};

/// Three frames, a tag over the first two and a ping-pong over the last.
fn tagged() -> Document {
    let mut doc = Document::new(8, 8, ColorMode::Rgba).expect("valid");
    doc.add_frame(1, false).expect("frame");
    doc.add_frame(2, false).expect("frame");
    doc.add_tag("walk", 0, 1).expect("tag");
    doc.add_tag("jump ü", 2, 2).expect("tag");
    doc.tags[1].direction = Direction::PingPong;
    doc
}

fn read(section: &[u8]) -> (Document, LoadNotes) {
    let mut doc = tagged();
    doc.tags.clear();
    let mut notes = LoadNotes::default();
    apply_tags(&mut doc, &[(TAGS.as_slice(), section)], &mut notes);
    (doc, notes)
}

#[test]
fn a_tag_repeat_survives_save_and_reload() {
    let mut doc = tagged();
    doc.tags[0].repeat = 3;
    doc.tags[1].repeat = MAX_TAG_REPEAT;
    let (back, notes) = load_noted(&save(&doc)).expect("loads");
    assert_eq!(back.tags, doc.tags);
    assert!(notes.can_save_over());
}

#[test]
fn tags_that_play_forever_are_written_as_before() {
    let doc = tagged();
    let section = tags_section(&doc).expect("tags");
    let records: usize = doc.tags.iter().map(|t| 1 + 4 + 4 + 2 + t.name.len()).sum();
    assert_eq!(section.len(), 4 + records, "no repeat block");
    assert_eq!(load(&save(&doc)).expect("loads").tags, doc.tags);
}

#[test]
fn a_version_that_knows_no_repeats_reads_the_same_tags() {
    let plain = tags_section(&tagged()).expect("tags");
    let mut doc = tagged();
    doc.tags[1].repeat = 2;
    let repeated = tags_section(&doc).expect("tags");
    assert_eq!(
        &repeated[..plain.len()],
        plain.as_slice(),
        "the records come first"
    );
    assert_eq!(
        &repeated[plain.len()..],
        [0, 0, 2, 0],
        "then one count for each tag"
    );
}

#[test]
fn bytes_after_the_repeats_are_noted_as_newer_and_the_tags_kept() {
    let mut doc = tagged();
    doc.tags[0].repeat = 5;
    let mut section = tags_section(&doc).expect("tags");
    section.extend_from_slice(&[7, 7, 7]);
    let (back, notes) = read(&section);
    assert_eq!(back.tags, doc.tags);
    assert!(notes.newer_fields && notes.damaged.is_empty());
}

#[test]
fn a_cut_short_repeat_block_costs_only_the_tags() {
    let mut doc = tagged();
    doc.tags[0].repeat = 5;
    let mut section = tags_section(&doc).expect("tags");
    section.pop();
    let (back, notes) = read(&section);
    assert!(back.tags.is_empty());
    assert_eq!(notes.damaged, ["tags"]);
}

#[test]
fn a_repeat_past_the_limit_is_kept_to_it() {
    let mut doc = tagged();
    doc.tags[0].repeat = 9;
    let mut section = tags_section(&doc).expect("tags");
    let at = section.len() - 4;
    section[at..at + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    let (back, _) = read(&section);
    assert_eq!(back.tags[0].repeat, MAX_TAG_REPEAT);
}
