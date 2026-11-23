//! The memory cap is hard: under hostile scripts, the heap this thread really holds never
//! passes the cap plus a small fixed overhead, and the meter never passes the cap.
//!
//! A counting allocator records, per thread, the bytes live and their peak. A reallocation
//! counts the old and new blocks together, as the heap holds both while it copies.

mod common;

use common::*;
use pikslar_script::{compile, run, ErrorKind, Limits, RunReport};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::AtomicBool;

thread_local! {
    static LIVE: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
}

fn add(n: usize) {
    let _ = LIVE.try_with(|l| {
        let now = l.get() + n;
        l.set(now);
        let _ = PEAK.try_with(|p| p.set(p.get().max(now)));
    });
}

fn sub(n: usize) {
    let _ = LIVE.try_with(|l| l.set(l.get().saturating_sub(n)));
}

struct Tracking;

// SAFETY: every call is passed to the system allocator unchanged. The counters are const
// initialised thread locals without destructors, so touching them never allocates.
unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        add(layout.size());
        // SAFETY: the caller's layout obligations are the ones `System` requires.
        unsafe { System.alloc(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        add(new);
        // SAFETY: `ptr` and `layout` came from this allocator, as `System` requires.
        let out = unsafe { System.realloc(ptr, layout, new) };
        sub(layout.size());
        out
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        sub(layout.size());
        // SAFETY: `ptr` came from this allocator with this same layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static TRACKING: Tracking = Tracking;

const CAP: usize = 4 << 20;
/// Heap the run may hold outside the meter: the meter itself, the run's bookkeeping and the
/// error it returns. Measured at under 1 KiB; this bound leaves room.
const FIXED: usize = 16 << 10;

struct Measured {
    report: RunReport,
    heap_peak: usize,
}

/// Compiles and runs `src` under the cap, measuring this thread's heap from before compiling.
fn measure(src: &str) -> Measured {
    let limits = Limits {
        memory: CAP,
        fuel: 200_000_000,
        ..Limits::default()
    };
    let start = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(start));
    let mut canvas = Canvas::new();
    let table = table();
    // The canvas and table are the host's, not the script's.
    let base = LIVE.with(Cell::get) - start;
    let report = match compile(src, &table, &limits) {
        Ok(p) => run(&p, &mut canvas, &limits, &AtomicBool::new(false)),
        Err(e) => panic!("compile failed: {e}"),
    };
    let heap_peak = PEAK.with(Cell::get) - start - base;
    Measured { report, heap_peak }
}

fn check_stopped_within_cap(name: &str, src: &str, want: ErrorKind) -> Measured {
    let m = measure(src);
    let err = m.report.result.as_ref().map(|_| ()).expect_err(name);
    assert_eq!(err.kind, want, "{name}: {err}");
    assert!(
        m.report.peak_memory <= CAP,
        "{name}: meter {}",
        m.report.peak_memory
    );
    assert!(
        m.heap_peak <= CAP + FIXED,
        "{name}: heap peak {} over {CAP} + {FIXED}",
        m.heap_peak
    );
    println!(
        "{name}: stopped by {:?}, meter peak {} B, heap peak {} B ({:.3} of the cap)",
        err.kind,
        m.report.peak_memory,
        m.heap_peak,
        m.heap_peak as f64 / CAP as f64
    );
    m
}

#[test]
fn list_growth_stops_at_the_cap() {
    check_stopped_within_cap(
        "list growth",
        "let l = []; while true { push(l, 1); }",
        ErrorKind::Memory,
    );
}

#[test]
fn string_doubling_stops_at_the_cap() {
    check_stopped_within_cap(
        "string doubling",
        "let s = \"x\"; while true { s = s + s; }",
        ErrorKind::Memory,
    );
}

#[test]
fn many_small_values_stop_at_the_cap() {
    let src = "let l = []; let i = 0; while true { push(l, str(i)); i = i + 1; }";
    check_stopped_within_cap("many small strings", src, ErrorKind::Memory);
    let src = "let ls = []; while true { let l = [1, 2]; push(ls, str(l)); }";
    check_stopped_within_cap("many short lists rendered", src, ErrorKind::Memory);
}

#[test]
fn memory_spread_over_call_frames_stops_at_the_cap() {
    let src = "fn f(n) { let l = list(20000, n); return f(n + 1) + len(l); } f(0);";
    let m = on_script_thread(move || {
        let m = measure(src);
        (
            m.report.result.map_err(|e| e.kind).err(),
            m.report.peak_memory,
            m.heap_peak,
        )
    });
    assert_eq!(m.0, Some(ErrorKind::Memory));
    assert!(
        m.1 <= CAP && m.2 <= CAP + FIXED,
        "meter {} heap {}",
        m.1,
        m.2
    );
    println!(
        "frames each holding memory: meter peak {} B, heap peak {} B",
        m.1, m.2
    );
}

#[test]
fn huge_single_allocations_are_refused_before_allocating() {
    for (name, src) in [
        ("huge repeat", "let s = repeat(\"ab\", 1000000000000);"),
        ("huge list", "let l = list(1000000000000, 0);"),
        (
            "overflowing repeat",
            "let s = repeat(\"ab\", 9223372036854775807);",
        ),
        ("just over the cap", "let s = repeat(\"x\", 4194304);"),
    ] {
        let m = check_stopped_within_cap(name, src, ErrorKind::Memory);
        assert!(
            m.heap_peak < 64 << 10,
            "{name}: allocated {} before refusing",
            m.heap_peak
        );
    }
}

#[test]
fn churn_refunds_and_runs_to_completion() {
    // Each turn drops the last string, so live memory stays flat however long it runs.
    let src = "let b = repeat(\"x\", 1000); let i = 0;
               while i < 200 { let s = repeat(b, 1000); i = i + 1; } return i;";
    let m = measure(src);
    assert!(m.report.result.is_ok(), "{:?}", m.report.result);
    assert!(m.report.peak_memory < 3 << 20);
    assert!(m.heap_peak <= CAP + FIXED);
    println!(
        "churn: meter peak {} B, heap peak {} B",
        m.report.peak_memory, m.heap_peak
    );
}

#[test]
fn the_parse_tree_is_charged_and_bounded() {
    // Long scripts of shapes that waste the most: one-item vectors, boxed calls, short blocks.
    for line in [
        "if true { let a = [1, 2, 3]; a[0] = a[1] + a[2] * 4 - len(\"some text\"); }\n",
        "if true { 1; }\n",
        "[1];\n",
        "f(1);\n",
        "while false { break; }\n",
        "\"s\";\n",
    ] {
        tree_within_charge(&format!("fn f(x) {{ }}\n{}", line.repeat(400)));
    }
}

fn tree_within_charge(src: &str) {
    let limits = Limits::default();
    let start = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(start));
    let table = table();
    let after_table = LIVE.with(Cell::get);
    let p = compile(src, &table, &limits).expect("compiles");
    let live = LIVE.with(Cell::get) - after_table;
    let peak = PEAK.with(Cell::get) - after_table;
    assert!(
        live <= p.memory(),
        "tree holds {live} B, charged {}",
        p.memory()
    );
    assert!(
        peak <= p.memory(),
        "compiling peaked at {peak} B, charged {}",
        p.memory()
    );
    println!(
        "parse tree: {live} B live, {peak} B peak, {} B charged",
        p.memory()
    );
    let small = Limits {
        memory: p.memory() - 1,
        ..limits
    };
    let e = compile(src, &table, &small).expect_err("over the cap");
    assert_eq!(e.kind, ErrorKind::TooLarge);
}

#[test]
fn source_and_nesting_are_bounded() {
    let limits = Limits {
        source_bytes: 100,
        ..Limits::default()
    };
    let e = compile(&"1;".repeat(51), &table(), &limits).expect_err("too long");
    assert_eq!(e.kind, ErrorKind::TooLarge);
    for src in [
        format!("let x = {}1{};", "(".repeat(100), ")".repeat(100)),
        format!("let x = 1{};", " + 1".repeat(100)),
        format!("let x = {}true;", "!".repeat(100)),
        format!("{}{}", "if true { ".repeat(100), "}".repeat(100)),
        format!("let l = [0]; let x = l{};", "[0]".repeat(100)),
    ] {
        let e = compile(&src, &table(), &Limits::default()).expect_err("too deep");
        assert_eq!(e.kind, ErrorKind::TooLarge, "{e}");
    }
}

#[test]
fn a_program_larger_than_the_cap_cannot_run() {
    let p = compile("let x = 1;", &table(), &Limits::default()).expect("compiles");
    let tiny = Limits {
        memory: p.memory() - 1,
        ..Limits::default()
    };
    let r = run(&p, &mut Canvas::new(), &tiny, &AtomicBool::new(false));
    assert_eq!(r.result.expect_err("over").kind, ErrorKind::Memory);
}
