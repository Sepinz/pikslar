//! Scripts measured through the program's own binding (`pikslar-bind`), each case in a new
//! process: `pikslar-bench script` runs them all, `pikslar-bench script-case <name>` one.
//! `script-check` holds the lines to `perf/script-baseline.txt` and `perf/script-limits.txt`
//! (`script_gate`).
//!
//! Repeatable by construction: one process per case, fixed documents built from fixed seeds,
//! the default limits (`ScriptLimits::DEFAULT`, never the user's settings), an untimed warm-up
//! run before timed ones, and the median of several timed runs.

use crate::process_memory::private;
use crate::script_cases::{case, CASES};
use pikslar_bind::{run_script, Capabilities, Outcome, ScriptLimits};
use pikslar_core::Editor;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Runs every case in a process of its own and prints what each reported.
pub fn run_all() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut ok = true;
    for (name, _) in CASES {
        match Command::new(&exe).args(["script-case", name]).output() {
            Ok(out) if out.status.success() => print!("{}", String::from_utf8_lossy(&out.stdout)),
            Ok(out) => {
                ok = false;
                eprintln!("{name} failed: {}", String::from_utf8_lossy(&out.stderr));
            }
            Err(e) => {
                ok = false;
                eprintln!("{name} could not start: {e}");
            }
        }
    }
    ok
}

/// One case, in this process.
pub fn run_one(name: &str) -> bool {
    match case(name) {
        Some(run) => {
            run(name);
            true
        }
        None => {
            eprintln!("no script case {name:?}");
            false
        }
    }
}

/// What one timed run used.
pub struct Measured {
    pub ms: f64,
    pub out: Outcome,
    /// Private bytes over those just before the run: at the process's peak, and after it.
    pub peak_kb: f64,
    pub after_kb: f64,
}

/// One run, with this process's private bytes sampled every millisecond while it runs: the
/// process's own peak counts building the document too.
pub fn measure(editor: &mut Editor, source: &str, limits: ScriptLimits) -> Measured {
    let before = private();
    let done = AtomicBool::new(false);
    let highest = AtomicUsize::new(before);
    let t = Instant::now();
    let out = std::thread::scope(|s| {
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                highest.fetch_max(private(), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        let out = run_script(
            editor,
            source,
            Capabilities::ALL,
            limits,
            &AtomicBool::new(false),
        );
        done.store(true, Ordering::Relaxed);
        out
    });
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let kb = |bytes: usize| bytes.saturating_sub(before) as f64 / 1024.0;
    Measured {
        ms,
        out,
        peak_kb: kb(highest.load(Ordering::Relaxed)),
        after_kb: kb(private()),
    }
}

/// Timed runs of a hostile case: each is long, so fewer are taken.
pub const HOSTILE_REPS: usize = 3;

/// The median of `reps` runs' times after one untimed warm-up run, and the last run's
/// outcome. A run that fails leaves the document as it was, so every run starts alike.
pub fn median_ms(editor: &mut Editor, source: &str, reps: usize) -> (f64, Outcome) {
    let _warm = measure(editor, source, ScriptLimits::DEFAULT);
    timed_median(editor, source, reps, Vec::new())
}

/// The median of `times` and `reps` more timed runs.
pub fn timed_median(
    editor: &mut Editor,
    source: &str,
    reps: usize,
    mut times: Vec<f64>,
) -> (f64, Outcome) {
    let mut last = None;
    for _ in 0..reps.max(1) {
        let m = measure(editor, source, ScriptLimits::DEFAULT);
        times.push(m.ms);
        last = Some(m.out);
    }
    times.sort_by(f64::total_cmp);
    let out = last.unwrap_or_else(|| unreachable!("at least one run"));
    (times[times.len() / 2], out)
}

/// The most private memory a run on `doc` may add, in KB: the script's memory, plus twice
/// the larger of its largest cel (a copy of the cel and a canvas-sized working buffer) and
/// what the document holds (the run's copy of a document whose structure, cel entries and
/// color table, outweighs any one cel).
pub fn peak_ceiling_kb(doc: &pikslar_core::Document) -> f64 {
    let cel = doc.width.saturating_mul(doc.height).saturating_mul(4);
    let most = ScriptLimits::DEFAULT
        .memory
        .saturating_add(cel.max(doc.held_bytes()).saturating_mul(2));
    (most / 1024) as f64
}

/// One report line: the value, then how the run ended and what it used.
pub fn report(name: &str, what: &str, value: f64, out: &Outcome) {
    let how = match &out.result {
        Ok(_) => "completed".to_string(),
        Err(f) => f
            .kind()
            .map_or_else(|| format!("refused ({f})"), |k| format!("{k:?}")),
    };
    println!(
        "script_{name}_{what} {value:.3}  # {how}, fuel {}, script memory peak {} bytes",
        out.fuel_used, out.peak_memory
    );
}
