//! When the system cannot allocate memory the meter allowed, the run ends with a clean
//! `AllocFailed` error at the line that asked for it: no panic, no abort.
//!
//! The allocator here fails every request of at least a threshold, on threads that arm it,
//! so the interpreter's `try_reserve` paths meet a real allocation failure.

mod common;

use common::*;
use pikslar_script::{compile, run, ErrorKind, Limits, Pos};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::AtomicBool;

thread_local! {
    static FAIL_FROM: Cell<usize> = const { Cell::new(usize::MAX) };
}

struct Failing;

fn refused(size: usize) -> bool {
    FAIL_FROM.try_with(|f| size >= f.get()).unwrap_or(false)
}

// SAFETY: requests are passed to the system allocator unchanged, or refused with a null
// pointer, which `GlobalAlloc` allows. The thread local has no destructor and never allocates.
unsafe impl GlobalAlloc for Failing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if refused(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's layout obligations are the ones `System` requires.
        unsafe { System.alloc(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        if refused(new) {
            return std::ptr::null_mut();
        }
        // SAFETY: `ptr` and `layout` came from this allocator, as `System` requires.
        unsafe { System.realloc(ptr, layout, new) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator with this same layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static FAILING: Failing = Failing;

/// Runs `src` with a 64 MiB cap while allocations of 1 MiB or more fail.
fn run_failing(src: &str) -> pikslar_script::RunReport {
    let limits = Limits {
        memory: 64 << 20,
        ..Limits::default()
    };
    let p = compile(src, &table(), &limits).expect("compiles");
    FAIL_FROM.with(|f| f.set(1 << 20));
    let r = run(&p, &mut Canvas::new(), &limits, &AtomicBool::new(false));
    FAIL_FROM.with(|f| f.set(usize::MAX));
    r
}

#[test]
fn failed_allocations_are_clean_script_errors() {
    let cases = [
        ("list growth", "let l = [];\nwhile true { push(l, 1); }", Pos { line: 2, column: 14 }),
        ("big list", "let l = list(200000, 0);", Pos { line: 1, column: 9 }),
        ("big string", "let s = repeat(\"ab\", 1000000);", Pos { line: 1, column: 9 }),
        ("string doubling", "let s = \"x\";\nwhile true { s = s + s; }", Pos { line: 2, column: 20 }),
        ("list slice", "let l = list(60000, 1); let m = slice(l, 0, 60000); let n = [];\nfor i in 0..100000 { push(n, i); }", Pos { line: 2, column: 22 }),
    ];
    for (name, src, pos) in cases {
        let r = run_failing(src);
        let e = r.result.expect_err(name);
        assert_eq!(e.kind, ErrorKind::AllocFailed, "{name}: {e}");
        assert_eq!(e.pos, pos, "{name}: {e}");
        assert!(e.kind.is_stop());
        assert!(r.peak_memory < 64 << 20);
    }
}

#[test]
fn the_meter_is_refunded_after_a_failure() {
    // After a failed allocation the run ends; a new run starts from nothing and succeeds.
    let r = run_failing("let s = repeat(\"ab\", 1000000);");
    assert_eq!(r.result.expect_err("fails").kind, ErrorKind::AllocFailed);
    let ok = run_failing("let s = repeat(\"ab\", 1000); return len(s);");
    assert_eq!(ok.result.ok().and_then(|v| v.as_int()), Some(2000));
}
