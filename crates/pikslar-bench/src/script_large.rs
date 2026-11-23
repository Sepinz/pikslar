//! Scripts on realistically large projects: a long timeline of many layers, a 4096 x 4096
//! canvas, and the tilemap stress map. Ordinary scripts (going through layers and frames,
//! renaming, setting durations, recoloring, reading and writing regions) must finish in
//! bounded time and memory; hostile ones must still stop within about a second.

use crate::script_bench::{median_ms, report};
use crate::script_cases::hostile_on;
use crate::stress_map::stress_map_document;
use crate::workloads::{ramp, sprite_layer, Rng};
use pikslar_core::{ColorMode, Document, Editor, Grid};

/// Layers and frames of the long timeline: every layer the document may have, and an
/// animation of 512 frames.
const TIMELINE_LAYERS: usize = 256;
const TIMELINE_FRAMES: usize = 512;
const TIMELINE_SIDE: usize = 64;
/// One frame in this many has a drawing on each layer; the rest are empty, as in a project
/// whose layers hold parts that come and go.
const DRAWN_EVERY: usize = 8;
const CANVAS_SIDE: usize = 4096;

/// 256 layers by 512 frames on a 64 x 64 canvas, a small moving shape on every eighth frame
/// of each layer (16,384 drawings).
pub fn timeline_document() -> Document {
    let side = TIMELINE_SIDE;
    let mut doc = Document::new(side, side, ColorMode::Rgba).unwrap_or_else(|_| unreachable!());
    for at in 1..TIMELINE_LAYERS {
        doc.add_layer(at, ColorMode::Rgba)
            .unwrap_or_else(|_| unreachable!());
    }
    for at in 1..TIMELINE_FRAMES {
        doc.add_frame(at, false).unwrap_or_else(|_| unreachable!());
    }
    for layer in 0..TIMELINE_LAYERS {
        for frame in (0..TIMELINE_FRAMES).step_by(DRAWN_EVERY) {
            let mut px = vec![0u32; side * side];
            let (x0, y0) = ((layer * 7 + frame) % (side - 8), layer % (side - 8));
            for y in y0..y0 + 8 {
                for x in x0..x0 + 8 {
                    px[y * side + x] = ramp(layer as u32 % 8, (x + y) as u32 % 4);
                }
            }
            doc.set_cel(layer, frame, Grid::from_cells(side, side, &px))
                .unwrap_or_else(|_| unreachable!());
        }
    }
    doc
}

/// One 4096 x 4096 RGBA layer with nothing empty: 64 MiB of pixels.
pub fn canvas_document() -> Document {
    let side = CANVAS_SIDE;
    let mut doc = Document::new(side, side, ColorMode::Rgba).unwrap_or_else(|_| unreachable!());
    let mut px = sprite_layer(side, side, &mut Rng(13));
    for (index, pixel) in px.iter_mut().enumerate().filter(|(_, p)| **p == 0) {
        *pixel = ramp(2, index as u32 / 89 % 4);
    }
    doc.set_cel(0, 0, Grid::from_cells(side, side, &px))
        .unwrap_or_else(|_| unreachable!());
    doc
}

/// Goes through every layer and frame: names, visibility, durations, and a 32 x 32 region
/// read, changed and written back on one cel of each of 64 layers.
const TIMELINE_ORDINARY: &str = r#"
    let ls = layers();
    let fs = frames();
    let seen = 0;
    for i in 0..len(ls) {
        if layer_visible(ls[i]) { seen = seen + 1; }
        rename_layer(ls[i], "Part " + str(i + 1) + " of " + str(len(ls)));
    }
    for i in 0..len(fs) { set_duration(fs[i], 40 + i % 3 * 20); }
    for k in 0..64 {
        let l = ls[k * 4]; let f = fs[k * 8];
        let b = read_rect(l, f, 0, 0, 32, 32);
        for i in 0..len(b) { if b[i] != 0 { b[i] = 0xff000000 + (b[i] % 0x1000000) / 2; } }
        write_rect(l, f, 0, 0, 32, b);
    }
    return seen;
"#;

/// Regions read, recolored and written back across the canvas, a block filled and a swatch
/// added and recolored.
const CANVAS_ORDINARY: &str = r#"
    let l = layers()[0]; let f = frames()[0];
    for k in 0..16 {
        let x = (k % 4) * 1024; let y = (k / 4) * 1024;
        let b = read_rect(l, f, x, y, 256, 256);
        for i in 0..len(b) { b[i] = 0xff000000 + (b[i] % 0x1000000) / 2; }
        write_rect(l, f, x, y, 256, b);
    }
    fill_rect(l, f, 2048, 2048, 512, 512, 0xff336699);
    let s = add_swatch(0xff00ff00);
    recolor(s, 0xff0000ff);
    return width() * height();
"#;

/// Every layer's name and kind, durations, and a region of the paint layer read back.
const MAP_ORDINARY: &str = r#"
    let ls = layers();
    let kinds = "";
    for i in 0..len(ls) {
        kinds = kinds + layer_kind(ls[i]) + " ";
        rename_layer(ls[i], "Layer " + str(i));
    }
    let fs = frames();
    for i in 0..len(fs) { set_duration(fs[i], 100); }
    let b = read_rect(ls[0], fs[0], 0, 0, 64, 64);
    return kinds;
"#;

fn ordinary_on(name: &str, src: &str, mut e: Editor) {
    let (ms, out) = median_ms(&mut e, src, 3);
    report(name, "ms", ms, &out);
}

pub fn timeline_ordinary(name: &str) {
    ordinary_on(name, TIMELINE_ORDINARY, Editor::new(timeline_document()));
}

pub fn canvas_ordinary(name: &str) {
    ordinary_on(name, CANVAS_ORDINARY, Editor::new(canvas_document()));
}

pub fn map_ordinary(name: &str) {
    ordinary_on(name, MAP_ORDINARY, Editor::new(stress_map_document()));
}

/// Adding and removing a frame forever: each copies 256 layers' cel entries.
pub fn timeline_hostile(name: &str) {
    let src = "while true { let f = add_frame(0); remove_frame(f); }";
    hostile_on(name, src, Editor::new(timeline_document()));
}

/// The whole 4096 x 4096 canvas made one color, then flood filled forever: each fill pays
/// for every pixel first, and reaches every pixel.
pub fn canvas_hostile(name: &str) {
    let src = "let l = layers()[0]; let f = frames()[0];\n\
               fill_rect(l, f, 0, 0, 4096, 4096, 0xff000000); let c = 0xff000001;\n\
               while true { fill(l, f, 0, 0, c); c = c + 1; }";
    hostile_on(name, src, Editor::new(canvas_document()));
}

/// A pixel of the last frame of every layer, read forever: each read finds its layer among
/// 256 and its frame among 512.
pub fn timeline_reads_hostile(name: &str) {
    let src = "let ls = layers(); let f = frames()[511];\n\
               while true { for l in ls { get_pixel(l, f, 0, 0); } }";
    hostile_on(name, src, Editor::new(timeline_document()));
}

/// Reading every layer's kind and name, forever.
pub fn map_hostile(name: &str) {
    let src = "let ls = layers(); while true { for l in ls { layer_kind(l); layer_name(l); } }";
    hostile_on(name, src, Editor::new(stress_map_document()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_long_timeline_has_every_layer_and_its_drawings() {
        let doc = timeline_document();
        assert_eq!((doc.layers().len(), doc.frames.len()), (256, 512));
        // 16,384 drawings of one or two tiles each, not 131,072 cels of pixels.
        assert!(doc.held_bytes() < 64 << 20, "{}", doc.held_bytes());
    }
}
