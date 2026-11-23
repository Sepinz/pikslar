//! Files the importer must refuse, or read with care: damaged sizes, limits and bad cels.

use crate::ase_build::*;
use crate::ase_tests::{one_layer, open, pixel, refused};
use crate::{import_ase, FormatError};
use pikslar_core::rgba;

#[test]
fn damaged_headers_are_refused() {
    let good = one_layer(4, 4, vec![]);
    assert!(matches!(refused(&good[..100]), FormatError::Corrupt(_)));
    assert!(matches!(refused(&[]), FormatError::Corrupt(_)));
    let mut bad = good.clone();
    bad[4] = 0;
    assert!(matches!(refused(&bad), FormatError::Corrupt(_)));
    let mut depth = good.clone();
    depth[12] = 24;
    assert!(matches!(refused(&depth), FormatError::Corrupt(_)));
    for (at, value) in [(6usize, 0u16), (8, 0), (10, 0)] {
        let mut zero = good.clone();
        zero[at..at + 2].copy_from_slice(&value.to_le_bytes());
        assert!(
            matches!(refused(&zero), FormatError::Corrupt(_)),
            "field {at}"
        );
    }
}

#[test]
fn a_canvas_past_the_limit_is_too_large() {
    let ase = Ase::new(8193, 10, 32).frame(100, vec![plain("L")]);
    assert!(matches!(refused(&ase.bytes()), FormatError::TooLarge));
    let ase = Ase::new(10, 8193, 32).frame(100, vec![plain("L")]);
    assert!(matches!(refused(&ase.bytes()), FormatError::TooLarge));
    let ase = Ase::new(8192, 8192, 32).frame(100, vec![plain("L")]);
    open(&ase.bytes());
}

#[test]
fn more_frames_than_the_file_holds_or_the_limit_allows_are_refused() {
    let mut bytes = one_layer(4, 4, vec![]);
    bytes[6..8].copy_from_slice(&4000u16.to_le_bytes());
    assert!(matches!(refused(&bytes), FormatError::Corrupt(_)));
    let mut ase = Ase::new(1, 1, 32).frame(100, vec![plain("L")]);
    for _ in 0..4096 {
        ase = ase.frame(100, vec![]);
    }
    assert!(matches!(refused(&ase.bytes()), FormatError::Unsupported(_)));
    let mut ok = Ase::new(1, 1, 32).frame(100, vec![plain("L")]);
    for _ in 1..4096 {
        ok = ok.frame(100, vec![]);
    }
    assert_eq!(open(&ok.bytes()).0.frames.len(), 4096);
}

#[test]
fn more_layers_than_a_document_holds_are_refused() {
    let many = |n: usize| {
        let chunks = (0..n).map(|i| plain(&format!("l{i}"))).collect();
        Ase::new(1, 1, 32).frame(100, chunks).bytes()
    };
    assert_eq!(open(&many(256)).0.layers().len(), 256);
    assert!(matches!(refused(&many(257)), FormatError::Unsupported(_)));
}

#[test]
fn a_file_without_layers_is_refused() {
    let err = refused(&Ase::new(2, 2, 32).frame(100, vec![]).bytes());
    assert!(matches!(err, FormatError::Corrupt(_)));
}

#[test]
fn a_file_of_tilemap_layers_only_is_unsupported_not_damaged() {
    // Aseprite writes such files (its own test sprites have some): nothing in them is wrong.
    let only_tiles = Ase::new(2, 2, 32).frame(100, vec![layer(3, 2, 0, 0, 255, "t")]);
    assert!(matches!(
        refused(&only_tiles.bytes()),
        FormatError::Unsupported(_)
    ));
    let unknown = Ase::new(2, 2, 32).frame(100, vec![layer(3, 7, 0, 0, 255, "?")]);
    assert!(matches!(
        refused(&unknown.bytes()),
        FormatError::Unsupported(_)
    ));
}

#[test]
fn frames_and_chunks_that_lie_about_their_size_are_refused() {
    let good = one_layer(4, 4, vec![cel_raw(0, 0, 0, 1, 1, &[1, 1, 1, 255])]);
    // The frame's size is past the file.
    for cut in [1, 5, 40] {
        let err = refused(&good[..good.len() - cut]);
        assert!(matches!(err, FormatError::Corrupt(_)), "cut {cut}: {err}");
    }
    let mut small = good.clone();
    small[128..132].copy_from_slice(&15u32.to_le_bytes());
    assert!(matches!(refused(&small), FormatError::Corrupt(_)));
    let mut magic = good.clone();
    magic[132] ^= 1;
    assert!(matches!(refused(&magic), FormatError::Corrupt(_)));
    // A chunk of size 5 cannot hold its own header; one that runs past the frame is cut short.
    let mut chunk_small = good.clone();
    chunk_small[144..148].copy_from_slice(&5u32.to_le_bytes());
    assert!(matches!(refused(&chunk_small), FormatError::Corrupt(_)));
    let mut chunk_big = good.clone();
    chunk_big[144..148].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(refused(&chunk_big), FormatError::Corrupt(_)));
}

#[test]
fn the_newer_chunk_count_is_used_when_the_old_one_is_full() {
    let mut bytes = one_layer(4, 4, vec![cel_raw(0, 0, 0, 1, 1, &[1, 1, 1, 255])]);
    bytes[134..136].copy_from_slice(&0xFFFFu16.to_le_bytes());
    assert_eq!(pixel(&open(&bytes).0, 0, 0, 0, 0), rgba(1, 1, 1, 255));
    // With no newer count, the old one is used.
    bytes[138..142].copy_from_slice(&0u32.to_le_bytes());
    bytes[134..136].copy_from_slice(&2u16.to_le_bytes());
    assert_eq!(pixel(&open(&bytes).0, 0, 0, 0, 0), rgba(1, 1, 1, 255));
}

fn bad_cels() -> Vec<(&'static str, Vec<u8>)> {
    let pic = rgba_picture(2, 2, 1);
    vec![
        ("no such layer", cel_raw(5, 0, 0, 1, 1, &[1, 1, 1, 255])),
        ("short raw pixels", cel_raw(0, 0, 0, 2, 2, &pic[..15])),
        (
            "past the offset limit",
            cel_raw(0, 25_000, 0, 1, 1, &[1, 1, 1, 255]),
        ),
        ("a cel too wide", cel_raw(0, 0, 0, 40_000, 1, &[])),
        (
            "a cel too big",
            cel(0, 0, 0, 255, 2, 0, &[0xff, 0x7f, 0xff, 0x7f]),
        ),
        (
            "not zlib",
            cel(0, 0, 0, 255, 2, 0, &[1, 0, 1, 0, 1, 2, 3, 4, 5, 6, 7, 8]),
        ),
        (
            "a dictionary",
            cel(
                0,
                0,
                0,
                255,
                2,
                0,
                &[1, 0, 1, 0, 0x78, 0xbb, 1, 2, 3, 4, 5, 6, 7],
            ),
        ),
        (
            "a bad check",
            cel(
                0,
                0,
                0,
                255,
                2,
                0,
                &[1, 0, 1, 0, 0x78, 0x9d, 1, 2, 3, 4, 5, 6, 7],
            ),
        ),
        ("too little", cel_zlib(0, 0, 0, 2, 2, &pic[..8])),
        (
            "too much",
            cel(
                0,
                0,
                0,
                255,
                2,
                0,
                &[1, 0, 1, 0]
                    .iter()
                    .chain(&zlib(&pic))
                    .copied()
                    .collect::<Vec<u8>>(),
            ),
        ),
        ("no stream", cel(0, 0, 0, 255, 2, 0, &[1, 0, 1, 0])),
        ("no size", cel(0, 0, 0, 255, 0, 0, &[1])),
    ]
}

#[test]
fn a_cel_that_cannot_be_is_refused() {
    for (what, c) in bad_cels() {
        let err = import_ase(&one_layer(4, 4, vec![c])).expect_err(what);
        assert!(!matches!(err, FormatError::Io(_)), "{what}");
    }
    let two = one_layer(
        4,
        4,
        vec![
            cel_raw(0, 0, 0, 1, 1, &[1, 1, 1, 255]),
            cel_raw(0, 0, 0, 1, 1, &[2, 2, 2, 255]),
        ],
    );
    assert!(matches!(refused(&two), FormatError::Corrupt(_)));
}

#[test]
fn a_cel_larger_than_the_canvas_keeps_its_pixels_past_it() {
    let pic = rgba_picture(30, 30, 1);
    let (doc, _) = open(&one_layer(4, 4, vec![cel_zlib(0, -10, -10, 30, 30, &pic)]));
    assert_ne!(pixel(&doc, 0, 0, 15, 15), 0);
    assert_eq!(doc.layers()[0].cel(0).position(), (-10, -10));
}

#[test]
fn a_stream_that_claims_far_more_than_it_holds_is_refused_without_the_memory() {
    // 8192 by 8192 RGBA claimed, a few bytes of stream given.
    let claim = [0, 0x20, 0, 0x20];
    let mut payload = claim.to_vec();
    payload.extend(zlib(&[7; 10]));
    let err = refused(&one_layer(
        8192,
        8192,
        vec![cel(0, 0, 0, 255, 2, 0, &payload)],
    ));
    assert!(matches!(err, FormatError::Corrupt(_)), "{err}");
}

/// A 1 by 1 RGBA cel whose zlib header is `head`, then a good deflate stream.
fn cel_with_header(head: [u8; 2]) -> Vec<u8> {
    let mut payload = vec![1, 0, 1, 0, head[0], head[1]];
    payload.extend(crate::deflate::deflate(&[1, 2, 3, 255]));
    cel(0, 0, 0, 255, 2, 0, &payload)
}

#[test]
fn only_a_plain_deflate_zlib_header_is_read() {
    let ok = import_ase(&one_layer(4, 4, vec![cel_with_header([0x78, 0x9c])]));
    assert!(ok.is_ok());
    // A 32 KiB window is the largest; 0x88 asks for 64 KiB, and the check bits are right.
    assert_eq!((0x88u32 * 256 + 0x1c) % 31, 0);
    // A preset dictionary (bit 5 of the second byte), with check bits that are right.
    assert_eq!((0x78u32 * 256 + 0xbb) % 31, 0);
    // Method 9 is not deflate; 0x789d has wrong check bits.
    for head in [[0x88, 0x1c], [0x78, 0xbb], [0x79, 0x9d], [0x78, 0x9d]] {
        let err = refused(&one_layer(4, 4, vec![cel_with_header(head)]));
        assert!(
            matches!(
                err,
                FormatError::Corrupt("a cel's pixels are not a zlib stream")
            ),
            "{head:02x?}: {err}"
        );
    }
}

#[test]
fn a_palette_range_must_lie_inside_the_palette() {
    let colors = [[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 255]];
    // Entries 0 to 2 of a palette of 2.
    let ase = Ase::new(2, 2, 32).frame(100, vec![plain("L"), palette(2, 0, &colors)]);
    let err = refused(&ase.bytes());
    assert!(matches!(
        err,
        FormatError::Corrupt("a palette range lies outside the palette")
    ));
    // Entries 0 to 2 of a palette of 3 are fine.
    let ase = Ase::new(2, 2, 32).frame(100, vec![plain("L"), palette(3, 0, &colors)]);
    assert!(import_ase(&ase.bytes()).is_ok());
}

#[test]
fn a_palette_chunk_that_claims_more_entries_than_it_holds_is_refused_early() {
    let mut d = Vec::new();
    for v in [65_535u32, 0, 65_534] {
        d.extend_from_slice(&v.to_le_bytes());
    }
    d.extend_from_slice(&[0; 8 + 12]);
    let ase = Ase::new(2, 2, 32).frame(100, vec![plain("L"), chunk(0x2019, &d)]);
    let err = refused(&ase.bytes());
    assert!(
        matches!(err, FormatError::Corrupt("a palette chunk is cut short")),
        "{err}"
    );
    let mut big = d.clone();
    big[0..4].copy_from_slice(&65_536u32.to_le_bytes());
    let ase = Ase::new(2, 2, 32).frame(100, vec![plain("L"), chunk(0x2019, &big)]);
    assert!(matches!(refused(&ase.bytes()), FormatError::Unsupported(_)));
}

#[test]
fn an_old_palette_may_not_run_past_256_colors() {
    let past = vec![(255u8, vec![[1u8, 1, 1]; 2])];
    let ase = Ase::new(2, 2, 32).frame(100, vec![plain("L"), old_palette(0x0004, &past)]);
    assert!(matches!(refused(&ase.bytes()), FormatError::Corrupt(_)));
    let last = vec![(255u8, vec![[1u8, 1, 1]])];
    let ase = Ase::new(2, 2, 32).frame(100, vec![plain("L"), old_palette(0x0004, &last)]);
    assert_eq!(import_ase(&ase.bytes()).expect("fits").0.colors.len(), 257);
}

#[test]
fn a_tag_must_end_on_a_frame_the_file_has() {
    let list = [(0, 1, 0, 0, "at the end"), (0, 0, 0, 0, "fine")];
    let ase = Ase::new(2, 2, 32).frame(100, vec![plain("L"), tags(&list)]);
    let (doc, warnings) = open(&ase.bytes());
    assert_eq!(doc.tags.len(), 1);
    assert!(warnings.contains(&crate::AseWarning::TagDropped("at the end".into())));
}

#[test]
fn the_layer_that_sits_too_deep_and_the_missing_frames_are_named() {
    let chunks = vec![plain("a"), layer(3, 0, 1, 0, 255, "b")];
    let err = refused(&Ase::new(2, 2, 32).frame(100, chunks).bytes());
    assert!(matches!(
        err,
        FormatError::Corrupt("a layer sits deeper than its group")
    ));
    let mut bytes = one_layer(4, 4, vec![]);
    bytes[6..8].copy_from_slice(&4000u16.to_le_bytes());
    let err = refused(&bytes);
    assert!(matches!(
        err,
        FormatError::Corrupt("the file has fewer frames than it says")
    ));
}

/// A layer chunk is a few bytes, and the table of frames it makes is charged to what the file may
/// hold, so many layers over many frames cannot hold memory the budget does not see.
#[test]
fn a_layer_charges_its_table_of_frames_to_the_file() {
    use crate::ase_cel::Budget;
    use crate::ase_chunks::{Depth, Sprite, FRAME_SLOT_BYTES};
    let table = (4096 * FRAME_SLOT_BYTES) as u64;
    let budget = |held| Budget {
        work: u64::MAX,
        held,
    };
    let data = &plain("L")[6..];
    let mut sprite = Sprite::new(8, 8, Depth::Rgba, 4096, budget(table));
    sprite.layer_chunk(data).expect("exactly room");
    assert_eq!(sprite.budget.held, 0);
    let mut sprite = Sprite::new(8, 8, Depth::Rgba, 4096, budget(table - 1));
    assert!(matches!(
        sprite.layer_chunk(data),
        Err(FormatError::FileTooLarge)
    ));
    // A group makes no table, so it costs nothing.
    let mut sprite = Sprite::new(8, 8, Depth::Rgba, 4096, budget(0));
    sprite
        .layer_chunk(&layer(3, 1, 0, 0, 255, "G")[6..])
        .expect("a group");
}

/// A large packed cel asks whether to stop as it is unpacked, about once a mebibyte, and a
/// true answer there ends the read: a stop does not wait for the whole picture.
#[test]
fn a_packed_cel_asks_to_stop_as_it_unpacks() {
    use crate::inflate::STOP_EVERY_BYTES;
    let side = 1024u16;
    let pixels = vec![0x40u8; usize::from(side) * usize::from(side) * 4];
    let mut payload = [side.to_le_bytes(), side.to_le_bytes()].concat();
    payload.extend_from_slice(&zlib(&pixels));
    let bytes = Ase::new(side, side, 32)
        .frame(100, vec![plain("a"), cel(0, 0, 0, 255, 2, 0, &payload)])
        .bytes();
    let asked = std::cell::Cell::new(0usize);
    let count = || {
        asked.set(asked.get() + 1);
        false
    };
    assert!(crate::ase_read::import_ase_until(&bytes, &count).is_ok());
    let unpacking = pixels.len() / STOP_EVERY_BYTES;
    assert!(asked.get() >= unpacking + 2, "asked {} times", asked.get());
    // Told to stop on the second question, which is asked inside the cel's stream.
    asked.set(0);
    let second = || {
        asked.set(asked.get() + 1);
        asked.get() >= 2
    };
    let stopped = crate::ase_read::import_ase_until(&bytes, &second);
    assert!(matches!(stopped, Err(FormatError::Stopped)), "{stopped:?}");
    assert_eq!(asked.get(), 2);
}
