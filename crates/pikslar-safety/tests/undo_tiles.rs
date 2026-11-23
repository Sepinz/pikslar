//! Random sequences with tilemaps (tiles, pixels drawn on tiles, tilesets, frames, links,
//! terrain sets and terrain strokes) among every other edit, over 2,000 random documents. Undo
//! and redo give identical documents, the maps keep their rules after every step, and saving and
//! opening again in the middle of a run loses nothing.

use pikslar_core::ColorMode;
use pikslar_safety::undo_model::{run, Counts, Plan};

/// Sequences in each colour mode; together they are the 2,000 the plan asks for.
const SEQUENCES: u64 = 1_000;
const STEPS: usize = 80;
/// Seeds clear of the other undo suites', so these are 2,000 sequences of their own.
const FIRST_SEED: u64 = 1_000_000;

fn plan(mode: ColorMode, budget: Option<usize>) -> Plan {
    Plan {
        mode,
        steps: STEPS,
        budget,
    }
}

fn check(mode: ColorMode) -> Counts {
    let mut total = Counts::default();
    for seed in FIRST_SEED..FIRST_SEED + SEQUENCES {
        let counts = run(plan(mode, None), seed).unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        total.merge(&counts);
    }
    total
}

/// Floors near half of what the runs made when written, so a change that stops the tilemap
/// actions from happening fails here.
fn did_real_work(counts: Counts) {
    assert!(counts.tiles > 2_500, "{counts:?}");
    assert!(counts.map_strokes > 200, "{counts:?}");
    assert!(counts.tile_pixels > 60, "{counts:?}");
    assert!(counts.terrains > 120, "{counts:?}");
    assert!(counts.terrain_strokes > 25, "{counts:?}");
    assert!(counts.saves > 400, "{counts:?}");
    assert!(counts.undos > 5_000, "{counts:?}");
}

#[test]
fn palette_documents_undo_exactly_with_tilemaps() {
    did_real_work(check(ColorMode::Palette));
}

#[test]
fn rgba_documents_undo_exactly_with_tilemaps() {
    did_real_work(check(ColorMode::Rgba));
}

#[test]
fn a_tiny_history_undoes_exactly_with_tilemaps() {
    for seed in FIRST_SEED..FIRST_SEED + 200 {
        for mode in [ColorMode::Palette, ColorMode::Rgba] {
            run(plan(mode, Some(8 * 1024)), seed).unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        }
    }
}
