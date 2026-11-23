//! The cost of Aseprite-compatible blending: one 1024 by 1024 frame of four translucent layers in
//! modes both blendings have, composited whole in each blending, and the same frame with a hue
//! layer, which only Aseprite-compatible blending draws as itself.

use crate::gate::{Kind, Metric};
use crate::workloads::{best_of, push};
use pikslar_core::{rgba, BlendMode, Blending, ColorMode, Damage, Document, Editor, Grid};
use pikslar_render::Compositor;
use std::hint::black_box;

const SIDE: usize = 1024;
pub(crate) const MODES: [BlendMode; 4] = [
    BlendMode::Normal,
    BlendMode::Multiply,
    BlendMode::Overlay,
    BlendMode::Screen,
];

/// A layer of soft stripes with alpha that changes across it, a few pixels clear.
fn layer_cells(layer: usize) -> Vec<u32> {
    (0..SIDE * SIDE)
        .map(|i| {
            let (x, y) = (i % SIDE, i / SIDE);
            let v = (x * 3 + y * 5 + layer * 61) as u32;
            let alpha = if (x + layer * 7) % 29 == 0 {
                0
            } else {
                96 + v % 160
            };
            rgba(v as u8, (v >> 2) as u8, (v >> 4) as u8, alpha as u8)
        })
        .collect()
}

pub(crate) fn document(blending: Blending, top: BlendMode) -> Document {
    let mut doc = Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid size");
    for (layer, blend) in MODES.iter().enumerate() {
        if layer > 0 {
            doc.add_layer(layer, ColorMode::Rgba).expect("room");
        }
        let grid = Grid::from_cells(SIDE, SIDE, &layer_cells(layer));
        doc.set_cel(layer, 0, grid).expect("canvas size");
        doc.layers_mut()[layer].blend = *blend;
    }
    doc.layers_mut()[MODES.len() - 1].blend = top;
    doc.blending = blending;
    doc
}

fn composite(doc: Document, name: &str, out: &mut Vec<Metric>) {
    let editor = Editor::new(doc);
    let mut comp = Compositor::new(SIDE, SIDE);
    let mut changed = Vec::new();
    let time = best_of(|| {
        changed.clear();
        comp.update(&editor, Damage::All, &mut changed);
        black_box(comp.pixels());
    });
    push(out, name, Kind::Micros, time);
}

pub fn measure(out: &mut Vec<Metric>) {
    let top = MODES[MODES.len() - 1];
    composite(
        document(Blending::Pikslar, top),
        "blend_compose_1024_pikslar",
        out,
    );
    composite(
        document(Blending::Aseprite, top),
        "blend_compose_1024_aseprite",
        out,
    );
    composite(
        document(Blending::Aseprite, BlendMode::Hue),
        "blend_compose_1024_aseprite_hue",
        out,
    );
}
