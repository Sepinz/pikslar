//! The slices section: written only when there are slices, and a bad one never costs the art.

use crate::piks::{load_noted, load_recovery, save, save_recovery};
use crate::piks_slices::SLICES;
use crate::piks_v3::encode;
use pikslar_core::{ColorMode, Document, Slice, SliceRect, MAX_SLICES};

fn doc() -> Document {
    Document::new(16, 16, ColorMode::Rgba).expect("valid")
}

fn rect(x: i32, y: i32, width: u32, height: u32) -> SliceRect {
    SliceRect {
        x,
        y,
        width,
        height,
    }
}

fn slice(name: &str) -> Slice {
    Slice {
        name: name.to_owned(),
        color: 0x11_22_33_ff,
        bounds: rect(2, 3, 8, 6),
        pivot: None,
        center: None,
    }
}

fn has_section(bytes: &[u8]) -> bool {
    bytes.windows(4).any(|w| w == SLICES)
}

/// A record the way the writer frames it: length, then name, color, bounds, flags, extras.
fn record(name: &str, bounds: [i32; 4], flags: u8, extra: &[i32]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend((name.len() as u16).to_le_bytes());
    body.extend(name.as_bytes());
    body.extend(0xff00_00ffu32.to_le_bytes());
    bounds.iter().for_each(|v| body.extend(v.to_le_bytes()));
    body.push(flags);
    extra.iter().for_each(|v| body.extend(v.to_le_bytes()));
    let mut out = (body.len() as u16).to_le_bytes().to_vec();
    out.extend(body);
    out
}

fn section(count: u32, records: &[Vec<u8>]) -> Vec<u8> {
    let mut out = count.to_le_bytes().to_vec();
    records.iter().for_each(|r| out.extend(r));
    out
}

fn load_with(bytes: Vec<u8>) -> (Document, crate::piks_sections::LoadNotes) {
    load_noted(&encode(&doc(), &[(SLICES, bytes)])).expect("loads")
}

#[test]
fn slices_survive_save_and_load() {
    let mut doc = doc();
    let mut with_pivot = slice("feet");
    with_pivot.pivot = Some((-3, 4));
    let mut with_center = slice("panel");
    with_center.center = Some(rect(1, 2, 3, 2));
    let mut both = slice("ünï ☃");
    both.pivot = Some((0, 0));
    both.center = Some(rect(0, 0, 8, 6));
    both.bounds = rect(-5, -6, 8, 6);
    let all = [slice("plain"), with_pivot, with_center, both];
    all.iter().for_each(|s| {
        doc.add_slice(s.clone()).expect("fits");
    });
    let (back, notes) = load_noted(&save(&doc)).expect("loads");
    assert_eq!(back.slices, all);
    assert!(notes.can_save_over());
    assert!(back.foreign.is_empty());
}

#[test]
fn no_bytes_change_without_slices() {
    let bytes = save(&doc());
    assert!(!has_section(&bytes));
    let (back, notes) = load_noted(&bytes).expect("loads");
    assert!(back.slices.is_empty());
    assert!(notes.can_save_over());
}

#[test]
fn the_tag_may_not_be_copied_by_older_versions() {
    assert!(!SLICES[3].is_ascii_lowercase());
    let mut doc = doc();
    doc.add_slice(slice("a")).expect("fits");
    assert!(has_section(&save(&doc)));
}

#[test]
fn a_recovery_copy_keeps_slices() {
    let mut doc = doc();
    let mut s = slice("a");
    s.pivot = Some((1, 1));
    doc.add_slice(s).expect("fits");
    let back = load_recovery(&save_recovery(&doc, None)).expect("recovers");
    assert_eq!(back.doc.slices, doc.slices);
}

#[test]
fn a_slice_with_a_bad_rectangle_is_left_out_and_noted() {
    let good = record("ok", [0, 0, 4, 4], 0, &[]);
    for bad in [
        record("zero", [0, 0, 0, 4], 0, &[]),
        record("far", [100_000, 0, 4, 4], 0, &[]),
        record("centre", [0, 0, 4, 4], 2, &[0, 0, 5, 1]),
        record("pivot", [0, 0, 4, 4], 1, &[i32::MAX, 0]),
    ] {
        let (back, notes) = load_with(section(2, &[bad, good.clone()]));
        assert_eq!(back.slices.len(), 1);
        assert_eq!(back.slices[0].name, "ok");
        assert_eq!(notes.damaged, ["slices"]);
        assert!(!notes.can_save_over());
    }
}

#[test]
fn a_name_that_is_not_text_is_left_out() {
    let mut bad = record("xx", [0, 0, 4, 4], 0, &[]);
    bad[4] = 0xff;
    let (back, notes) = load_with(section(1, &[bad]));
    assert!(back.slices.is_empty());
    assert_eq!(notes.damaged, ["slices"]);
}

#[test]
fn cut_short_data_keeps_the_slices_before_the_cut() {
    let first = record("one", [0, 0, 4, 4], 0, &[]);
    let second = record("two", [1, 1, 4, 4], 0, &[]);
    let mut bytes = section(2, &[first, second]);
    bytes.truncate(bytes.len() - 3);
    let (back, notes) = load_with(bytes);
    assert_eq!(back.slices.len(), 1);
    assert_eq!(notes.damaged, ["slices"]);
}

#[test]
fn a_count_larger_than_the_data_or_no_count_is_noted() {
    let (back, notes) = load_with(section(5, &[record("a", [0, 0, 1, 1], 0, &[])]));
    assert_eq!(back.slices.len(), 1);
    assert_eq!(notes.damaged, ["slices"]);
    let (back, notes) = load_with(vec![1, 0]);
    assert!(back.slices.is_empty());
    assert_eq!(notes.damaged, ["slices"]);
    let (back, notes) = load_with(section(u32::MAX, &[]));
    assert!(back.slices.is_empty());
    assert_eq!(notes.damaged, ["slices"]);
}

#[test]
fn a_record_shorter_than_the_fields_is_left_out() {
    let mut short = vec![3, 0, 0, 0, 0];
    short.truncate(5);
    let (back, notes) = load_with(section(1, &[short]));
    assert!(back.slices.is_empty());
    assert_eq!(notes.damaged, ["slices"]);
}

#[test]
fn more_than_the_most_keeps_the_first_ones() {
    let records: Vec<Vec<u8>> = (0..MAX_SLICES + 3)
        .map(|i| record(&format!("s{i}"), [0, 0, 2, 2], 0, &[]))
        .collect();
    let (back, notes) = load_with(section(records.len() as u32, &records));
    assert_eq!(back.slices.len(), MAX_SLICES);
    assert_eq!(back.slices[0].name, "s0");
    assert_eq!(
        back.slices[MAX_SLICES - 1].name,
        format!("s{}", MAX_SLICES - 1)
    );
    assert_eq!(notes.damaged, ["slices"]);
}

#[test]
fn a_long_name_is_cut_and_trailing_bytes_are_skipped() {
    let name = "n".repeat(40);
    let mut r = record(&name, [0, 0, 4, 4], 1, &[1, 2]);
    // A later version added a field to the record.
    r.extend([9, 9, 9]);
    let len = u16::from_le_bytes([r[0], r[1]]) + 3;
    r[..2].copy_from_slice(&len.to_le_bytes());
    let after = record("next", [0, 0, 1, 1], 0, &[]);
    let (back, notes) = load_with(section(2, &[r, after]));
    assert_eq!(back.slices.len(), 2);
    assert_eq!(back.slices[0].name.len(), 32);
    assert_eq!(back.slices[0].pivot, Some((1, 2)));
    assert_eq!(back.slices[1].name, "next");
    assert!(notes.damaged.is_empty());
    assert!(notes.newer_fields);
}

#[test]
fn a_known_layout_is_what_is_written() {
    let mut doc = doc();
    let mut s = slice("ab");
    s.pivot = Some((5, -1));
    doc.add_slice(s).expect("fits");
    let bytes = save(&doc);
    let at = bytes.windows(4).position(|w| w == SLICES).expect("written");
    let want = record("ab", [2, 3, 8, 6], 1, &[5, -1]);
    let body = &bytes[at + 8..];
    assert_eq!(&body[..4], 1u32.to_le_bytes());
    assert_eq!(&body[4..6], &want[..2], "record length");
    assert_eq!(&body[6..8], 2u16.to_le_bytes());
    assert_eq!(&body[8..10], b"ab");
    assert_eq!(&body[10..14], 0x11_22_33_ffu32.to_le_bytes());
    assert_eq!(body[30], 1, "flags");
}

#[test]
fn the_most_foreign_sections_and_every_section_written_load_back() {
    use crate::piks::FLOATING;
    use crate::piks_anim::{RATE, TAGS};
    use crate::piks_sections::{MAX_FOREIGN, MAX_SECTIONS};
    use pikslar_core::ForeignSection;
    let mut doc = doc();
    doc.add_frame(1, false).expect("frame");
    doc.add_tag("walk", 0, 1).expect("tag");
    doc.frame_rate = Some(12);
    doc.add_slice(slice("a")).expect("fits");
    // Every section the document itself writes, then as many foreign ones as fit.
    let written = [TAGS, RATE, SLICES, FLOATING].len();
    for i in 0..MAX_FOREIGN {
        doc.foreign.push(ForeignSection {
            tag: [b'x', b'x', b'a' + (i / 26) as u8, b'a' + (i % 26) as u8],
            data: vec![i as u8].into(),
        });
    }
    let bytes = encode(&doc, &[(FLOATING, Vec::new())]);
    let back = load_recovery(&bytes).expect("loads");
    assert_eq!(back.doc.foreign.len(), MAX_FOREIGN);
    assert!(MAX_FOREIGN + written <= MAX_SECTIONS);
    assert_eq!(back.doc.slices, doc.slices);
    assert_eq!(back.doc.tags.len(), 1);
    assert_eq!(back.doc.frame_rate, Some(12));
    assert!(back.notes.not_kept.is_empty());
}

#[test]
fn foreign_sections_past_the_room_left_are_not_kept() {
    use crate::piks_sections::{MAX_FOREIGN, MAX_SECTIONS};
    let mut sections = Vec::new();
    for i in 0..MAX_SECTIONS - 1 {
        let tag = [b'x', b'x', b'a' + (i / 26) as u8, b'a' + (i % 26) as u8];
        sections.push((tag, vec![0u8]));
    }
    let refs: Vec<(&[u8; 4], Vec<u8>)> = sections.iter().map(|(t, d)| (t, d.clone())).collect();
    let (back, notes) = load_noted(&encode(&doc(), &refs)).expect("loads");
    assert_eq!(back.foreign.len(), MAX_FOREIGN);
    assert_eq!(notes.not_kept.len(), MAX_SECTIONS - 1 - MAX_FOREIGN);
}

#[test]
fn a_short_record_leaves_out_only_itself() {
    let short = vec![3, 0, 0, 0, 0];
    let good = record("after", [0, 0, 4, 4], 0, &[]);
    let (back, notes) = load_with(section(2, &[short, good]));
    assert_eq!(back.slices.len(), 1);
    assert_eq!(back.slices[0].name, "after");
    assert_eq!(notes.damaged, ["slices"]);
}

#[test]
fn records_past_the_count_are_noted() {
    let (a, b) = (
        record("a", [0, 0, 2, 2], 0, &[]),
        record("b", [0, 0, 2, 2], 0, &[]),
    );
    let (back, notes) = load_with(section(1, &[a, b]));
    assert_eq!(back.slices.len(), 1);
    assert_eq!(notes.damaged, ["slices"]);
}

#[test]
fn a_second_section_is_noted_and_the_first_is_kept() {
    let one = section(1, &[record("one", [0, 0, 2, 2], 0, &[])]);
    let two = section(1, &[record("two", [0, 0, 2, 2], 0, &[])]);
    let bytes = encode(&doc(), &[(SLICES, one), (SLICES, two)]);
    let (back, notes) = load_noted(&bytes).expect("loads");
    assert_eq!(back.slices.len(), 1);
    assert_eq!(back.slices[0].name, "one");
    assert_eq!(notes.damaged, ["slices"]);
}

#[test]
fn a_slice_too_big_for_its_length_fields_is_not_written_wrong() {
    let mut doc = doc();
    let mut s = slice("a");
    s.name = "n".repeat(70_000);
    doc.slices.push(s);
    doc.slices.push(slice("fine"));
    let (back, notes) = load_noted(&save(&doc)).expect("loads");
    assert_eq!(back.slices.last().map(|s| s.name.as_str()), Some("fine"));
    assert!(back.slices.len() <= 2);
    assert!(notes.damaged.is_empty() || notes.damaged == ["slices"]);
}
