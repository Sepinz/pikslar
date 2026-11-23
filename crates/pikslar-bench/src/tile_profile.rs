//! `pikslar-bench tile-profile`: where the time of a frame step on the tilemap stress map goes.
//! It prints a breakdown, not gate lines: the whole composite, each layer on its own, reading
//! each layer's canvas tiles without blending them, the blit to a window-sized canvas at the
//! zooms the window uses, and the part of the canvas a 1400 by 900 window shows at zoom 1.

use crate::stress_map::{self, SIDE};
use crate::tile_steps::{self, Zoom, WINDOW_CANVAS};
use crate::workloads::best_of;
use pikslar_core::tilemap::TileCache;
use pikslar_core::{Damage, Document, Editor, TILE, TILE_AREA};
use pikslar_render::{blit_canvas, Compositor, Rect, Surface, View};
use std::hint::black_box;

fn ms(micros: f64) -> f64 {
    micros / 1e3
}

/// The time of one composite of every canvas tile, averaged over stepping through the frames.
fn step_all(doc: &Document) -> f64 {
    let mut editor = Editor::new(doc.clone());
    let mut comp = Compositor::new(doc.width, doc.height);
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

/// The document with only `keep` shown.
fn only(doc: &Document, keep: usize) -> Document {
    let mut alone = doc.clone();
    for (index, layer) in alone.layers_mut().iter_mut().enumerate() {
        layer.visible = index == keep;
    }
    alone
}

/// Reading every canvas tile of `layer`, frame by frame, without blending.
fn sample_layer(doc: &Document, layer: usize, cached: bool) -> f64 {
    let (tw, th) = (doc.width.div_ceil(TILE), doc.height.div_ceil(TILE));
    let frames = doc.frames.len();
    let mut raw = [0u32; TILE_AREA];
    let mut cache = TileCache::new();
    best_of(|| {
        for frame in 0..frames {
            let layer = &doc.layers()[layer];
            for ty in 0..th {
                for tx in 0..tw {
                    black_box(match cached {
                        true => {
                            doc.layer_tile_cached((layer, frame), (tx, ty), &mut raw, &mut cache)
                        }
                        false => doc.layer_tile(layer, frame, tx, ty, &mut raw),
                    });
                }
            }
            black_box(&raw);
        }
    }) / frames as f64
}

fn blit(comp: &Compositor, view: View, area: Rect) -> f64 {
    let mut fb = vec![0u32; 1400 * 900];
    let mut xmap = Vec::with_capacity(1400);
    let (w, h) = (comp.width() as i32, comp.height() as i32);
    best_of(|| {
        let mut target = Surface {
            px: &mut fb,
            w: 1400,
            h: 900,
        };
        blit_canvas(comp.pixels(), w, h, view, &mut target, area, &mut xmap);
        black_box(&fb);
    })
}

/// The document part a view of `area` shows, in document pixels.
fn shown(view: View, area: Rect, side: usize) -> Rect {
    let (x0, y0) = view.to_doc(area.x, area.y);
    let (x1, y1) = view.to_doc(area.right() - 1, area.bottom() - 1);
    Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1).intersect(Rect::new(0, 0, side as i32, side as i32))
}

/// The region of the current frame a window at zoom 1 shows, composited alone.
fn visible_region(doc: &Document, region: Rect) -> f64 {
    let mut editor = Editor::new(doc.clone());
    let mut comp = Compositor::new(doc.width, doc.height);
    let mut out = Vec::new();
    let frames = doc.frames.len();
    let r = (
        region.x as usize,
        region.y as usize,
        region.w as usize,
        region.h as usize,
    );
    best_of(|| {
        for frame in 0..frames {
            editor.frame = (frame + 1) % frames;
            comp.compose_plain(&editor, r, &mut out);
            black_box(&out);
        }
    }) / frames as f64
}

/// The whole composite, and each layer composited and read on its own.
fn by_layer(doc: &Document) {
    println!(
        "whole composite, all layers           {:8.2}",
        ms(step_all(doc))
    );
    for layer in 0..doc.layers().len() {
        let kind = if doc.layers()[layer].is_tilemap() {
            "map"
        } else {
            "paint"
        };
        println!(
            "layer {layer} ({kind:5}) composited alone      {:8.2}   read only {:8.2}, cached {:8.2}",
            ms(step_all(&only(doc, layer))),
            ms(sample_layer(doc, layer, false)),
            ms(sample_layer(doc, layer, true))
        );
    }
    let none = only(doc, usize::MAX);
    println!(
        "no layer shown (loop and store)       {:8.2}",
        ms(step_all(&none))
    );
}

/// The blits, and the region a window at zoom 1 shows, which it returns.
fn window_region(doc: &Document) -> Rect {
    let mut comp = Compositor::new(SIDE, SIDE);
    comp.update(&Editor::new(doc.clone()), Damage::All, &mut Vec::new());
    let one = View {
        num: 1,
        den: 1,
        x: WINDOW_CANVAS.x + (WINDOW_CANVAS.w - SIDE as i32) / 2,
        y: WINDOW_CANVAS.y + (WINDOW_CANVAS.h - SIDE as i32) / 2,
    };
    let fit = View::fit(SIDE as i32, SIDE as i32, WINDOW_CANVAS);
    println!(
        "blit of the window canvas, zoom 1     {:8.2}",
        ms(blit(&comp, one, WINDOW_CANVAS))
    );
    println!(
        "blit of the window canvas, fit 1/{}    {:8.2}",
        fit.den,
        ms(blit(&comp, fit, WINDOW_CANVAS))
    );
    let region = shown(one, WINDOW_CANVAS, SIDE);
    let tiles = |a: i32, b: i32| (b - 1) / TILE as i32 - a / TILE as i32 + 1;
    let count = tiles(region.x, region.right()) * tiles(region.y, region.bottom());
    println!(
        "zoom 1 shows {}x{} pixels, {} of {} canvas tiles ({:.1}%)",
        region.w,
        region.h,
        count,
        (SIDE / TILE).pow(2),
        100.0 * count as f64 / (SIDE / TILE).pow(2) as f64
    );
    println!(
        "that region composited alone          {:8.2}",
        ms(visible_region(doc, region))
    );
    region
}

pub fn run() {
    let doc = stress_map::stress_map_document();
    println!("tilemap stress map, {SIDE} by {SIDE}, best of runs, per frame step, in ms");
    by_layer(&doc);
    window_region(&doc);
    window_steps(&doc);
}

/// Finding the canvas tiles that change on a frame step, without compositing them.
fn frame_damage(doc: &Document) -> f64 {
    let mut editor = Editor::new(doc.clone());
    let frames = doc.frames.len();
    best_of(|| {
        for frame in 0..frames {
            let from = editor.frame;
            editor.frame = (frame + 1) % frames;
            editor.damage_frame_change(from);
            black_box(editor.take_damage());
        }
    }) / frames as f64
}

/// Frame steps as the window takes them now, on the three maps of `tile_steps.rs`.
fn window_steps(stress: &Document) {
    let maps = [
        ("stress", stress.clone()),
        ("realistic", tile_steps::realistic_map()),
        ("worst", tile_steps::worst_map()),
    ];
    for (name, doc) in &maps {
        println!(
            "frame damage, {name:9}        {:8.2}",
            ms(frame_damage(doc))
        );
        println!(
            "window step, {name:9} zoom 1 {:8.2}   fit {:8.2}   all tiles {:8.2}",
            ms(tile_steps::window_step_micros(doc, Zoom::One, None)),
            ms(tile_steps::window_step_micros(doc, Zoom::Fit, None)),
            ms(tile_steps::full_step_micros(doc))
        );
    }
}
