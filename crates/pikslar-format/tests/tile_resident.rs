//! What a tile really holds once a project is read, counted by an allocator that sees every
//! allocation of this thread, against what the tile budget charges for it
//! (`pikslar_core::tileset::stored_tile_bytes`). The charge must never be below what is resident,
//! so that the tile budget is a real bound, and not far above it, so that it does
//! not refuse more than it must.

use pikslar_core::tileset::{stored_tile_bytes, TileEntry, TileId, Tileset, TilesetId, Tilesets};
use pikslar_core::{ColorMode, Document, Grid, Image};
use pikslar_format::piks;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

thread_local! {
    static NOW: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: every call is passed to the system allocator unchanged. The counter is a const
// initialised thread local without a destructor, so touching it never allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = NOW.try_with(|now| now.set(now.get() + layout.size()));
        // SAFETY: the caller's layout obligations are the ones `System` requires.
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

/// What reading `bytes` leaves in memory, with the document kept.
fn resident(bytes: &[u8]) -> (Document, usize) {
    let before = NOW.with(Cell::get);
    let doc = piks::load(bytes).expect("opens");
    (doc, NOW.with(Cell::get) - before)
}

/// A project of one tileset holding `count` tiles of `image`, each stored on its own.
fn project(count: usize, side: usize, image: &Image) -> Vec<u8> {
    let tiles: Vec<TileEntry> = (1..=count as u32)
        .map(|n| TileEntry {
            id: TileId::new(n).expect("id"),
            image: Arc::new(image.clone()),
        })
        .collect();
    let set = Tileset::from_parts(
        TilesetId::new(1),
        "T".into(),
        ColorMode::Rgba,
        (side, side),
        count as u32 + 1,
        tiles,
    )
    .expect("a tileset");
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("size");
    doc.tilesets = Arc::new(Tilesets::from_parts(2, vec![set]).expect("a table"));
    piks::save(&doc)
}

/// Resident bytes a tile adds, from two projects that differ only in their tile count, so the
/// document around them cancels out.
fn per_tile(count: usize, side: usize, image: &Image) -> f64 {
    let (_small, base) = resident(&project(1, side, image));
    let (_large, full) = resident(&project(count + 1, side, image));
    (full - base) as f64 / count as f64
}

fn images() -> Vec<(&'static str, usize, Image)> {
    let noise: Vec<u32> = (0..256u32 * 256)
        .map(|i| 0xff00_0000 | i.wrapping_mul(2_654_435_761) >> 8)
        .collect();
    let two: Vec<u32> = (0..16 * 16).map(|i| 0xff00_0000 | (i % 2)).collect();
    vec![
        ("blank", 8, Image::blank()),
        (
            "one pixel",
            1,
            Grid::from_cells(1, 1, &[0xff12_3456]).into_image(),
        ),
        (
            "two colours",
            16,
            Grid::from_cells(16, 16, &two).into_image(),
        ),
        (
            "solid 256",
            256,
            Grid::from_cells(256, 256, &[0xff10_2030; 256 * 256]).into_image(),
        ),
        (
            "noise 256",
            256,
            Grid::from_cells(256, 256, &noise).into_image(),
        ),
    ]
}

/// The charge against what is resident, for every kind of tile and for counts that fill the
/// tile index well and badly (just after and just before it grows).
#[test]
fn the_tile_charge_covers_what_a_read_tile_holds() {
    let mut wrong = Vec::new();
    for (name, side, image) in images() {
        let counts: &[usize] = match side {
            256 => &[64, 200],
            _ => &[1_000, 14_337, 28_671, 57_345, 65_534],
        };
        for &count in counts {
            let held = per_tile(count, side, &image);
            let charged = stored_tile_bytes(&image) as f64;
            println!(
                "{name:12} {count:6} tiles: resident {held:10.1} B a tile, charged {charged:10.1} \
                 ({:.3})",
                charged / held
            );
            if charged < held || charged > held * 1.25 + 16.0 {
                wrong.push(format!("{name}, {count} tiles: {charged} for {held}"));
            }
        }
    }
    assert!(wrong.is_empty(), "the charge does not fit: {wrong:#?}");
}
