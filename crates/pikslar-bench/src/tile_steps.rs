//! Frame steps on 2,048 pixel tilemaps as the window takes them: the frame change damages the
//! canvas tiles that look different, the compositor composites those the view shows, and the
//! changed parts are blitted to a 1400 by 900 window, at zoom 1 (as `limits.ps1` runs the
//! program) and with the whole map fitted in the canvas.
//!
//! Three maps: the stress map; a realistic one, where only a few hundred cells of one layer are
//! animated and the rest is held; and the worst case, where every map layer changes every cell
//! in every frame, so that every canvas tile changes.

use crate::gate::{Kind, Metric};
use crate::stress_map::{self, map_cells, CELLS, FRAMES, TILES};
use crate::workloads::{best_of, push};
use pikslar_core::{Damage, Document, Editor};
use pikslar_render::{blit_canvas, shown_region, Compositor, Rect, Surface, View, PARALLEL_WORK};
use std::hint::black_box;

/// The canvas area of a 1400 by 900 window at interface scale 1, as `layout.rs` lays it out
/// with the cel matrix folded: 1400 - 44 - 268 wide, 900 - 32 - 82 high.
pub const WINDOW_CANVAS: Rect = Rect::new(44, 32, 1088, 786);
const WINDOW: (i32, i32) = (1400, 900);
/// Cells of the realistic map's animated layer that change from frame to frame.
pub const ANIMATED: usize = 256;

/// How the window shows the map.
#[derive(Clone, Copy)]
pub enum Zoom {
    /// One screen pixel for each document pixel, the map centred: part of it shows.
    One,
    /// The whole map fitted in the canvas.
    Fit,
    /// So many screen pixels for each document pixel, centred, as `--zoom` starts the program.
    Times(i32),
}

/// The view of a square document `side` pixels wide.
pub fn view(zoom: Zoom, side: usize) -> View {
    let side = side as i32;
    let centred = |num: i32| View {
        num,
        den: 1,
        x: WINDOW_CANVAS.x + (WINDOW_CANVAS.w - side * num) / 2,
        y: WINDOW_CANVAS.y + (WINDOW_CANVAS.h - side * num) / 2,
    };
    match zoom {
        Zoom::One => centred(1),
        Zoom::Times(num) => centred(num.max(1)),
        Zoom::Fit => View::fit(side, side, WINDOW_CANVAS),
    }
}

/// A document, its compositor and a window, kept between frame steps as the app keeps them.
pub struct WindowStep {
    editor: Editor,
    comp: Compositor,
    view: View,
    fb: Vec<u32>,
    xmap: Vec<i32>,
    changed: Vec<Rect>,
}

impl WindowStep {
    /// The first frame, drawn whole.
    pub fn new(doc: &Document, zoom: Zoom) -> Self {
        let mut step = WindowStep {
            editor: Editor::new(doc.clone()),
            comp: Compositor::new(doc.width, doc.height),
            view: view(zoom, doc.width),
            fb: vec![0; (WINDOW.0 * WINDOW.1) as usize],
            xmap: Vec::with_capacity(WINDOW.0 as usize),
            changed: Vec::new(),
        };
        step.editor.damage_all();
        step.draw(true);
        step
    }

    /// Composites on at most `threads` threads from now on, on more than one from `min_work`.
    pub fn set_threads(&mut self, (threads, min_work): (usize, usize)) {
        self.comp.set_threads(threads, min_work);
    }

    /// Cuts threaded steps into `per_thread` shared bands for each thread from now on.
    pub fn set_bands_per_thread(&mut self, per_thread: usize) {
        self.comp.set_bands_per_thread(per_thread);
    }

    /// Shows the next frame, as the timeline's frame step and the next canvas draw do.
    pub fn step(&mut self) {
        let frames = self.editor.doc().frames.len();
        let from = self.editor.frame;
        self.editor.frame = (from + 1) % frames;
        self.editor.damage_frame_change(from);
        self.draw(false);
    }

    fn draw(&mut self, whole: bool) {
        let size = (self.comp.width(), self.comp.height());
        self.comp
            .set_visible(Some(shown_region(self.view, WINDOW_CANVAS, size)));
        self.changed.clear();
        let damage = self.editor.take_damage();
        self.comp.update(&self.editor, damage, &mut self.changed);
        let mut surface = Surface {
            px: &mut self.fb,
            w: WINDOW.0,
            h: WINDOW.1,
        };
        let (w, h) = (size.0 as i32, size.1 as i32);
        let px = self.comp.pixels();
        if whole {
            blit_canvas(
                px,
                w,
                h,
                self.view,
                &mut surface,
                WINDOW_CANVAS,
                &mut self.xmap,
            );
            return;
        }
        for r in &self.changed {
            let clip = self.view.to_screen(*r).intersect(WINDOW_CANVAS);
            blit_canvas(px, w, h, self.view, &mut surface, clip, &mut self.xmap);
        }
        black_box(&self.fb);
    }

    pub fn compositor(&self) -> &Compositor {
        &self.comp
    }
}

/// The time of one window frame step, best of runs, each run stepping through every frame, on
/// as many threads as the program uses, or on `(threads, from this work on)`.
pub fn window_step_micros(doc: &Document, zoom: Zoom, threads: Option<(usize, usize)>) -> f64 {
    let mut window = WindowStep::new(doc, zoom);
    if let Some(threads) = threads {
        window.set_threads(threads);
    }
    let frames = doc.frames.len();
    best_of(|| (0..frames).for_each(|_| window.step())) / frames as f64
}

/// The time of compositing every canvas tile on a frame step on one thread, as the program
/// did before.
pub fn full_step_micros(doc: &Document) -> f64 {
    let mut editor = Editor::new(doc.clone());
    let mut comp = Compositor::new(doc.width, doc.height);
    comp.set_threads(1, PARALLEL_WORK);
    let mut changed = Vec::new();
    let frames = doc.frames.len();
    best_of(|| {
        for frame in 0..frames {
            editor.frame = (frame + 1) % frames;
            changed.clear();
            comp.update(&editor, Damage::All, &mut changed);
            black_box(comp.pixels());
        }
    }) / frames as f64
}

/// Whether cell `i` of the realistic map's animated layer is animated: one in every 256,
/// spread over the map.
fn animated(i: usize) -> bool {
    i % (CELLS * CELLS / ANIMATED) == 37
}

/// The stress map's tiles laid out as a level might be: a ground layer whose water cells cycle
/// through four tiles, and two still layers over it. Only the water changes between frames.
pub fn realistic_map() -> Document {
    stress_map::map_document(
        |layer, frame| {
            let mut cells = map_cells(layer, 0);
            if layer == 1 {
                for (i, cell) in cells.iter_mut().enumerate().filter(|(i, _)| animated(*i)) {
                    *cell = 1 + (i as u32 + frame as u32 % 4) % TILES;
                }
            }
            cells
        },
        &[0, 2, 3],
    )
}

/// The stress map with every map layer changing every cell in every frame.
pub fn worst_map() -> Document {
    stress_map::map_document(map_cells, &[0])
}

pub fn measure(out: &mut Vec<Metric>) {
    let maps = [
        ("stress", stress_map::stress_map_document()),
        ("realistic", realistic_map()),
        ("worst", worst_map()),
    ];
    for (name, doc) in &maps {
        for (zoom, label) in [(Zoom::One, "zoom1"), (Zoom::Fit, "fit")] {
            let time = window_step_micros(doc, zoom, None);
            push(
                out,
                &format!("tile_step_{name}_{label}"),
                Kind::Micros,
                time,
            );
        }
    }
    // The same steps kept to one thread, so that the compositing itself stays watched.
    let one = [
        (&maps[0].1, Zoom::One, "stress_zoom1"),
        (&maps[2].1, Zoom::Fit, "worst_fit"),
    ];
    for (doc, zoom, label) in one {
        let time = window_step_micros(doc, zoom, Some((1, PARALLEL_WORK)));
        let name = format!("tile_step_{label}_one_thread");
        push(out, &name, Kind::Micros, time);
    }
    let worst = &maps[2].1;
    push(
        out,
        "tile_step_worst_full",
        Kind::Micros,
        full_step_micros(worst),
    );
    let mut window = WindowStep::new(worst, Zoom::Fit);
    (0..FRAMES).for_each(|_| window.step());
    let held = window.compositor().tile_cache_bytes() as f64;
    push(out, "tile_cache_bytes_worst", Kind::Bytes, held);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stress_map::SIDE;

    fn tiles_changed(doc: &Document, from: usize, to: usize) -> usize {
        let mut count = 0;
        doc.frame_change_tiles((from, to), &doc.screen_layers(), |_| {
            count += 1;
            std::ops::ControlFlow::Continue(())
        });
        count
    }

    #[test]
    fn the_realistic_map_changes_a_few_tiles_and_the_worst_every_one() {
        let side = SIDE / pikslar_core::TILE;
        let realistic = realistic_map();
        let few = tiles_changed(&realistic, 0, 1);
        assert!(few > 0 && few <= ANIMATED, "{few} tiles");
        let worst = worst_map();
        let mut marked = std::collections::HashSet::new();
        worst.frame_change_tiles((0, 1), &worst.screen_layers(), |i| {
            marked.insert(i);
            std::ops::ControlFlow::Continue(())
        });
        assert_eq!(marked.len(), side * side, "every canvas tile");
    }

    /// After `steps` window steps, what the view shows equals a full composite of that frame.
    fn shows_as_a_full_composite(doc: &Document, zoom: Zoom, steps: usize) -> Compositor {
        let mut window = WindowStep::new(doc, zoom);
        (0..steps).for_each(|_| window.step());
        let mut fresh = Compositor::new(SIDE, SIDE);
        let mut editor = Editor::new(doc.clone());
        editor.frame = steps % doc.frames.len();
        fresh.update(&editor, Damage::All, &mut Vec::new());
        let shown = shown_region(view(zoom, SIDE), WINDOW_CANVAS, (SIDE, SIDE));
        for y in shown.y..shown.bottom() {
            let row = y as usize * SIDE;
            let r = row + shown.x as usize..row + shown.right() as usize;
            assert!(
                window.compositor().pixels()[r.clone()] == fresh.pixels()[r],
                "row {y}"
            );
        }
        window.comp
    }

    #[test]
    fn a_window_step_shows_each_map_as_a_full_composite_does() {
        let realistic = realistic_map();
        let comp = shows_as_a_full_composite(&realistic, Zoom::One, 3);
        assert!(comp.stale_tiles() > 0, "water off screen waits");
        let comp = shows_as_a_full_composite(&realistic, Zoom::Fit, 3);
        assert_eq!(comp.stale_tiles(), 0, "the whole map shows");
        for doc in [stress_map::stress_map_document(), worst_map()] {
            shows_as_a_full_composite(&doc, Zoom::One, 2);
            shows_as_a_full_composite(&doc, Zoom::Fit, FRAMES + 1);
        }
    }
}
