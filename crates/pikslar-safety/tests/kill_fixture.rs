//! The project the RC3 kill matrix (spikes/kill-matrix.ps1) saves, restores and exports: big
//! enough that each write of it takes long enough to be stopped part way. Ignored; written
//! only when asked for:
//!
//!   PIKSLAR_KILL_FIXTURE=<file> cargo test -p pikslar-safety --test kill_fixture -- --ignored
//!
//! Noise does not compress, so the file is about the size of its pixels.

use pikslar_core::{ColorMode, Document, Grid};
use pikslar_format::piks;

const SIDE: usize = 384;
const FRAMES: usize = 6;
const LAYERS: usize = 2;

/// Noise of the given opacity. The layers above the first are half see-through, so a stroke on
/// the first layer (the one the program selects) changes the picture the checks compare.
fn noise(seed: u32, alpha: u32) -> Vec<u32> {
    let mut x = seed.wrapping_mul(2_654_435_761) | 1;
    (0..SIDE * SIDE)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (alpha << 24) | (x >> 8)
        })
        .collect()
}

fn big_document(offset: u32) -> Document {
    let mut doc = Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid size");
    for layer in 1..LAYERS {
        doc.add_layer(layer, ColorMode::Rgba)
            .expect("room for a layer");
    }
    for frame in 1..FRAMES {
        doc.add_frame(frame, false).expect("room for a frame");
    }
    for frame in 0..FRAMES {
        for layer in 0..LAYERS {
            let seed = (frame * LAYERS + layer) as u32 + 1 + offset;
            doc.set_cel(
                layer,
                frame,
                Grid::from_cells(
                    SIDE,
                    SIDE,
                    &noise(seed, if layer == 0 { 0xff } else { 0x80 }),
                ),
            )
            .expect("canvas size");
        }
    }
    doc
}

#[test]
#[ignore]
fn write_the_kill_matrix_project() {
    let path =
        std::env::var_os("PIKSLAR_KILL_FIXTURE").expect("PIKSLAR_KILL_FIXTURE names the file");
    // A second project, of other pixels, for the export kills: PIKSLAR_KILL_FIXTURE_SEED=100.
    let offset = std::env::var("PIKSLAR_KILL_FIXTURE_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let bytes = piks::save(&big_document(offset));
    let again = piks::load(&bytes).expect("the project reads back");
    assert_eq!(again.layers().len(), LAYERS);
    assert_eq!(again.layers()[0].frame_count(), FRAMES);
    std::fs::write(&path, &bytes).expect("the project is written");
}
