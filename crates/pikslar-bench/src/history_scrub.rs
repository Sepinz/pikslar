//! `pikslar-bench history-scrub [rounds]`: the gate's `history_undo_redo_all_2000` alone, taken
//! apart. The same 2,000 strokes are undone and redone, and it reports the best time of the whole
//! round trip, of undoing alone and of redoing alone, with what each allocates. Outside the
//! gate's baseline: it is for finding where that line's time goes.

use crate::total_allocs;
use crate::workloads::{document, draw_history_strokes};
use pikslar_core::{Editor, StepCmd};
use std::time::Instant;

const DEFAULT_ROUNDS: usize = 200;

/// Best time and allocations of one scrub.
struct Best {
    micros: f64,
    allocs: usize,
}

fn scrub(editor: &mut Editor, depth: usize) -> (f64, usize) {
    let allocs = total_allocs();
    let start = Instant::now();
    editor
        .apply(StepCmd::ScrubTo { depth })
        .expect("scrubbing within history");
    let micros = start.elapsed().as_secs_f64() * 1e6;
    (micros, total_allocs() - allocs)
}

fn keep(best: &mut Best, (micros, allocs): (f64, usize)) {
    best.micros = best.micros.min(micros);
    best.allocs = best.allocs.max(allocs);
}

pub(crate) fn run() {
    let rounds = std::env::args()
        .nth(2)
        .and_then(|n| n.parse().ok())
        .unwrap_or(DEFAULT_ROUNDS);
    let mut editor = Editor::new(document(512));
    draw_history_strokes(&mut editor);
    let depth = editor.history().undo_depth();
    let fresh = || Best {
        micros: f64::MAX,
        allocs: 0,
    };
    let (mut both, mut undo, mut redo) = (fresh(), fresh(), fresh());
    for _ in 0..rounds {
        let down = scrub(&mut editor, 0);
        let up = scrub(&mut editor, depth);
        keep(&mut undo, down);
        keep(&mut redo, up);
        keep(&mut both, (down.0 + up.0, down.1 + up.1));
    }
    let steps = depth as f64;
    let lines = [
        ("undo_redo_all", both, 2.0 * steps),
        ("undo_all", undo, steps),
        ("redo_all", redo, steps),
    ];
    for (name, best, played) in lines {
        println!(
            "{name:<16} {:>9.1} us {:>7.2} ns/step {:>6} allocs (most in a round)",
            best.micros,
            best.micros * 1e3 / played,
            best.allocs,
        );
    }
}
