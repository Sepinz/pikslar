//! Random edits saved with Rewind on, reopened and undone through the store, against the model.

use pikslar_core::ColorMode;
use pikslar_rewind::Budgets;
use pikslar_safety::undo_model::rewind::run_rewind;
use pikslar_safety::undo_model::{Counts, Plan};
use std::path::PathBuf;

const SEQUENCES: u64 = 60;
const STEPS: usize = 50;

fn check(mode: ColorMode, budget: Option<usize>, budgets: Budgets, name: &str) -> Counts {
    let plan = Plan {
        mode,
        steps: STEPS,
        budget,
    };
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("undo-rewind")
        .join(name);
    let mut total = Counts::default();
    for seed in 0..SEQUENCES {
        let dir = root.join(seed.to_string());
        let counts = run_rewind(plan, seed, &dir, budgets)
            .unwrap_or_else(|e| panic!("{mode:?} {budget:?}: {e}"));
        total.merge(&counts);
    }
    total
}

#[test]
fn palette_documents_undo_exactly_after_reopening() {
    let counts = check(ColorMode::Palette, None, Budgets::default(), "palette");
    assert!(counts.steps > 1000, "{counts:?}");
}

#[test]
fn rgba_documents_undo_exactly_after_reopening() {
    let counts = check(ColorMode::Rgba, None, Budgets::default(), "rgba");
    assert!(counts.undos > 500, "{counts:?}");
}

#[test]
fn small_budgets_keep_the_newest_steps_exactly() {
    let small = Budgets {
        project: 16 * 1024,
        keep: 8 * 1024,
        store: 1 << 30,
    };
    check(ColorMode::Rgba, Some(4096), small, "small");
}
