//! Counts what the current thread allocates, so a parser can be held to a memory limit.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static NOW: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: every call is passed to the system allocator unchanged. The counters are const
// initialised thread locals without destructors, so touching them never allocates. `try_with`
// keeps a thread that is shutting down from panicking inside the allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = NOW.try_with(|now| {
            let current = now.get() + layout.size();
            now.set(current);
            let _ = PEAK.try_with(|peak| peak.set(peak.get().max(current)));
            let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(layout.size())));
        });
        // SAFETY: the caller's layout obligations are the same ones `System` requires.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = NOW.try_with(|now| now.set(now.get().saturating_sub(layout.size())));
        // SAFETY: `ptr` came from `alloc` above with this same layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// What a piece of work needed, beyond what the thread already held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub peak: usize,
    pub largest: usize,
}

/// Runs `work` and reports its memory use on this thread.
pub fn measure<R>(work: impl FnOnce() -> R) -> (R, Usage) {
    let base = NOW.with(Cell::get);
    PEAK.with(|peak| peak.set(base));
    LARGEST.with(|largest| largest.set(0));
    let result = work();
    let usage = Usage {
        peak: PEAK.with(Cell::get).saturating_sub(base),
        largest: LARGEST.with(Cell::get),
    };
    (result, usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_allocation_is_seen() {
        let (bytes, usage) = measure(|| vec![0u8; 1 << 20]);
        assert_eq!(bytes.len(), 1 << 20);
        assert!(usage.largest >= 1 << 20);
        assert!(usage.peak >= 1 << 20);
    }

    #[test]
    fn freed_memory_does_not_count_twice() {
        let (_, usage) = measure(|| {
            for _ in 0..8 {
                drop(vec![0u8; 1 << 16]);
            }
        });
        assert!(usage.peak < 1 << 18);
    }
}
