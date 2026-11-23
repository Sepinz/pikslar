//! Damaged brush libraries: valid files are mutated at random, by a small generator that is the
//! same on every run, and every one must be read or refused without a panic, in modest memory
//! and time, and what is read must hold to the library's limits and write back out. The normal
//! run does 3,000; the ignored test does as many as `PIKSLAR_BRUSH_MUTATIONS` says (100,000 by
//! default), for release runs.

use pikslar_format::brush_lib::{
    read_library, write_library, Handle, LibraryBrush, Paint, MAX_BRUSHES, MAX_NAME_BYTES,
    MAX_PIXEL_BYTES, MAX_SIDE,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Counts the bytes in use and the most there have been since the last reset.
struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grew(by: usize) {
    let now = CURRENT.fetch_add(by, Ordering::Relaxed) + by;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc_zeroed(layout);
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
        System.dealloc(ptr, layout);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        let p = System.realloc(ptr, layout, new);
        if !p.is_null() {
            CURRENT.fetch_sub(layout.size(), Ordering::Relaxed);
            grew(new);
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The most one read may use beyond what the test itself holds.
const MEMORY_LIMIT: usize = 32 << 20;
/// The longest one may take, even in a build with no optimising.
const TIME_LIMIT: Duration = Duration::from_secs(2);

/// xorshift64*: the same numbers for the same seed, on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() >> 11) as usize % n.max(1)
    }
}

const BIG: [u32; 8] = [
    0,
    1,
    0xFFFF,
    0x100,
    0x7FFF_FFFF,
    0x8000_0000,
    0xFFFF_FFFF,
    0x0100_0000,
];

fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    for (slot, v) in bytes.iter_mut().skip(at).zip(value) {
        *slot = *v;
    }
}

fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut b = seed.to_vec();
    for _ in 0..1 + rng.below(4) {
        let at = rng.below(b.len());
        match rng.below(9) {
            0 => b[at] ^= 1 << rng.below(8),
            1 => b.truncate(at),
            2 => (0..1 + rng.below(8)).for_each(|i| put(&mut b, at + i, &[rng.next() as u8])),
            3 => put(&mut b, at, &BIG[rng.below(BIG.len())].to_le_bytes()),
            4 => put(
                &mut b,
                at,
                &(BIG[rng.below(BIG.len())] as u16).to_le_bytes(),
            ),
            5 => drop(b.drain(at..(at + 1 + rng.below(40)).min(b.len()))),
            6 => {
                let junk: Vec<u8> = (0..1 + rng.below(16)).map(|_| rng.next() as u8).collect();
                b.splice(at..at, junk);
            }
            7 => {
                let end = (at + 1 + rng.below(60)).min(b.len());
                let copy = b[at..end].to_vec();
                b.splice(at..at, copy);
            }
            _ => put(&mut b, at, &(rng.next() as u32).to_le_bytes()),
        }
        if b.is_empty() {
            b.push(0);
        }
    }
    b
}

fn brush(name: &str, w: u16, h: u16, noisy: bool, rng: &mut Rng) -> LibraryBrush {
    let n = w as usize * h as usize * 4;
    let rgba = if noisy {
        (0..n).map(|_| rng.next() as u8).collect()
    } else {
        [200, 100, 50, 255].repeat(n / 4)
    };
    LibraryBrush {
        name: name.to_owned(),
        width: w,
        height: h,
        paint: if noisy {
            Paint::Shape
        } else {
            Paint::OwnColors
        },
        orientation: (w % 8) as u8,
        handle: if noisy {
            Handle::Corner(2)
        } else {
            Handle::Centre
        },
        rgba,
    }
}

fn corpus() -> Vec<Vec<u8>> {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let small = vec![
        brush("flat", 6, 6, false, &mut rng),
        brush("noise", 5, 4, true, &mut rng),
        brush("caf\u{e9}", 1, 1, false, &mut rng),
    ];
    let big = vec![
        brush("square", 64, 64, false, &mut rng),
        brush("static", 40, 30, true, &mut rng),
    ];
    let many: Vec<LibraryBrush> = (0..MAX_BRUSHES)
        .map(|i| brush(&format!("b{i}"), 2, 2, i % 2 == 0, &mut rng))
        .collect();
    [small, big, many]
        .iter()
        .map(|b| write_library(b).expect("the seeds are valid"))
        .collect()
}

/// What was read must keep to every limit, and must write out again.
fn holds_to_limits(brushes: &[LibraryBrush], n: u64) {
    assert!(brushes.len() <= MAX_BRUSHES, "mutation {n}: count");
    let pixels: usize = brushes.iter().map(|b| b.rgba.len()).sum();
    assert!(pixels <= MAX_PIXEL_BYTES, "mutation {n}: pixels");
    for b in brushes {
        assert!(b.name.len() <= MAX_NAME_BYTES, "mutation {n}: name");
        assert!(b.width <= MAX_SIDE && b.height <= MAX_SIDE, "mutation {n}");
        assert!(
            b.check().is_ok(),
            "mutation {n}: a read brush fails its check"
        );
    }
    let saved = write_library(brushes)
        .unwrap_or_else(|e| panic!("mutation {n}: what was read cannot be written: {e}"));
    let again = read_library(&saved).unwrap_or_else(|e| panic!("mutation {n}: {e}"));
    assert_eq!(again.brushes, brushes, "mutation {n}: a save is not stable");
    assert!(again.report.is_clean(), "mutation {n}");
}

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn run(count: u64) {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let seeds = corpus();
    let (mut whole, mut partial, mut refused) = (0u64, 0u64, 0u64);
    let (mut slowest, mut most) = (Duration::ZERO, 0usize);
    for n in 0..count {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ n.wrapping_mul(0xd6e8_feb8_6659_fd93) | 1);
        let seed = &seeds[rng.below(seeds.len())];
        let file = mutate(&mut rng, seed);
        let base = CURRENT.load(Ordering::Relaxed);
        PEAK.store(base, Ordering::Relaxed);
        let start = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| read_library(&file)));
        let (took, used) = (
            start.elapsed(),
            PEAK.load(Ordering::Relaxed).saturating_sub(base),
        );
        let result = result.unwrap_or_else(|_| panic!("mutation {n} panicked on {file:02x?}"));
        assert!(used < MEMORY_LIMIT, "mutation {n} used {used} bytes");
        assert!(took < TIME_LIMIT, "mutation {n} took {took:?}");
        (slowest, most) = (slowest.max(took), most.max(used));
        match result {
            Ok(lib) => {
                if lib.report.is_clean() {
                    whole += 1;
                } else {
                    partial += 1;
                }
                holds_to_limits(&lib.brushes, n);
            }
            Err(_) => refused += 1,
        }
    }
    eprintln!(
        "{count} mutations: {whole} whole, {partial} partly lost, {refused} refused, slowest {slowest:?}, most memory {most} bytes"
    );
    assert!(
        whole > 0 && partial > 0 && refused > 0,
        "the mutations should land on all three sides"
    );
}

#[test]
fn three_thousand_damaged_libraries_never_panic() {
    run(3_000);
}

#[test]
#[ignore = "long: set PIKSLAR_BRUSH_MUTATIONS, and run in release"]
fn many_damaged_libraries_never_panic() {
    let count = std::env::var("PIKSLAR_BRUSH_MUTATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);
    run(count);
}
