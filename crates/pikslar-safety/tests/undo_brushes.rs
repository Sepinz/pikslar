//! Random sequences with brush captures, turns and strokes undo and redo to identical documents,
//! and the same input gives the same bytes every time.

use pikslar_core::ColorMode;
use pikslar_safety::undo_model::{run, run_saved, Counts, Plan};

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
    // Tilemap layers, mixed in since M10, take some strokes the brushes had: 1,473 of them
    // in the palette run at that time, against 1,555 before.
    assert!(counts.brushes > 1400, "{counts:?}");
    assert!(counts.brush_turns > 400, "{counts:?}");
    assert!(counts.brush_strokes > 800, "{counts:?}");
    assert!(counts.undos > 5000, "{counts:?}");
}

#[test]
fn palette_documents_undo_exactly_with_brush_strokes() {
    did_real_work(check(ColorMode::Palette));
}

#[test]
fn rgba_documents_undo_exactly_with_brush_strokes() {
    did_real_work(check(ColorMode::Rgba));
}

#[test]
fn a_tiny_history_undoes_exactly_with_brush_strokes() {
    for seed in 0..200 {
        for mode in [ColorMode::Palette, ColorMode::Rgba] {
            run(plan(mode, Some(8 * 1024)), seed).unwrap_or_else(|e| panic!("{mode:?}: {e}"));
        }
    }
}

#[test]
fn the_same_input_gives_byte_identical_documents() {
    for seed in 0..200 {
        for mode in [ColorMode::Palette, ColorMode::Rgba] {
            let first = run_saved(plan(mode, None), seed).expect("runs");
            let second = run_saved(plan(mode, None), seed).expect("runs again");
            assert!(first.1 == second.1, "{mode:?} seed {seed}: bytes differ");
            assert!(!first.1.is_empty());
        }
    }
}
