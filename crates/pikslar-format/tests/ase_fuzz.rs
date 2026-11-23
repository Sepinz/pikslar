//! Damaged Aseprite files: valid files built from the specification are mutated at random, by
//! a small generator that is the same on every run, and every one must be read or refused
//! without a panic, in modest memory and time. The normal run does 2,000; the ignored test
//! does as many as `PIKSLAR_ASE_MUTATIONS` says (100,000 by default), for release runs.

#[path = "../src/ase_build.rs"]
mod ase_build;

use pikslar_format::{import_ase, piks};
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

/// The most one import of a few hundred bytes may use.
const MEMORY_LIMIT: usize = 192 << 20;
/// The longest one may take, even in a build with no optimising.
const TIME_LIMIT: Duration = Duration::from_secs(5);

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
    0x8000,
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

fn corpus() -> Vec<Vec<u8>> {
    vec![
        ase_build::rich(),
        ase_build::rich_indexed(),
        ase_build::rich_tiles(),
        ase_build::rich_tiles_indexed(),
    ]
}

/// What was read must be a document Pikslar can keep: it saves, loads again from its own file,
/// keeps its groups in order, and saves to the same bytes.
fn survives_a_save(doc: &pikslar_core::Document, n: u64) {
    // A document with tilesets is held to the rules of its tilemaps, and to a save as soon as
    // the project file can keep tilesets.
    assert!(doc.check_tilemaps().is_ok(), "mutation {n}: tilemaps");
    let Ok(saved) = piks::try_save(doc) else {
        assert!(!doc.tilesets.is_empty(), "mutation {n}: a save is refused");
        return;
    };
    let again = piks::load(&saved)
        .unwrap_or_else(|e| panic!("mutation {n}: Pikslar's own save is refused: {e}"));
    assert!(
        again.check_groups().is_ok(),
        "mutation {n}: groups after a save"
    );
    assert_eq!(
        piks::save(&again),
        saved,
        "mutation {n}: a save is not stable"
    );
}

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn run(count: u64) {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let seeds = corpus();
    let (mut read, mut refused, mut mapped) = (0u64, 0u64, 0u64);
    let (mut slowest, mut most) = (Duration::ZERO, 0usize);
    for n in 0..count {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ n.wrapping_mul(0xd6e8_feb8_6659_fd93) | 1);
        let seed = &seeds[rng.below(seeds.len())];
        let file = mutate(&mut rng, seed);
        let base = CURRENT.load(Ordering::Relaxed);
        PEAK.store(base, Ordering::Relaxed);
        let start = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| import_ase(&file).map(|(doc, _)| doc)));
        let (took, used) = (
            start.elapsed(),
            PEAK.load(Ordering::Relaxed).saturating_sub(base),
        );
        let doc = result.unwrap_or_else(|_| panic!("mutation {n} panicked on {file:02x?}"));
        assert!(used < MEMORY_LIMIT, "mutation {n} used {used} bytes");
        assert!(took < TIME_LIMIT, "mutation {n} took {took:?}");
        (slowest, most) = (slowest.max(took), most.max(used));
        match doc {
            Ok(doc) => {
                read += 1;
                mapped += u64::from(doc.layers().iter().any(|l| l.tileset().is_some()));
                assert!(doc.check_groups().is_ok(), "mutation {n}: groups");
                let frames = doc.frames.len();
                assert!(doc.layers().iter().all(|l| l.frame_count() == frames));
                assert!(doc.tags.iter().all(|t| t.from <= t.to && t.to < frames));
                survives_a_save(&doc, n);
            }
            Err(_) => refused += 1,
        }
    }
    eprintln!(
        "{count} mutations: {read} read ({mapped} with tilemaps), {refused} refused, slowest {slowest:?}, most memory {most} bytes"
    );
    assert!(
        read > 0 && refused > 0 && mapped > 0,
        "the mutations should land on both sides"
    );
}

#[test]
fn two_thousand_damaged_files_never_panic() {
    run(2_000);
}

#[test]
#[ignore = "long: set PIKSLAR_ASE_MUTATIONS, and run in release"]
fn many_damaged_files_never_panic() {
    let count = std::env::var("PIKSLAR_ASE_MUTATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);
    run(count);
}
