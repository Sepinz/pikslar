//! Performance regression gate. Run with `cargo run -p pikslar-bench --release`.
//!
//! `check` (the default) exits non-zero if anything regressed. `update` rewrites the
//! baseline. Run on an otherwise idle machine, and use the same machine as the baseline.

mod cpu;
mod creative;
mod creative_blend;
mod creative_gate;
mod creative_palette;
mod creative_terrain;
mod creative_tiles;
mod drag_study;
mod gate;
mod group_study;
mod history_scrub;
mod history_study;
mod image_study;
mod limits;
mod lint_eval;
mod offset_study;
mod process_memory;
mod reference_study;
mod save_study;
mod script_bench;
mod script_cases;
mod script_gate;
mod script_large;
mod session_study;
mod stress;
mod stress_map;
mod stress_map_limits;
mod text_gate;
mod text_study;
mod tile_cpu;
mod tile_pool;
mod tile_profile;
mod tile_steps;
mod tile_threads;
mod workloads;
mod workloads_edit;

use gate::{best_of_runs, format_baseline, parse_baseline, regressions};
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A failing check is repeated this many times before it counts. Each attempt is a new
/// process, because a process tends to stay fast or slow for its whole life.
const CHECK_ATTEMPTS: usize = 4;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// SAFETY: forwards every call unchanged to the system allocator and only adds counters.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller upholds `alloc`'s contract, which we pass on.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: `ptr` came from this allocator with this layout, per the caller's contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller upholds `realloc`'s contract, which we pass on.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

pub fn live_bytes() -> usize {
    LIVE.load(Ordering::Relaxed)
}

pub fn total_allocs() -> usize {
    ALLOCS.load(Ordering::Relaxed)
}

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../perf/baseline.txt")
}

fn measure_in_new_process(mode: &str) -> Option<BTreeMap<String, f64>> {
    let exe = std::env::current_exe().ok()?;
    let output = Command::new(exe).arg(mode).output().ok()?;
    output
        .status
        .success()
        .then(|| parse_baseline(&String::from_utf8_lossy(&output.stdout)))
}

fn run_lint_eval() -> ExitCode {
    let (Some(source), Some(prefix)) = (std::env::args().nth(2), std::env::args().nth(3)) else {
        eprintln!("Usage: pikslar-bench lint-eval <png or folder> <output prefix>");
        return ExitCode::from(2);
    };
    match lint_eval::run(std::path::Path::new(&source), &prefix) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run_make(document: fn() -> pikslar_core::Document) -> ExitCode {
    let Some(path) = std::env::args().nth(2) else {
        eprintln!("Usage: pikslar-bench make-stress|make-large|make-stress-map <output.piks>");
        return ExitCode::from(2);
    };
    match stress::write_project(&document(), std::path::Path::new(&path)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn judge_limits(measured: &str, limits_path: &str) -> Result<u8, String> {
    let read = |path: &str| std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"));
    let measured = parse_baseline(&read(measured)?);
    let limits = limits::parse_limits(&read(limits_path)?)?;
    let outcomes = limits::evaluate(&limits, &measured);
    outcomes
        .iter()
        .for_each(|o| println!("{}", limits::report_line(o)));
    let proved = limits::prove_each_limit_fails(&limits, &measured)?;
    println!("{proved} hard limits each shown to fail the gate when exceeded.");
    Ok(limits::exit_code(&outcomes))
}

fn run_limits() -> ExitCode {
    let (Some(measured), Some(limits)) = (std::env::args().nth(2), std::env::args().nth(3)) else {
        eprintln!("Usage: pikslar-bench limits <measured.txt> <limits.txt>");
        return ExitCode::from(2);
    };
    match judge_limits(&measured, &limits) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn run_stress_map_limits() -> ExitCode {
    let Some(path) = std::env::args().nth(2) else {
        eprintln!("Usage: pikslar-bench stress-map-limits <file.piks> [hold seconds]");
        return ExitCode::from(2);
    };
    let hold = std::env::args().nth(3).and_then(|s| s.parse().ok());
    match stress_map_limits::run(&path, hold.unwrap_or(0)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// The modes that measure nothing in this process.
fn run_tool(mode: &str) -> Option<ExitCode> {
    match mode {
        "lint-eval" => Some(run_lint_eval()),
        "limits" => Some(run_limits()),
        "make-stress" => Some(run_make(stress::stress_document)),
        "make-large" => Some(run_make(stress::large_document)),
        "make-stress-map" => Some(run_make(stress_map::stress_map_document)),
        "stress-map-limits" => Some(run_stress_map_limits()),
        _ => None,
    }
}

/// The creative tools' group: `creative-measure`, `creative-check` and `creative-update`, which
/// has its own baseline file and only ever appends to it.
fn run_creative(mode: &str) -> ExitCode {
    let metrics = creative::measure();
    for m in &metrics {
        println!("{:<34} {:>14.1}  {:?}", m.name, m.value, m.kind);
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../perf/creative-baseline.txt");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    match mode {
        "creative-measure" => ExitCode::SUCCESS,
        "creative-update" => {
            // The best of three processes, each a new one, as the file's header says.
            let best = (1..3).fold(metrics, |best, _| {
                match measure_in_new_process("creative-measure") {
                    Some(again) => best_of_runs(&best, &again),
                    None => best,
                }
            });
            append_missing(&path, &existing, &best)
        }
        _ => judge_creative(metrics, &existing),
    }
}

fn append_missing(path: &std::path::Path, existing: &str, metrics: &[gate::Metric]) -> ExitCode {
    use std::io::Write;
    let text = creative_gate::lines_to_append(existing, metrics);
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()));
    match written {
        Ok(()) => {
            println!(
                "{} lines appended.",
                text.lines().filter(|l| !l.starts_with('#')).count()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Could not append to {}: {e}", path.display());
            ExitCode::FAILURE
        }
    }
}

fn judge_creative(mut metrics: Vec<gate::Metric>, existing: &str) -> ExitCode {
    if existing.is_empty() {
        eprintln!("No creative baseline yet (run creative-update).");
        return ExitCode::FAILURE;
    }
    let baseline = parse_baseline(existing);
    for _ in 1..CHECK_ATTEMPTS {
        if regressions(&metrics, &baseline).is_empty() {
            break;
        }
        let Some(again) = measure_in_new_process("creative-measure") else {
            break;
        };
        metrics = best_of_runs(&metrics, &again);
    }
    let failures = regressions(&metrics, &baseline);
    if failures.is_empty() {
        println!("No regressions.");
        return ExitCode::SUCCESS;
    }
    failures.iter().for_each(|f| eprintln!("REGRESSION  {f}"));
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    if cfg!(debug_assertions) {
        eprintln!("Timing a debug build means nothing. Add --release.");
        return ExitCode::from(2);
    }
    let mode = std::env::args().nth(1).unwrap_or_else(|| "check".into());
    if let Some(code) = run_tool(&mode) {
        return code;
    }
    if mode.starts_with("creative-") {
        return run_creative(&mode);
    }
    if matches!(
        mode.as_str(),
        "script-measure" | "script-check" | "script-update"
    ) {
        return script_gate::run(&mode);
    }
    if matches!(mode.as_str(), "text-measure" | "text-check" | "text-update") {
        return text_gate::run(&mode);
    }
    if mode == "text-case" {
        return text_gate::run_case();
    }
    if mode == "script" {
        return exit(script_bench::run_all());
    }
    if mode == "script-case" {
        let name = std::env::args().nth(2).unwrap_or_default();
        return exit(script_bench::run_one(&name));
    }
    if mode == "tile-threads" {
        tile_threads::run();
        return ExitCode::SUCCESS;
    }
    if mode == "tile-cpu" {
        tile_cpu::run();
        return ExitCode::SUCCESS;
    }
    if mode == "tile-pool" {
        tile_pool::run();
        return ExitCode::SUCCESS;
    }
    if mode == "tile-profile" {
        tile_profile::run();
        return ExitCode::SUCCESS;
    }
    if mode == "save-study" {
        let folder = std::env::args().nth(2).map(PathBuf::from);
        return match save_study::run(folder) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    if mode == "image-study" {
        image_study::run();
        return ExitCode::SUCCESS;
    }
    if mode == "history-scrub" {
        history_scrub::run();
        return ExitCode::SUCCESS;
    }
    if mode == "history-study" {
        history_study::run();
        return ExitCode::SUCCESS;
    }
    if mode == "group-study" {
        group_study::run();
        return ExitCode::SUCCESS;
    }
    if mode == "reference-study" {
        reference_study::run();
        return ExitCode::SUCCESS;
    }
    if mode == "offset-study" {
        offset_study::run();
        return ExitCode::SUCCESS;
    }
    if mode == "text-study" {
        text_study::run();
        return ExitCode::SUCCESS;
    }
    if mode == "drag-study" {
        let options: Vec<String> = std::env::args().skip(2).collect();
        let has = |word: &str| options.iter().any(|o| o == word);
        drag_study::run(!has("tiles"), has("scale"));
        return ExitCode::SUCCESS;
    }
    let metrics = workloads::measure();
    for m in &metrics {
        println!("{:<34} {:>14.1}  {:?}", m.name, m.value, m.kind);
    }
    if mode == "measure" {
        return ExitCode::SUCCESS;
    }
    let path = baseline_path();
    if mode == "update" {
        return match std::fs::write(&path, format_baseline(&metrics)) {
            Ok(()) => {
                println!("Baseline updated.");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Could not write {}: {e}", path.display());
                ExitCode::FAILURE
            }
        };
    }
    let baseline = match std::fs::read_to_string(&path) {
        Ok(text) => parse_baseline(&text),
        Err(e) => {
            eprintln!("Could not read {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let mut metrics = metrics;
    for _ in 1..CHECK_ATTEMPTS {
        if regressions(&metrics, &baseline).is_empty() {
            break;
        }
        let Some(again) = measure_in_new_process("measure") else {
            break;
        };
        metrics = best_of_runs(&metrics, &again);
    }
    let failures = regressions(&metrics, &baseline);
    if failures.is_empty() {
        println!("No regressions.");
        return ExitCode::SUCCESS;
    }
    failures.iter().for_each(|f| eprintln!("REGRESSION  {f}"));
    ExitCode::FAILURE
}

fn exit(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
