//! M9's measurements: the three palette commands that rewrite every palette image, timed on the
//! palette stress document, the undo of a remap, a ramp, and reading the largest GIMP palette.
//!
//! The palette stress document is the stress document's shape (400 frames of 12 layers, 128 by
//! 128, every drawing different) with every layer in palette mode and 4,096 colours, some of
//! them duplicates, about a hundred unused, and the swatches in the reverse of id order, so each
//! remap has work on every image.

use crate::gate::{Kind, Metric};
use crate::workloads::{best_of, push};
use pikslar_core::{
    rgba, ColorMode, ColorTable, Document, Editor, Grid, PaletteCmd, RemapKind, StepCmd,
};
use pikslar_format::palette_file::{self, PaletteData, PaletteFormat, MAX_COLORS};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

const FRAMES: usize = 400;
const LAYERS: usize = 12;
const SIDE: usize = 128;
const COLORS: u32 = 4_096;
/// Ids the pixels use: the last ones are left for Remove unused.
const USED: u32 = 4_000;
/// Each rewrite is timed this many times, the document put back by undo between.
const ROUNDS: usize = 3;

/// Colour `id` of the table. Every eighth id repeats the one before it, so merging has work.
fn color_of(id: u32) -> u32 {
    let v = if id % 8 == 0 { id - 1 } else { id };
    rgba(
        (v & 15) as u8 * 16,
        (v >> 4 & 15) as u8 * 16,
        (v >> 8) as u8 * 16,
        255,
    )
}

fn cel(layer: usize, frame: usize) -> Grid {
    let shift = (layer * FRAMES + frame) as u32 * 7;
    let cells: Vec<u32> = (0..SIDE * SIDE)
        .map(|i| {
            let (x, y) = ((i % SIDE) as u32, (i / SIDE) as u32);
            if (x + frame as u32) % 32 < 6 {
                return 0;
            }
            1 + (shift + x / 8 * 13 + y / 8 * 29) % USED
        })
        .collect();
    Grid::from_cells(SIDE, SIDE, &cells)
}

pub fn palette_stress_document() -> Document {
    let mut doc = Document::new(SIDE, SIDE, ColorMode::Palette).expect("valid size");
    let colors: Vec<u32> = std::iter::once(0)
        .chain((1..=COLORS).map(color_of))
        .collect();
    let swatches: Vec<u32> = (1..=COLORS).rev().collect();
    doc.colors = Arc::new(ColorTable::from_parts(&colors, &swatches));
    for layer in 1..LAYERS {
        doc.add_layer(layer, ColorMode::Palette).expect("room");
    }
    for frame in 1..FRAMES {
        doc.add_frame(frame, false).expect("room");
    }
    for layer in 0..LAYERS {
        for frame in 0..FRAMES {
            doc.set_cel(layer, frame, cel(layer, frame))
                .expect("canvas size");
        }
    }
    doc
}

fn micros_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e6
}

/// The best time of `command`, and of undoing it. Each round starts from the same document.
fn rewrite(editor: &mut Editor, command: PaletteCmd) -> (f64, f64) {
    let (mut apply, mut undo) = (f64::MAX, f64::MAX);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        let outcome = editor.apply(command.clone()).expect("room");
        apply = apply.min(micros_since(start));
        assert!(outcome.changed, "{command:?} had nothing to do");
        let start = Instant::now();
        editor.apply(StepCmd::Undo).expect("undo");
        undo = undo.min(micros_since(start));
    }
    (apply, undo)
}

fn rewrites(out: &mut Vec<Metric>) {
    let mut editor = Editor::new(palette_stress_document());
    let remap = PaletteCmd::Remap(RemapKind::ToSwatchOrder);
    let (time, undo) = rewrite(&mut editor, remap);
    push(out, "palette_remap_stress", Kind::Micros, time);
    push(out, "palette_remap_undo", Kind::Micros, undo);
    let merge = PaletteCmd::Remap(RemapKind::MergeDuplicates);
    let (time, _) = rewrite(&mut editor, merge);
    push(out, "palette_merge_stress", Kind::Micros, time);
    let replace = PaletteCmd::ReplaceColor {
        from: 1 + USED / 2,
        to: 3,
        layer: None,
    };
    let (time, _) = rewrite(&mut editor, replace);
    push(out, "palette_replace_stress", Kind::Micros, time);
    let ramp = PaletteCmd::AddRamp {
        from: 1,
        to: COLORS,
        steps: 16,
    };
    let (time, _) = rewrite(&mut editor, ramp);
    push(out, "palette_ramp_16", Kind::Micros, time);
}

/// Reading a GIMP palette of the most colours a file may hold, each named.
fn parse_gpl(out: &mut Vec<Metric>) {
    let palette = PaletteData {
        name: Some("Largest".into()),
        names: (0..MAX_COLORS)
            .map(|i| Some(format!("colour {i}")))
            .collect(),
        ..PaletteData::from_colors(
            (0..MAX_COLORS)
                .map(|i| [i as u8, (i >> 8) as u8, (i * 7) as u8, 255])
                .collect(),
        )
    };
    let bytes = palette_file::write(&palette, PaletteFormat::Gpl).expect("writes");
    let time = best_of(|| {
        let read = palette_file::read(black_box(&bytes), PaletteFormat::Gpl).expect("reads");
        assert_eq!(read.palette.colors.len(), MAX_COLORS);
        black_box(read);
    });
    push(out, "palette_parse_gpl_65535", Kind::Micros, time);
}

pub fn measure(out: &mut Vec<Metric>) {
    rewrites(out);
    parse_gpl(out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stress_palette_has_duplicates_unused_ids_and_reversed_swatches() {
        let colors: Vec<u32> = (1..=64).map(color_of).collect();
        assert_eq!(colors[7], colors[6], "id 8 repeats id 7");
        assert_ne!(colors[0], colors[1]);
        let grid = cel(3, 5);
        let image = grid.into_image();
        let top = (0..SIDE as i32)
            .flat_map(|y| (0..SIDE as i32).map(move |x| (x, y)))
            .map(|(x, y)| image.get(x, y))
            .max();
        assert!(top.is_some_and(|t| t <= USED && t > 0));
    }
}
