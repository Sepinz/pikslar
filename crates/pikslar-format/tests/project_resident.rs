//! What a project's images and cels really take while a file is read, counted by an allocator
//! that sees every allocation of this thread, against what the reader charges its decode limit
//! for them (`pikslar_format::piks::charges`). The charge must cover the peak of a read, so the
//! limit bounds what a file can make the program hold.

use pikslar_core::{Cel, ColorMode, ColorTable, Document, FrameInfo, Grid, Layer};
use pikslar_format::piks;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

thread_local! {
    static NOW: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: every call is passed to the system allocator unchanged. The counters are const
// initialised thread locals without destructors, so touching them never allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = NOW.try_with(|now| {
            now.set(now.get() + layout.size());
            let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now.get())));
        });
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

/// The peak of reading `bytes`, above what was held before, and what stays held.
fn read(bytes: &[u8]) -> (usize, usize) {
    let before = NOW.with(Cell::get);
    PEAK.with(|peak| peak.set(before));
    let doc = piks::load(bytes).expect("opens");
    let peak = PEAK.with(Cell::get) - before;
    let kept = NOW.with(Cell::get) - before;
    drop(doc);
    (peak, kept)
}

/// A 16 by 16 project of `layers` by `frames` cels, each its own one-pixel drawing when
/// `distinct`, or all one drawing.
fn project(layers: usize, frames: usize, distinct: bool) -> Vec<u8> {
    let one = Cel::new(Grid::from_cells(16, 16, &{
        let mut px = [0u32; 256];
        px[0] = 0xff00_00ff;
        px
    }));
    let list = (0..layers)
        .map(|l| {
            let cels = (0..frames)
                .map(|f| match distinct {
                    true => {
                        let mut px = [0u32; 256];
                        px[(l + f) % 256] = 0xff00_0000 | (l * frames + f) as u32;
                        Cel::new(Grid::from_cells(16, 16, &px))
                    }
                    false => one.clone(),
                })
                .collect();
            Layer::new(format!("L{l}"), ColorMode::Rgba).with_cels(cels)
        })
        .collect();
    let frames = vec![FrameInfo::new(100); frames];
    let colors = Arc::new(ColorTable::from_parts(&[0], &[]));
    let doc = Document::from_parts(16, 16, colors, list, frames).expect("a document");
    piks::save(&doc)
}

#[test]
fn the_decode_charge_covers_the_peak_of_reading_many_cels() {
    for distinct in [true, false] {
        let (layers, frames) = (64, 2048);
        let bytes = project(layers, frames, distinct);
        let charged = piks::charges(&bytes).expect("reads");
        let (peak, kept) = read(&bytes);
        let cels = (layers * frames) as f64;
        println!(
            "distinct {distinct}: {} byte file, peak {:.1} B a cel, kept {:.1}, charged {:.1}",
            bytes.len(),
            peak as f64 / cels,
            kept as f64 / cels,
            charged as f64 / cels
        );
        assert!(
            charged >= peak,
            "distinct {distinct}: charged {charged} under the {peak} peak"
        );
    }
}
