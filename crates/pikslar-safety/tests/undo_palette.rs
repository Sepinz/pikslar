//! Random sequences with palette workflows (reorder, sort, groups, ramps, the three remaps,
//! Replace colour and Load) among every other edit, over 2,000 random documents. Every remap
//! keeps every composited frame identical, undo and redo give identical documents, and saving
//! and opening again in the middle of a run loses nothing.

use pikslar_core::ColorMode;
use pikslar_safety::undo_model::{run, Counts, Plan};

/// Sequences in each colour mode; together they are the 2,000 the plan asks for.
const SEQUENCES: u64 = 1_000;
const STEPS: usize = 80;

fn plan(mode: ColorMode, budget: Option<usize>) -> Plan {
    Plan {
        mode,
        steps: STEPS,
        budget,
    }
}

fn check(mode: ColorMode) -> Counts {
    let mut total = Counts::default();
    for seed in 0..SEQUENCES {
        let counts = run(plan(mode, None), seed).unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        total.merge(&counts);
    }
    total
}

fn did_real_work(counts: Counts) {
    // Tilemap actions, mixed in since M10, change the runs: 1,911 palette steps in the RGBA
    // run at that time.
    assert!(counts.palette > 1800, "{counts:?}");
    assert!(counts.remaps > 1200, "{counts:?}");
    assert!(counts.loads > 500, "{counts:?}");
    assert!(counts.saves > 400, "{counts:?}");
    assert!(counts.map_strokes > 250, "{counts:?}");
    assert!(counts.undos > 5000, "{counts:?}");
}

#[test]
fn palette_documents_undo_exactly_with_palette_workflows() {
    did_real_work(check(ColorMode::Palette));
}

#[test]
fn rgba_documents_undo_exactly_with_palette_workflows() {
    did_real_work(check(ColorMode::Rgba));
}

#[test]
fn a_tiny_history_undoes_exactly_with_palette_workflows() {
    for seed in 0..200 {
        for mode in [ColorMode::Palette, ColorMode::Rgba] {
            run(plan(mode, Some(8 * 1024)), seed).unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        }
    }
}
