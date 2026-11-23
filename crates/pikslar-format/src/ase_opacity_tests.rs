//! Cel opacity, multiplied into the alpha of the cel's pixels the way Aseprite rounds it.

use crate::ase_build::*;
use crate::ase_tests::{open, pixel};
use crate::AseWarning;
use pikslar_core::rgba;

/// Aseprite's 8-bit multiply: `a * b / 255`, rounded.
fn mul(a: u32, b: u32) -> u8 {
    let t = a * b + 0x80;
    (((t >> 8) + t) >> 8) as u8
}

#[test]
fn an_rgba_cels_opacity_goes_into_its_alpha_and_nothing_is_said() {
    let pixels = [
        10, 20, 30, 255, 10, 20, 30, 200, 10, 20, 30, 77, 10, 20, 30, 0,
    ];
    let ase = Ase::new(4, 4, 32).frame(
        100,
        vec![
            plain("L"),
            cel(0, 0, 0, 100, 0, 0, &sized_payload(4, 1, &pixels)),
        ],
    );
    let (doc, warnings) = open(&ase.bytes());
    assert!(warnings.is_empty(), "{warnings:?}");
    for (x, alpha) in [(0, 255u32), (1, 200), (2, 77), (3, 0)] {
        let want = rgba(10, 20, 30, mul(alpha, 100));
        assert_eq!(pixel(&doc, 0, 0, x, 0), want, "alpha {alpha}");
    }
}

#[test]
fn the_rounding_is_aseprites_for_every_alpha_and_opacity() {
    for opacity in [0u8, 1, 3, 127, 128, 254] {
        let pixels: Vec<u8> = (0..=255u8).flat_map(|a| [9, 8, 7, a]).collect();
        let c = cel(0, 0, 0, opacity, 0, 0, &sized_payload(16, 16, &pixels));
        let (doc, _) = open(&Ase::new(16, 16, 32).frame(100, vec![plain("L"), c]).bytes());
        for a in 0..=255u32 {
            let got = pixel(&doc, 0, 0, (a % 16) as i32, (a / 16) as i32);
            assert_eq!(
                got,
                rgba(9, 8, 7, mul(a, u32::from(opacity))),
                "{a} at {opacity}"
            );
        }
    }
}

#[test]
fn a_grayscale_cels_opacity_goes_into_its_alpha() {
    let c = cel(0, 0, 0, 128, 0, 0, &sized_payload(1, 1, &[90, 255]));
    let (doc, warnings) = open(&Ase::new(2, 2, 16).frame(100, vec![plain("L"), c]).bytes());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(pixel(&doc, 0, 0, 0, 0), rgba(90, 90, 90, 128));
}

#[test]
fn full_opacity_changes_nothing_and_zero_clears_the_cel() {
    let px = [1, 2, 3, 255];
    let at = |opacity| {
        let c = cel(0, 0, 0, opacity, 0, 0, &sized_payload(1, 1, &px));
        let (doc, _) = open(&Ase::new(2, 2, 32).frame(100, vec![plain("L"), c]).bytes());
        pixel(&doc, 0, 0, 0, 0)
    };
    assert_eq!(at(255), rgba(1, 2, 3, 255));
    assert_eq!(at(0), 0);
}

#[test]
fn an_indexed_cels_opacity_is_still_not_kept_and_says_so() {
    let c = cel(0, 0, 0, 100, 0, 0, &sized_payload(1, 1, &[1]));
    let ase = Ase::new(2, 2, 8).frame(
        100,
        vec![plain("L"), palette(2, 0, &[[0; 4], [9, 9, 9, 255]]), c],
    );
    let (_, warnings) = open(&ase.bytes());
    assert!(warnings.contains(&AseWarning::CelOpacity("L".into())));
}

#[test]
fn a_link_with_the_opacity_of_its_source_says_nothing_and_one_with_another_does() {
    let px = [1, 2, 3, 200];
    let link = |opacity| cel(0, 0, 0, opacity, 1, 0, &0u16.to_le_bytes());
    let frames = |opacity| {
        Ase::new(2, 2, 32)
            .frame(
                100,
                vec![
                    plain("L"),
                    cel(0, 0, 0, 100, 0, 0, &sized_payload(1, 1, &px)),
                ],
            )
            .frame(100, vec![link(opacity)])
    };
    let (doc, warnings) = open(&frames(100).bytes());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(pixel(&doc, 0, 1, 0, 0), rgba(1, 2, 3, mul(200, 100)));
    assert!(doc.is_held(0, 1));
    let (doc, warnings) = open(&frames(30).bytes());
    assert!(warnings.contains(&AseWarning::CelOpacity("L".into())));
    assert_eq!(pixel(&doc, 0, 1, 0, 0), rgba(1, 2, 3, mul(200, 100)));
}
