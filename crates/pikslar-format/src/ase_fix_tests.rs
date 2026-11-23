//! Tests for the findings of the review of the importer: checksums, palettes, and the caps
//! that keep a hostile file's cost in step with its size.

use crate::ase_build::*;
use crate::ase_read::Warnings;
use crate::ase_tests::{one_layer, open, pixel, refused};
use crate::{AseWarning, FormatError};
use pikslar_core::rgba;
use std::time::{Duration, Instant};

/// A zlib cel payload of `w` by `h` with its checksum changed by `edit`.
fn cel_with_trailer(w: u16, h: u16, pixels: &[u8], edit: impl Fn(&mut Vec<u8>)) -> Vec<u8> {
    let mut stream = zlib(pixels);
    edit(&mut stream);
    let mut payload = Vec::new();
    payload.extend_from_slice(&w.to_le_bytes());
    payload.extend_from_slice(&h.to_le_bytes());
    payload.extend_from_slice(&stream);
    cel(0, 0, 0, 255, 2, 0, &payload)
}

fn red() -> Vec<u8> {
    [255, 0, 0, 255].repeat(4)
}

#[test]
fn a_cel_with_a_wrong_checksum_is_left_empty_and_the_rest_opens() {
    let bad = cel_with_trailer(2, 2, &red(), |s| {
        let last = s.len() - 1;
        s[last] ^= 0x55;
    });
    let ase = Ase::new(4, 4, 32)
        .frame(100, vec![plain("L"), bad])
        .frame(100, vec![cel_zlib(0, 0, 0, 2, 2, &red())]);
    let (doc, warnings) = open(&ase.bytes());
    assert_eq!(pixel(&doc, 0, 0, 0, 0), 0);
    assert_eq!(pixel(&doc, 0, 1, 0, 0), rgba(255, 0, 0, 255));
    let damaged = AseWarning::Damaged {
        layer: "L".into(),
        frame: 0,
    };
    assert!(warnings.contains(&damaged), "{warnings:?}");
    assert!(damaged.to_string().contains("\"L\""));
}

#[test]
fn a_cel_with_no_checksum_at_all_is_left_empty_too() {
    let cut = cel_with_trailer(2, 2, &red(), |s| s.truncate(s.len() - 4));
    let (doc, warnings) = open(&one_layer(4, 4, vec![cut]));
    assert_eq!(pixel(&doc, 0, 0, 0, 0), 0);
    assert!(warnings
        .iter()
        .any(|w| matches!(w, AseWarning::Damaged { .. })));
}

#[test]
fn a_right_checksum_warns_of_nothing() {
    let good = cel_with_trailer(2, 2, &red(), |_| {});
    let (doc, warnings) = open(&one_layer(4, 4, vec![good]));
    assert_eq!(pixel(&doc, 0, 0, 1, 1), rgba(255, 0, 0, 255));
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_stored_block_checksum_is_checked_too() {
    let mut stream = zlib_stored(&red());
    let last = stream.len() - 1;
    stream[last] ^= 1;
    let mut payload = vec![2, 0, 2, 0];
    payload.extend_from_slice(&stream);
    let (_, warnings) = open(&one_layer(4, 4, vec![cel(0, 0, 0, 255, 2, 0, &payload)]));
    assert!(warnings
        .iter()
        .any(|w| matches!(w, AseWarning::Damaged { .. })));
}

#[test]
fn a_palette_never_shrinks_when_a_later_chunk_is_smaller() {
    let big = palette(10, 0, &[[9, 9, 9, 255]; 10]);
    let small = palette(3, 0, &[[1, 2, 3, 255]]);
    let (doc, _) = open(&one_layer(4, 4, vec![big, small]));
    assert_eq!(doc.colors.colors()[1], rgba(1, 2, 3, 255));
    assert_eq!(doc.colors.colors()[10], rgba(9, 9, 9, 255));
}

#[test]
fn an_old_palette_after_the_first_frame_is_a_change_too() {
    for kind in [0x0004, 0x0011] {
        let old = old_palette(kind, &[(0, vec![[1, 2, 3]])]);
        let ase = Ase::new(2, 2, 8)
            .frame(100, vec![plain("L")])
            .frame(100, vec![old]);
        let (_, warnings) = open(&ase.bytes());
        assert!(warnings.contains(&AseWarning::PaletteChanges), "{kind}");
    }
    let old = old_palette(0x0004, &[(0, vec![[1, 2, 3]])]);
    let (_, warnings) = open(&Ase::new(2, 2, 8).frame(100, vec![plain("L"), old]).bytes());
    assert!(!warnings.contains(&AseWarning::PaletteChanges));
}

#[test]
fn an_index_past_the_palette_is_warned_of() {
    let colors = palette(3, 0, &[[1, 1, 1, 255]; 3]);
    let ase = Ase::new(4, 4, 8).frame(
        100,
        vec![plain("L"), colors, cel_raw(0, 0, 0, 2, 1, &[2, 3])],
    );
    let (_, warnings) = open(&ase.bytes());
    assert!(
        warnings.contains(&AseWarning::IndexPastPalette),
        "{warnings:?}"
    );
    let inside = palette(3, 0, &[[1, 1, 1, 255]; 3]);
    let ase = Ase::new(4, 4, 8).frame(
        100,
        vec![plain("L"), inside, cel_raw(0, 0, 0, 2, 1, &[1, 2])],
    );
    let (_, warnings) = open(&ase.bytes());
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn the_transparent_index_does_not_count_as_past_the_palette() {
    let mut ase = Ase::new(4, 4, 8);
    ase.transparent = 200;
    let colors = palette(3, 0, &[[1, 1, 1, 255]; 3]);
    let (_, warnings) = open(
        &ase.frame(
            100,
            vec![plain("L"), colors, cel_raw(0, 0, 0, 1, 1, &[200])],
        )
        .bytes(),
    );
    assert!(
        !warnings.contains(&AseWarning::IndexPastPalette),
        "{warnings:?}"
    );
}

#[test]
fn an_indexed_file_with_no_palette_says_so() {
    let ase = Ase::new(4, 4, 8).frame(100, vec![plain("L"), cel_raw(0, 0, 0, 1, 1, &[5])]);
    let (_, warnings) = open(&ase.bytes());
    assert!(warnings.contains(&AseWarning::NoPalette), "{warnings:?}");
    let (_, warnings) = open(&one_layer(4, 4, vec![]));
    assert!(!warnings.contains(&AseWarning::NoPalette));
}

#[test]
fn a_repeated_warning_costs_no_scan_of_the_others() {
    let mut warnings = Warnings::default();
    for i in 0..250 {
        warnings.add(AseWarning::TagDropped(format!("tag {i}")));
    }
    let started = Instant::now();
    for _ in 0..2_000_000 {
        warnings.add(AseWarning::Skipped("slices"));
        warnings.add(AseWarning::CelOpacity("layer".into()));
    }
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn skipped_layers_are_counted_against_a_cap() {
    let skipped = layer(3, 7, 0, 0, 255, "t");
    let mut chunks = vec![plain("L")];
    chunks.extend(std::iter::repeat_n(skipped, 65_535));
    let ase = Ase::new(2, 2, 32).frame(100, chunks.clone());
    let (doc, _) = open(&ase.bytes());
    assert_eq!(doc.layers().len(), 1);
    chunks.push(layer(3, 7, 0, 0, 255, "t"));
    let err = refused(&Ase::new(2, 2, 32).frame(100, chunks).bytes());
    assert!(matches!(err, FormatError::Unsupported(_)), "{err}");
}

#[test]
fn slice_notes_never_crowd_out_the_warning_for_a_damaged_drawing() {
    let mut chunks = vec![plain("L")];
    // 300 slices that cannot be kept, then a drawing that is damaged.
    chunks.extend((0..300).map(|i| {
        let key = SliceKey {
            frame: 0,
            bounds: (0, 0, 0, 0),
            center: None,
            pivot: None,
        };
        slice(&format!("s{i}"), &[key])
    }));
    chunks.push(cel_with_trailer(2, 2, &red(), |s| {
        let last = s.len() - 1;
        s[last] ^= 0x55;
    }));
    let (_, warnings) = open(&Ase::new(4, 4, 32).frame(100, chunks).bytes());
    let damaged = AseWarning::Damaged {
        layer: "L".into(),
        frame: 0,
    };
    assert!(warnings.contains(&damaged), "{warnings:?}");
    assert!(warnings.len() < 10, "{}", warnings.len());
}

#[test]
fn notes_about_metadata_leave_places_for_lost_art() {
    let mut warnings = Warnings::default();
    for i in 0..1000 {
        warnings.add(AseWarning::TagDropped(format!("t{i}")));
    }
    let damaged = AseWarning::Damaged {
        layer: "L".into(),
        frame: 0,
    };
    warnings.add(damaged.clone());
    let all = warnings.into_vec();
    assert!(all.contains(&damaged));
    assert_eq!(all.last(), Some(&AseWarning::TooMany));
}
