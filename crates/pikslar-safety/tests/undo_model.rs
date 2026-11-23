//! A short version of the long undo run, with every debug check on.

use pikslar_core::ColorMode;
use pikslar_safety::undo_model::{run, Counts, Plan};

const SEQUENCES: u64 = 200;
const STEPS: usize = 60;

fn check(mode: ColorMode, budget: Option<usize>) -> Counts {
    let plan = Plan {
        mode,
        steps: STEPS,
        budget,
    };
    let mut total = Counts::default();
    for seed in 0..SEQUENCES {
        let counts = run(plan, seed).unwrap_or_else(|e| panic!("{mode:?} {budget:?}: {e}"));
        total.merge(&counts);
    }
    total
}

fn did_real_work(counts: Counts) {
    assert!(counts.steps > 1000, "{counts:?}");
    assert!(counts.undos > 1000, "{counts:?}");
    assert!(counts.redos > 500, "{counts:?}");
    // Floating pixels are one action in 41 since palette steps joined the mix.
    assert!(counts.floating > 110, "{counts:?}");
    assert!(counts.refused > 200, "{counts:?}");
    // Tilemap actions, mixed in since M10, change the runs: 194 slice steps at that time.
    assert!(counts.slices > 180, "{counts:?}");
    assert!(counts.slices_refused > 100, "{counts:?}");
    assert!(counts.tweens > 20, "{counts:?}");
    // Four scrubs and the last back to the end, in each run that kept steps.
    assert!(counts.scrubs > 500, "{counts:?}");
    assert!(counts.scrubbed_steps > 5000, "{counts:?}");
    assert!(counts.blendings > 50, "{counts:?}");
}

#[test]
fn palette_documents_undo_exactly() {
    did_real_work(check(ColorMode::Palette, None));
}

#[test]
fn rgba_documents_undo_exactly() {
    did_real_work(check(ColorMode::Rgba, None));
}

#[test]
fn a_tiny_history_still_undoes_exactly() {
    let mut total = check(ColorMode::Palette, Some(32 * 1024));
    total.merge(&check(ColorMode::Rgba, Some(1024)));
    assert!(total.dropped > 0, "the budget was never reached: {total:?}");
}
