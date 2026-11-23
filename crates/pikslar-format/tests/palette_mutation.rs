//! Damaged palette files: hand-written GIMP, JASC and hex files are mutated at random, by a small
//! generator that is the same on every run, and every one must be read or refused without a
//! panic, in modest memory and time. What is read must be a palette the writers can write and
//! the readers read again, to the same colours. The normal run does 3,000 per format; the ignored
//! test does as many as `PIKSLAR_PALETTE_MUTATIONS` says (100,000 by default), for release runs.

use pikslar_format::palette_file::{self, PaletteFormat, MAX_COLORS};
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

/// The most one read of a small file may use.
const MEMORY_LIMIT: usize = 64 << 20;
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

/// Bytes that matter to these formats.
const SPECIAL: &[u8] = b" \t\r\n#0123456789-+.:aAfFgG\xEF\xBB\xBF\xFF\xFE\0";

fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut b = seed.to_vec();
    for _ in 0..1 + rng.below(4) {
        let at = rng.below(b.len());
        match rng.below(8) {
            0 => b[at] ^= 1 << rng.below(8),
            1 => b.truncate(at),
            2 => b[at] = SPECIAL[rng.below(SPECIAL.len())],
            3 => drop(b.drain(at..(at + 1 + rng.below(20)).min(b.len()))),
            4 => {
                let junk: Vec<u8> = (0..1 + rng.below(8))
                    .map(|_| SPECIAL[rng.below(SPECIAL.len())])
                    .collect();
                b.splice(at..at, junk);
            }
            5 => {
                let end = (at + 1 + rng.below(60)).min(b.len());
                let copy = b[at..end].to_vec();
                b.splice(at..at, copy);
            }
            6 => {
                let run = vec![b[at]; 1 + rng.below(3_000)];
                b.splice(at..at, run);
            }
            _ => b.splice(at..at, b"\r\n".iter().copied()).for_each(drop),
        }
        if b.is_empty() {
            b.push(b'0');
        }
    }
    b
}

fn corpus() -> Vec<(PaletteFormat, Vec<u8>)> {
    let fixtures: [(PaletteFormat, &[u8]); 5] = [
        (
            PaletteFormat::Gpl,
            include_bytes!("fixtures/palette/sunset.gpl"),
        ),
        (
            PaletteFormat::Gpl,
            include_bytes!("fixtures/palette/plain.gpl"),
        ),
        (
            PaletteFormat::Hex,
            include_bytes!("fixtures/palette/lospec.hex"),
        ),
        (
            PaletteFormat::Jasc,
            include_bytes!("fixtures/palette/rgb.pal"),
        ),
        (
            PaletteFormat::Jasc,
            include_bytes!("fixtures/palette/short_count.pal"),
        ),
    ];
    let mut seeds: Vec<(PaletteFormat, Vec<u8>)> =
        fixtures.iter().map(|(f, b)| (*f, b.to_vec())).collect();
    // The same files with Windows line ends and a byte order mark.
    for (f, b) in fixtures {
        let text = String::from_utf8_lossy(b)
            .replace("\r\n", "\n")
            .replace('\n', "\r\n");
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(text.as_bytes());
        seeds.push((f, with_bom));
    }
    seeds
}

/// What was read is a palette the writers write and the readers read again, unchanged.
fn survives_a_write(palette: &palette_file::PaletteData, n: u64) {
    assert!(
        !palette.colors.is_empty() && palette.colors.len() <= MAX_COLORS,
        "mutation {n}: colour count {}",
        palette.colors.len()
    );
    assert!(palette.names.is_empty() || palette.names.len() == palette.colors.len());
    assert!(palette.colors.iter().all(|c| c[3] == 255));
    for f in [PaletteFormat::Gpl, PaletteFormat::Jasc, PaletteFormat::Hex] {
        let bytes = palette_file::write(palette, f)
            .unwrap_or_else(|e| panic!("mutation {n}: {f:?} write refused: {e}"));
        let back = palette_file::read(&bytes, f)
            .unwrap_or_else(|e| panic!("mutation {n}: {f:?} own file refused: {e}"));
        assert_eq!(back.palette.colors, palette.colors, "mutation {n}: {f:?}");
    }
}

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn run(count: u64) {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let seeds = corpus();
    let (mut read, mut refused) = (0u64, 0u64);
    let (mut slowest, mut most) = (Duration::ZERO, 0usize);
    for n in 0..count {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ n.wrapping_mul(0xd6e8_feb8_6659_fd93) | 1);
        let (format, seed) = &seeds[rng.below(seeds.len())];
        let file = mutate(&mut rng, seed);
        let base = CURRENT.load(Ordering::Relaxed);
        PEAK.store(base, Ordering::Relaxed);
        let start = Instant::now();
        let result = catch_unwind(AssertUnwindSafe(|| {
            // Half the time the format is the file's own, half the time it is sniffed.
            if n % 2 == 0 {
                palette_file::read(&file, *format)
            } else {
                palette_file::read_any(&file)
            }
        }));
        let (took, used) = (
            start.elapsed(),
            PEAK.load(Ordering::Relaxed).saturating_sub(base),
        );
        let result = result.unwrap_or_else(|_| panic!("mutation {n} panicked on {file:02x?}"));
        assert!(used < MEMORY_LIMIT, "mutation {n} used {used} bytes");
        assert!(took < TIME_LIMIT, "mutation {n} took {took:?}");
        (slowest, most) = (slowest.max(took), most.max(used));
        match result {
            Ok(r) => {
                read += 1;
                survives_a_write(&r.palette, n);
            }
            Err(e) => {
                refused += 1;
                assert!(!e.message.is_empty(), "mutation {n}: empty message");
                assert!(!e.message.chars().any(char::is_control), "mutation {n}");
            }
        }
    }
    eprintln!(
        "{count} mutations: {read} read, {refused} refused, slowest {slowest:?}, most memory {most} bytes"
    );
    assert!(
        read > 0 && refused > 0,
        "the mutations should land on both sides"
    );
}

#[test]
fn three_thousand_damaged_files_never_panic() {
    run(3_000);
}

#[test]
#[ignore = "long: set PIKSLAR_PALETTE_MUTATIONS, and run in release"]
fn many_damaged_files_never_panic() {
    let count = std::env::var("PIKSLAR_PALETTE_MUTATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);
    run(count);
}
