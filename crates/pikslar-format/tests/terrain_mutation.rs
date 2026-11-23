//! Damaged terrain sections: the `Atil` section of a project is mutated at random, by a small
//! generator that is the same on every run, and put back in a sound file. Every file must open or
//! be refused without a panic, in modest memory and time; what opens must keep the tilemap rules
//! and save to a file that opens again with nothing to note. The normal run does 4,000 files; the
//! ignored test does as many as `PIKSLAR_TERRAIN_MUTATIONS` says (100,000 by default).

use pikslar_core::autotile::TerrainKind;
use pikslar_core::orient::Orientation;
use pikslar_core::tilemap::MapCell;
use pikslar_core::tileset::TileId;
use pikslar_core::{Cel, ColorMode, Document, Grid};
use pikslar_format::piks::{load_noted, save};
use std::alloc::{GlobalAlloc, Layout, System};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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

/// The most one open of a small project may use above what was in use before it.
const MEMORY_LIMIT: usize = 16 << 20;
/// The longest one may take, even in a build with no optimising.
const TIME_LIMIT: Duration = Duration::from_secs(5);
/// One run at a time, so the peak is this test's own.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const TAG: &[u8; 4] = b"Atil";

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

/// The CRC-32 of the file format (IEEE, reflected), so a mutated section reaches the reader.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn id(n: u32) -> TileId {
    TileId::new(n).expect("an id")
}

/// A map of two tilesets, each with terrain sets of every kind.
fn project() -> Vec<u8> {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    let mut sets = Vec::new();
    for name in ["Ground", "Walls"] {
        let set = doc
            .add_tileset(name, ColorMode::Rgba, (4, 4))
            .expect("room");
        let image = Arc::new(Grid::from_cells(4, 4, &[0xff00_00ff; 16]).into_image());
        doc.add_tile(set, image.clone()).expect("fits");
        doc.add_tile(set, image).expect("fits");
        doc.add_terrain(set, TerrainKind::Edge4, "Road", false)
            .expect("room");
        doc.add_terrain(set, TerrainKind::Corner4, "Sand", false)
            .expect("room");
        doc.add_terrain(set, TerrainKind::Blob47, "Water", true)
            .expect("room");
        doc.set_terrain_slot(set, 1, 3, Some(id(2))).expect("slot");
        sets.push(set);
    }
    doc.add_tilemap_layer(1, sets[0]).expect("room");
    let cell = MapCell::place(id(1), Orientation::IDENTITY).bits();
    let map = Cel::new(Grid::from_cells(2, 2, &[cell, 0, 0, cell]));
    doc.put_cel(1, 0, map).expect("fits");
    save(&doc)
}

/// Where the section is: the offset of its tag and the length of its data.
fn find(bytes: &[u8]) -> (usize, usize) {
    let at = bytes
        .windows(4)
        .rposition(|w| w == TAG)
        .expect("an Atil section");
    let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().expect("4 bytes"));
    (at, len as usize)
}

/// The project with the section's data replaced, its length set and the file resealed.
fn with_data(bytes: &[u8], data: &[u8]) -> Vec<u8> {
    let (at, len) = find(bytes);
    let mut out = bytes[..at + 4].to_vec();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&bytes[at + 8 + len..]);
    let end = out.len() - 4;
    let crc = crc32(&out[..end]);
    out[end..].copy_from_slice(&crc.to_le_bytes());
    out
}

/// Values that matter to the section: counts, lengths, kinds and ids at their edges.
const SPECIAL: &[u8] = &[0, 1, 2, 3, 16, 46, 47, 63, 64, 65, 0x7f, 0x80, 0xfe, 0xff];

fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut b = seed.to_vec();
    for _ in 0..1 + rng.below(4) {
        let at = rng.below(b.len());
        match rng.below(7) {
            0 => b[at] ^= 1 << rng.below(8),
            1 => b.truncate(at),
            2 => b[at] = SPECIAL[rng.below(SPECIAL.len())],
            3 => drop(b.drain(at..(at + 1 + rng.below(40)).min(b.len()))),
            4 => {
                let end = (at + 1 + rng.below(120)).min(b.len());
                let copy = b[at..end].to_vec();
                b.splice(at..at, copy);
            }
            5 => {
                // A length or count set huge, little end first.
                let end = (at + 4).min(b.len());
                b[at..end].iter_mut().for_each(|v| *v = 0xff);
            }
            _ => {
                let run = vec![b[at]; 1 + rng.below(2_000)];
                b.splice(at..at, run);
            }
        }
        if b.is_empty() {
            b.push(0);
        }
    }
    b
}

/// Opens one mutated project, checking it keeps the rules and saves cleanly if it opens.
fn try_one(file: &[u8]) -> Result<(), String> {
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let started = Instant::now();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if let Ok((doc, _)) = load_noted(file) {
            doc.check_tilemaps().map_err(|f| f.to_string())?;
            let (_, notes) = load_noted(&save(&doc)).map_err(|e| e.to_string())?;
            if !notes.can_save_over() {
                return Err(format!("saved again, it notes {notes:?}"));
            }
        }
        Ok(())
    }));
    let used = PEAK.load(Ordering::Relaxed).saturating_sub(base);
    match outcome {
        Err(_) => Err("panicked".into()),
        Ok(Err(why)) => Err(why),
        Ok(Ok(())) if used > MEMORY_LIMIT => Err(format!("used {used} bytes")),
        Ok(Ok(())) if started.elapsed() > TIME_LIMIT => Err("took too long".into()),
        Ok(Ok(())) => Ok(()),
    }
}

fn run(count: usize) {
    let _alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let bytes = project();
    let (at, len) = find(&bytes);
    let seed = bytes[at + 8..at + 8 + len].to_vec();
    let mut rng = Rng(0x5eed_a711);
    for n in 0..count {
        let data = mutate(&mut rng, &seed);
        if let Err(why) = try_one(&with_data(&bytes, &data)) {
            panic!("mutation {n} ({} bytes): {why}", data.len());
        }
    }
}

#[test]
fn mutated_terrain_sections_open_or_are_refused_safely() {
    run(4_000);
}

#[test]
#[ignore = "a long run for release checks"]
fn many_mutated_terrain_sections_open_or_are_refused_safely() {
    let count = std::env::var("PIKSLAR_TERRAIN_MUTATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);
    run(count);
}
