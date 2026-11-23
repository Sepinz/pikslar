//! The decode limit for every version: what a file is charged, refused at the byte where it
//! would pass the limit, and sizes a file claims but cannot hold refused before anything is
//! made for them.

use super::*;
use std::time::{Duration, Instant};

macro_rules! frozen {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!("../../../pikslar-safety/corpus/", $name)).as_slice(),
        )
    };
}

/// One frozen file of each version and kind, as earlier builds wrote them.
fn frozen() -> [(&'static str, &'static [u8]); 6] {
    [
        frozen!("v1-layers.piks"),
        frozen!("v2-animated.piks"),
        frozen!("v2-large-sparse.piks"),
        frozen!("v2-recovery-lifted.piks"),
        frozen!("v3-groups.piks"),
        frozen!("v3-tilemap.piks"),
    ]
}

/// Reads `bytes` with `room` of the limit left.
fn read_with_room(bytes: &[u8], room: usize) -> Result<(), FormatError> {
    let mut spent = MAX_DECODED_BYTES - room;
    parse(bytes, &mut spent).map(|_| ())
}

#[test]
fn every_version_opens_with_exactly_its_charge_left_and_not_a_byte_less() {
    for (name, bytes) in frozen() {
        let charge = charges(bytes).expect(name);
        assert!(charge > 0, "{name}");
        assert!(read_with_room(bytes, charge).is_ok(), "{name}");
        assert!(
            matches!(
                read_with_room(bytes, charge - 1),
                Err(FormatError::FileTooLarge)
            ),
            "{name}"
        );
        let recovered = load_recovery(bytes).expect(name);
        assert!(!recovered.floating_damaged, "{name}");
        if name.contains("recovery") {
            assert!(
                recovered.floating.is_some(),
                "{name} keeps its floating pixels"
            );
        }
    }
}

#[test]
fn frozen_files_are_charged_far_below_the_limit() {
    for (name, bytes) in frozen() {
        let charge = charges(bytes).expect(name);
        assert!(charge < 16 << 20, "{name}: {charge}");
    }
}

/// Every cel is charged before any is read: a file of the most layers and frames, with nothing
/// after its counts, is refused for its size when the limit has no room for its cels.
#[test]
fn the_cels_are_charged_before_any_is_read() {
    for version in [1u32, 2, 3] {
        let bytes = claimed(version, 256, 4096, 1);
        let cels = 256 * 4096 * CEL_CHARGE;
        assert!(
            matches!(
                read_with_room(&bytes, cels - 1),
                Err(FormatError::FileTooLarge)
            ),
            "version {version}"
        );
        // With room, it fails later, for the data it lacks.
        assert!(
            !matches!(
                read_with_room(&bytes, cels + (1 << 20)),
                Err(FormatError::FileTooLarge) | Ok(())
            ),
            "version {version}"
        );
    }
}

/// A file that claims the most of everything on the largest canvas and holds almost nothing is
/// refused at once, in every version: nothing is made in proportion to what it claims.
#[test]
fn claimed_sizes_a_file_cannot_hold_are_refused_at_once() {
    for version in [1u32, 2, 3] {
        for images in [1u32, 4096, 256 * 4096] {
            let bytes = claimed(version, 256, 4096, images);
            let started = Instant::now();
            assert!(load(&bytes).is_err(), "version {version}, {images} images");
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "version {version}, {images} images: {:?}",
                started.elapsed()
            );
        }
    }
}

/// A sealed file of `version` on the largest canvas with `layers` layers of `frames` frames and
/// `images` images claimed, every layer record whole (version 1 and 2 layers list their cels)
/// and nothing after the image count.
fn claimed(version: u32, layers: u32, frames: u32, images: u32) -> Vec<u8> {
    let mut out = Out(Vec::new());
    out.0.extend_from_slice(MAGIC);
    out.u32(version);
    if version == 3 {
        out.u32(0);
    }
    out.u32(MAX_DIMENSION as u32);
    out.u32(MAX_DIMENSION as u32);
    out.u32s(&[0]);
    out.u32s(&[]);
    out.u32s(&vec![100; frames as usize]);
    out.u32(layers);
    for n in 0..layers {
        match version {
            3 => {
                let mut record = Out(Vec::new());
                record.u32(n + 1);
                record.0.extend_from_slice(&[1, 255, 0, 1]);
                record.u16(0);
                out.u16(record.0.len() as u16);
                out.0.extend_from_slice(&record.0);
            }
            _ => {
                out.u16(0);
                out.0.extend_from_slice(&[1, 255, 0, 1]);
                (0..frames).for_each(|_| out.u32(0));
            }
        }
    }
    out.u32(images);
    let crc = crc32(&[&out.0]);
    out.u32(crc);
    out.0
}
