//! Random terrain sequences (M11, 11.5) over 2,000 random documents: terrain sets of the three
//! kinds made and changed, terrain strokes of every shape in both corner modes, hand placements
//! that pin, terrain tiles removed, with undo, redo, saves and ordinary edits among them. Every
//! undo and redo gives the document it should, the maps keep their rules, each stroke's cells
//! obey the model's own reading of the rules, and a stroke on a reopened save does the same.

use pikslar_core::ColorMode;
use pikslar_safety::undo_model::autotile::run;
use pikslar_safety::undo_model::{Counts, Plan};

/// Sequences in each colour mode; together they are the 2,000 the plan asks for.
const SEQUENCES: u64 = 1_000;
const STEPS: usize = 80;
/// Seeds clear of the other undo suites'.
const FIRST_SEED: u64 = 3_000_000;

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

/// Floors near half of what the runs made when written, so a change that stops the terrain
/// work from happening fails here.
fn did_real_work(counts: Counts) {
    assert!(counts.terrain_strokes > 2_500, "{counts:?}");
    assert!(counts.terrain_cells > 60_000, "{counts:?}");
    assert!(counts.previews > 1_100, "{counts:?}");
    assert!(counts.terrains > 4_500, "{counts:?}");
    assert!(counts.pins > 25_000, "{counts:?}");
    assert!(counts.removed_tiles > 1_900, "{counts:?}");
    assert!(counts.reloaded_strokes > 2_000, "{counts:?}");
    assert!(counts.saves > 2_000, "{counts:?}");
    assert!(counts.undos > 20_000, "{counts:?}");
}

#[test]
fn palette_terrain_sequences_undo_exactly() {
    did_real_work(check(ColorMode::Palette));
}

#[test]
fn rgba_terrain_sequences_undo_exactly() {
    did_real_work(check(ColorMode::Rgba));
}

/// Replays one failing run: `PIKSLAR_TERRAIN_SEED=n cargo test ... -- --ignored one_seed`.
#[test]
#[ignore = "replays the seed PIKSLAR_TERRAIN_SEED names"]
fn one_seed() {
    let seed = std::env::var("PIKSLAR_TERRAIN_SEED").expect("a seed");
    let seed: u64 = seed.parse().expect("a number");
    for mode in [ColorMode::Palette, ColorMode::Rgba] {
        let counts = run(plan(mode, None), seed).unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        println!("{mode:?} {counts:?}");
    }
}

#[test]
fn a_tiny_history_undoes_terrain_exactly() {
    for seed in FIRST_SEED..FIRST_SEED + 200 {
        for mode in [ColorMode::Palette, ColorMode::Rgba] {
            run(plan(mode, Some(8 * 1024)), seed).unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        }
    }
}
