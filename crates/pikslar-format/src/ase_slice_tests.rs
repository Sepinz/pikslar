//! Slices of an Aseprite file, brought into the document's own slices.

use crate::ase_build::*;
use crate::ase_tests::open;
use crate::{AseWarning, Tally};
use pikslar_core::{SliceRect, MAX_SLICES, MAX_SLICE_NAME};

/// The warning of this kind, if there is one: how many, the first, and its frame.
fn counted(warnings: &[AseWarning], kind: Tally) -> Option<(usize, String, usize)> {
    warnings.iter().find_map(|w| match w {
        AseWarning::Counted {
            kind: k,
            count,
            first,
            frame,
        } if *k == kind => Some((*count, first.clone(), *frame)),
        _ => None,
    })
}

fn key(frame: u32, bounds: (i32, i32, u32, u32)) -> SliceKey {
    SliceKey {
        frame,
        bounds,
        center: None,
        pivot: None,
    }
}

fn file(chunks: Vec<Vec<u8>>) -> Vec<u8> {
    let mut all = vec![plain("L")];
    all.extend(chunks);
    Ase::new(32, 32, 32)
        .frame(100, all)
        .frame(100, vec![])
        .bytes()
}

#[test]
fn a_slice_keeps_its_name_bounds_centre_and_pivot() {
    let k = SliceKey {
        center: Some((2, 3, 4, 5)),
        pivot: Some((1, -1)),
        ..key(0, (-4, 6, 10, 12))
    };
    let (doc, warnings) = open(&file(vec![slice("button", &[k])]));
    assert_eq!(doc.slices.len(), 1);
    let s = &doc.slices[0];
    assert_eq!(s.name, "button");
    let rect = |x, y, width, height| SliceRect {
        x,
        y,
        width,
        height,
    };
    assert_eq!(s.bounds, rect(-4, 6, 10, 12));
    assert_eq!(s.center, Some(rect(2, 3, 4, 5)));
    assert_eq!(s.pivot, Some((1, -1)));
    assert_eq!(counted(&warnings, Tally::SliceKeys), None);
    assert!(warnings.contains(&AseWarning::Skipped("slice colors and user data")));
}

#[test]
fn a_slice_that_moves_keeps_the_key_at_frame_zero_and_says_so() {
    let keys = [
        key(1, (9, 9, 2, 2)),
        key(0, (1, 2, 3, 4)),
        key(2, (5, 5, 5, 5)),
    ];
    let (doc, warnings) = open(&file(vec![slice("walk", &keys)]));
    assert_eq!(doc.slices[0].bounds.x, 1);
    assert_eq!(
        counted(&warnings, Tally::SliceKeys),
        Some((1, "walk".into(), 1))
    );
    assert_eq!(counted(&warnings, Tally::SliceLate), None);
}

#[test]
fn a_slice_whose_first_key_comes_later_uses_that_key() {
    let keys = [key(1, (7, 8, 3, 3)), key(3, (7, 8, 3, 3))];
    let (doc, warnings) = open(&file(vec![slice("late", &keys)]));
    assert_eq!(doc.slices[0].bounds.x, 7);
    // The keys are all the same, so nothing about the slice changes over time.
    assert_eq!(counted(&warnings, Tally::SliceKeys), None);
    // It starts at frame 2 and covers every frame here, which is told.
    assert_eq!(
        counted(&warnings, Tally::SliceLate),
        Some((1, "late".into(), 2))
    );
}

#[test]
fn a_long_name_is_shortened_and_said_so() {
    let name = "n".repeat(MAX_SLICE_NAME + 8);
    let (doc, warnings) = open(&file(vec![slice(&name, &[key(0, (0, 0, 1, 1))])]));
    assert_eq!(doc.slices[0].name.chars().count(), MAX_SLICE_NAME);
    assert_eq!(
        counted(&warnings, Tally::SliceName),
        Some((1, "n".repeat(MAX_SLICE_NAME), 0))
    );
}

#[test]
fn a_centre_outside_the_slice_is_dropped_and_the_slice_kept() {
    let k = SliceKey {
        center: Some((0, 0, 99, 99)),
        pivot: Some((1, 1)),
        ..key(0, (0, 0, 4, 4))
    };
    let (doc, warnings) = open(&file(vec![slice("bad", &[k])]));
    assert_eq!(doc.slices[0].center, None);
    assert_eq!(doc.slices[0].pivot, Some((1, 1)));
    assert_eq!(
        counted(&warnings, Tally::SliceParts),
        Some((1, "bad".into(), 0))
    );
}

#[test]
fn an_empty_or_far_off_slice_is_left_out_and_said_so() {
    let chunks = vec![
        slice("empty", &[key(0, (0, 0, 0, 5))]),
        slice("far", &[key(0, (i32::MAX, 0, 5, 5))]),
        slice("huge", &[key(0, (0, 0, u32::MAX, 5))]),
        slice("none", &[]),
        slice("ok", &[key(0, (0, 0, 5, 5))]),
    ];
    let (doc, warnings) = open(&file(chunks));
    let names: Vec<&str> = doc.slices.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["ok"]);
    // Four slices, told as one warning that names the first.
    assert_eq!(
        counted(&warnings, Tally::SliceDropped),
        Some((4, "empty".into(), 0))
    );
}

#[test]
fn slices_past_the_most_a_document_holds_are_left_out() {
    let chunks = (0..MAX_SLICES + 3)
        .map(|i| slice(&format!("s{i}"), &[key(0, (0, 0, 1, 1))]))
        .collect();
    let (doc, warnings) = open(&file(chunks));
    assert_eq!(doc.slices.len(), MAX_SLICES);
    assert_eq!(
        counted(&warnings, Tally::SliceDropped),
        Some((3, "s256".into(), 0))
    );
}

/// A slice chunk of these keys with `cut` bytes taken off its end, its size told to match.
fn cut_chunk(keys: &[SliceKey], cut: usize) -> Vec<u8> {
    let mut chunk = slice("x", keys);
    chunk.truncate(chunk.len() - cut);
    let size = (chunk.len() as u32).to_le_bytes();
    chunk[..4].copy_from_slice(&size);
    chunk
}

#[test]
fn a_slice_chunk_cut_short_keeps_the_keys_it_has_and_the_file_opens() {
    // The second key is cut; the first is whole.
    let keys = [key(0, (0, 0, 1, 1)), key(1, (2, 2, 3, 3))];
    let after = slice("after", &[key(0, (5, 5, 2, 2))]);
    let (doc, warnings) = open(&file(vec![cut_chunk(&keys, 3), after]));
    let names: Vec<&str> = doc.slices.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["x", "after"]);
    assert_eq!(doc.slices[0].bounds.x, 0);
    assert_eq!(
        counted(&warnings, Tally::SliceCut),
        Some((1, "x".into(), 0))
    );
}

#[test]
fn a_slice_chunk_cut_before_any_key_is_left_out_and_the_next_one_read() {
    let keys = [key(0, (0, 0, 1, 1))];
    let after = slice("after", &[key(0, (5, 5, 2, 2))]);
    let (doc, warnings) = open(&file(vec![cut_chunk(&keys, 3), after]));
    let names: Vec<&str> = doc.slices.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["after"]);
    assert_eq!(
        counted(&warnings, Tally::SliceDropped),
        Some((1, "x".into(), 0))
    );
}

#[test]
fn a_slice_that_claims_billions_of_keys_costs_no_memory() {
    let mut huge = slice("x", &[key(0, (0, 0, 1, 1))]);
    huge[6..10].copy_from_slice(&u32::MAX.to_le_bytes());
    let (doc, warnings) = open(&file(vec![huge]));
    // The one key there is is kept, and the chunk is told to be cut short.
    assert_eq!(doc.slices.len(), 1);
    assert!(counted(&warnings, Tally::SliceCut).is_some());
}

#[test]
fn the_earliest_key_with_a_rectangle_stands_for_the_slice() {
    let keys = [
        key(0, (0, 0, 0, 0)),
        key(4, (7, 7, 2, 2)),
        key(2, (3, 3, 2, 2)),
    ];
    let (doc, warnings) = open(&file(vec![slice("blinks", &keys)]));
    assert_eq!(doc.slices[0].bounds.x, 3);
    // It varies, and the key kept is at frame 3.
    assert_eq!(
        counted(&warnings, Tally::SliceKeys),
        Some((1, "blinks".into(), 3))
    );
    assert!(counted(&warnings, Tally::SliceLate).is_some());
}

#[test]
fn a_slice_whose_keys_are_all_empty_is_left_out() {
    let keys = [key(0, (0, 0, 0, 0)), key(1, (3, 3, 0, 4))];
    let (doc, warnings) = open(&file(vec![slice("gone", &keys)]));
    assert!(doc.slices.is_empty());
    assert!(counted(&warnings, Tally::SliceDropped).is_some());
}

#[test]
fn names_that_are_not_text_or_hold_control_characters_are_repaired_and_said_so() {
    let mut bad = slice("ab", &[key(0, (0, 0, 1, 1))]);
    // The name is the two bytes before the key (frame, x, y, width, height: 20 bytes).
    let at = bad.len() - 20 - 2;
    bad[at..at + 2].copy_from_slice(&[0xff, b'b']);
    let chunks = vec![
        bad,
        slice("a\u{202e}b\u{85}\n", &[key(0, (0, 0, 1, 1))]),
        slice("tidy", &[key(0, (0, 0, 1, 1))]),
    ];
    let (doc, warnings) = open(&file(chunks));
    assert_eq!(doc.slices[0].name, "\u{FFFD}b");
    assert_eq!(doc.slices[1].name, "a\u{FFFD}b\u{FFFD}\u{FFFD}");
    assert_eq!(doc.slices[2].name, "tidy");
    assert_eq!(counted(&warnings, Tally::LossyName).map(|c| c.0), Some(1));
    assert_eq!(counted(&warnings, Tally::CleanedName).map(|c| c.0), Some(1));
}

#[test]
fn layer_and_tag_names_are_repaired_too() {
    let ase = Ase::new(4, 4, 32).frame(
        100,
        vec![
            layer(1 | 2, 0, 0, 0, 255, "l\u{202a}\u{1}"),
            tags(&[(0, 0, 0, 0, "t\u{2066}")]),
        ],
    );
    let (doc, warnings) = open(&ase.bytes());
    assert_eq!(doc.layers()[0].name, "l\u{FFFD}\u{FFFD}");
    assert_eq!(doc.tags[0].name, "t\u{FFFD}");
    assert_eq!(counted(&warnings, Tally::CleanedName).map(|c| c.0), Some(2));
}

#[test]
fn a_slice_with_no_name_is_called_slice_n() {
    let chunks = vec![
        slice("Slice 1", &[key(0, (0, 0, 1, 1))]),
        slice("", &[key(0, (0, 0, 1, 1))]),
        slice("", &[key(0, (0, 0, 1, 1))]),
    ];
    let (doc, _) = open(&file(chunks));
    let names: Vec<&str> = doc.slices.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Slice 1", "Slice 2", "Slice 3"]);
}

#[test]
fn slice_warnings_have_words() {
    let said = Tally::SliceKeys.say(1, "a", 1);
    assert!(said.contains("\"a\"") && said.contains("frame 1"), "{said}");
    let many = Tally::SliceDropped.say(300, "a", 0);
    assert_eq!(many, "300 slices were left out, the first \"a\"");
    assert_eq!(Tally::SliceDropped.say(1, "", 0), "Slice \"\" was left out");
}
