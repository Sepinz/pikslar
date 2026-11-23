//! M12 red team: memory a script makes the host allocate outside the meter. The whole test
//! process counts its heap, and each run may raise the peak by at most a small multiple of the
//! run's memory cap. The tests take turns, so nothing else allocates meanwhile.

mod common;

use common::project;
use pikslar_bind::{run_script, Capabilities, Capability, ScriptLimits};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

static TURN: Mutex<()> = Mutex::new(());

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is passed straight to the system allocator; only counters are added.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's layout is handed on unchanged.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = NOW.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `alloc` with this layout.
        unsafe { System.dealloc(ptr, layout) };
        NOW.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// The rise in the heap's peak over a run of `src`, in bytes.
fn peak_rise(src: &str, caps: Capabilities, limits: ScriptLimits) -> usize {
    let mut e = project();
    let _turn = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let base = NOW.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let out = run_script(&mut e, src, caps, limits, &AtomicBool::new(false));
    println!("{src}: {:?}", out.result.as_ref().map(|_| ()));
    PEAK.load(Ordering::Relaxed).saturating_sub(base)
}

/// What a run may make the host hold besides what it meters: the script's own values, the
/// document's copy for the transaction and the run's thread stack are all within this.
fn most(limits: ScriptLimits) -> usize {
    3 * limits.memory + (32 << 20)
}

#[test]
fn a_list_of_one_shared_string_is_not_copied_per_item() {
    let limits = ScriptLimits {
        memory: 4 << 20,
        ..ScriptLimits::default()
    };
    let src = "let s = repeat(\"a\", 1048576); pick_file_to_read(\"t\", list(64, s));";
    let caps = Capabilities::NONE.with(Capability::PickFileToRead);
    let rise = peak_rise(src, caps, limits);
    println!("heap rose {rise} bytes; at most {}", most(limits));
    assert!(rise <= most(limits), "heap rose {rise} bytes");
}

/// Host functions that take strings or lists, given the largest the cap allows.
#[test]
fn host_arguments_are_copied_at_most_once() {
    let limits = ScriptLimits {
        memory: 4 << 20,
        ..ScriptLimits::default()
    };
    let big = "let s = repeat(\"a\", 1048576); let l = layers()[0]; let f = frames()[0];";
    for call in [
        "rename_layer(l, s)",
        "add_layer(s)",
        "add_tag(s, f, f)",
        "set_blend(l, s)",
        "print(s)",
        "write_rect(l, f, 0, 0, 1, list(100000, 4278190335))",
        "write_rect(l, f, 0, 0, 32, list(100000, s))",
        "str(list(100000, s))",
        "find(s, s)",
        "s + s",
        "return s",
    ] {
        let src = format!("{big} {call};");
        let rise = peak_rise(&src, Capabilities::ALL, limits);
        assert!(rise <= most(limits), "{call}: heap rose {rise} bytes");
    }
}
