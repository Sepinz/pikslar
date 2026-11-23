//! A stroke with a custom brush allocates nothing for each input event: once the stroke has
//! begun and its overlay tiles exist, more events only add to the damage list, which grows by
//! doubling. The test counts this thread's allocations over strokes of different lengths.

use pikslar_core::custom_brush::{Brush, BrushChoice, BrushPaint};
use pikslar_core::{rgba, ColorMode, Document, DrawCmd, Editor, Tool};
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

/// Begins a stroke, then runs `events` pointer events back and forth over the same area, and
/// returns what the events (not the begin) allocated.
fn event_allocations(mode: ColorMode, tool: Tool, events: usize) -> usize {
    let mut e = Editor::new(Document::new(128, 128, mode).expect("valid"));
    e.pixel_perfect = false;
    e.color = rgba(10, 200, 30, 255);
    let side = 32;
    let colors: Vec<u32> = (0..side * side)
        .map(|i| {
            if i % 5 == 0 {
                0
            } else {
                rgba(i as u8, 90, 160, 255)
            }
        })
        .collect();
    let brush = Brush::from_parts("b", side, side, &colors, None, BrushPaint::OwnColors);
    e.set_brush(BrushChoice::Custom(Arc::new(brush.expect("brush"))));
    // One stroke first, so the overlay's tile pool and the colour table are warm.
    let walk = |e: &mut Editor, n: usize| {
        for i in 0..n {
            let (x, y) = (24 + (i * 7) % 80, 24 + (i * 13) % 80);
            e.apply(DrawCmd::To {
                x: x as i32,
                y: y as i32,
            })
            .expect("to");
        }
    };
    let begin = DrawCmd::Begin { tool, x: 30, y: 30 };
    e.apply(begin.clone()).expect("begin");
    walk(&mut e, 150);
    e.apply(DrawCmd::Cancel).expect("cancel");
    e.apply(begin).expect("begin");
    let used = allocations_of(|| walk(&mut e, events));
    e.apply(DrawCmd::Commit).expect("commit");
    used
}

#[test]
fn pointer_events_with_a_custom_brush_do_not_allocate_each() {
    for mode in [ColorMode::Palette, ColorMode::Rgba] {
        for tool in [Tool::Pencil, Tool::Eraser, Tool::Line, Tool::Rect] {
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
