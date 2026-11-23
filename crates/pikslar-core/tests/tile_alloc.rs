//! A map stroke allocates nothing for each input event beyond the damage list, which grows by
//! doubling, and the overlay's tile pool: once a stroke has begun and its overlay tiles exist,
//! more events only add to the damage list. Counted as `brush_alloc.rs` counts, over strokes of
//! different lengths on this thread.

use pikslar_core::{ColorMode, Document, Editor, Image, TileCmd, TileTool};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

thread_local! {
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: every call is passed to the system allocator unchanged. The counter is a const
// initialised thread local without a destructor, so touching it never allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        // SAFETY: the caller's layout obligations are the ones `System` requires.
        unsafe { System.alloc(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        // SAFETY: `ptr` and `layout` came from this allocator, as `System` requires.
        unsafe { System.realloc(ptr, layout, new) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `alloc` above with this same layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn allocations_of(work: impl FnOnce()) -> usize {
    let before = COUNT.with(Cell::get);
    work();
    COUNT.with(Cell::get) - before
}

/// A 256 by 256 canvas of 8 by 8 tiles, so the map is 32 by 32 cells, with a map layer active
/// and a 3 by 3 block of tiles chosen.
fn editor(mode: ColorMode) -> Editor {
    let mut doc = Document::new(256, 256, mode).expect("valid");
    let set = doc.add_tileset("T", mode, (8, 8)).expect("room");
    let image = Arc::new(Image::blank());
    let tiles: Vec<_> = (0..9)
        .map(|_| Some(doc.add_tile(set, image.clone()).expect("fits")))
        .collect();
    doc.add_tilemap_layer(1, set).expect("room");
    let mut e = Editor::new(doc);
    e.layer = 1;
    e.set_tile_block(set, 3, 3, &tiles).expect("a block");
    e
}

/// Begins a stroke, then runs `events` pointer events back and forth over the same area, and
/// returns what the events (not the begin) allocated.
fn event_allocations(mode: ColorMode, tool: TileTool, events: usize) -> usize {
    let mut e = editor(mode);
    let walk = |e: &mut Editor, n: usize| {
        for i in 0..n {
            let (x, y) = (24 + (i * 7) % 200, 24 + (i * 13) % 200);
            e.apply(TileCmd::To {
                x: x as i32,
                y: y as i32,
            })
            .expect("to");
        }
    };
    let begin = TileCmd::Begin { tool, x: 30, y: 30 };
    // One stroke first, so the overlay's tiles are in the pool for the next.
    e.apply(begin.clone()).expect("begin");
    walk(&mut e, 150);
    e.apply(TileCmd::Cancel).expect("cancel");
    e.apply(begin).expect("begin");
    let used = allocations_of(|| walk(&mut e, events));
    e.apply(TileCmd::Commit).expect("commit");
    used
}

#[test]
fn pointer_events_of_a_map_stroke_do_not_allocate_each() {
    for mode in [ColorMode::Palette, ColorMode::Rgba] {
        for tool in [
            TileTool::Place,
            TileTool::Erase,
            TileTool::Line,
            TileTool::Rect,
        ] {
            let short = event_allocations(mode, tool, 100);
            let long = event_allocations(mode, tool, 600);
            let extra = long.saturating_sub(short);
            assert!(
                extra <= 6,
                "{mode:?} {tool:?}: 500 more events cost {extra} allocations ({short} to {long})"
            );
        }
    }
}
