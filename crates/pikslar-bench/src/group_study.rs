//! `pikslar-bench group-study`: what layer groups cost on the stress document, to decide whether
//! group opacity and blend (phase 6.5b) fit the compositor's budget. Groups as built in 6.5a
//! composite exactly as the same layers flat; the extra blends stand in for what a group drawn
//! on its own would add, one per group on every tile. Stays out of the gate's baseline.

use crate::stress::stress_document;
use pikslar_core::{Cel, ColorMode, Damage, Document, Editor, LayerCmd};
use pikslar_render::Compositor;
use std::time::Instant;

const ROUNDS: usize = 41;

fn report(name: &str, value: f64, unit: &str) {
    println!("{name:<48} {value:>12.3}  {unit}");
}

/// The median time of a whole-canvas composite, in milliseconds.
fn composite_ms(editor: &Editor) -> f64 {
    let doc = editor.doc();
    let mut comp = Compositor::new(doc.width, doc.height);
    let mut changed = Vec::new();
    let mut times: Vec<f64> = (0..ROUNDS)
        .map(|_| {
            let start = Instant::now();
            comp.update(editor, Damage::All, &mut changed);
            changed.clear();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[ROUNDS / 2]
}

/// The stress document with its layers in groups of three.
fn grouped(doc: Document) -> (Editor, usize) {
    let mut e = Editor::new(doc);
    let (mut groups, mut index) = (0, 0);
    while index < e.doc().layers().len() {
        let layer = e.layer_id(index).expect("layer");
        let group = e.apply(LayerCmd::Group { layer }).map(|_| e.layer);
        let mut group = group.expect("grouped");
        groups += 1;
        // Two more layers above go in by stepping down into the new group.
        for _ in 0..2 {
            let Some(layer) = e.layer_id(group + 1) else {
                break;
            };
            e.apply(LayerCmd::Step { layer, up: false })
                .expect("stepped in");
            group += 1;
        }
        index = group + 1;
    }
    (e, groups)
}

/// The stress document with `extra` full opaque layers on top: one blend each, per tile.
fn with_extra_blends(mut doc: Document, extra: usize) -> Editor {
    let cells = vec![0xff40_8020u32; doc.width * doc.height];
    for _ in 0..extra {
        let at = doc.layers().len();
        doc.add_layer(at, ColorMode::Rgba).expect("room");
        let grid = pikslar_core::Grid::from_cells(doc.width, doc.height, &cells);
        doc.set_cel(at, 0, grid).expect("cel");
    }
    Editor::new(doc)
}

pub fn run() {
    let flat = Editor::new(stress_document());
    let layers = flat.doc().layers().len();
    report("stress layers", layers as f64, "layers");
    let flat_ms = composite_ms(&flat);
    report("composite, flat", flat_ms, "ms");
    let (groups, count) = grouped(stress_document());
    report("groups", count as f64, "groups");
    let max_depth = groups.doc().layers().iter().map(|l| l.depth).max();
    report("deepest layer", f64::from(max_depth.unwrap_or(0)), "groups");
    let grouped_ms = composite_ms(&groups);
    report("composite, in groups (6.5a)", grouped_ms, "ms");
    let same = groups.doc().picture_layers().iter().filter(|&&p| p).count() == layers;
    report(
        "every paint layer still drawn",
        f64::from(u8::from(same)),
        "1 = yes",
    );
    let isolated_ms = composite_ms(&with_extra_blends(stress_document(), count));
    report(
        "composite, one blend more per group (6.5b)",
        isolated_ms,
        "ms",
    );
    let frames = groups.doc().frames.len();
    let per_group = frames * std::mem::size_of::<Cel>();
    report(
        "memory per group, its blank cels",
        per_group as f64 / 1024.0,
        "KiB",
    );
    let saved = |d: &Document| pikslar_format::piks::save(d).len() as f64 / 1024.0;
    report("saved, flat", saved(flat.doc()), "KiB");
    report("saved, in groups", saved(groups.doc()), "KiB");
}
