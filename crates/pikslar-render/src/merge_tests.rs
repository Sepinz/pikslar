//! Merging two layers gives exactly the pixels the screen and export showed for them.

use crate::compose::Compositor;
use crate::flatten::flatten_document;
use pikslar_core::{BlendMode, Blending, Cel, ColorMode, Document, Editor, Grid, LayerCmd};

const SIDE: usize = 20;

/// Every alpha from clear to opaque against a spread of colours, different on each layer.
fn drawing(seed: u32) -> Cel {
    let mut px = vec![0u32; SIDE * SIDE];
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    for cell in px.iter_mut() {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let alpha = [0, 1, 37, 128, 200, 254, 255][(state >> 24) as usize % 7];
        *cell = if alpha == 0 {
            0
        } else {
            (state & 0x00ff_ffff) | (alpha << 24)
        };
    }
    Cel::new(Grid::from_cells(SIDE, SIDE, &px))
}

fn document(lower: (BlendMode, u8), upper: (BlendMode, u8)) -> Document {
    let mut doc = Document::new(SIDE, SIDE, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("layer");
    for (index, (blend, opacity)) in [lower, upper].into_iter().enumerate() {
        doc.put_cel(index, 0, drawing(index as u32 * 7 + opacity as u32))
            .expect("cel");
        let layer = &mut doc.layers_mut()[index];
        (layer.blend, layer.opacity) = (blend, opacity);
    }
    doc
}

/// What the screen holds and what export writes.
fn shown(doc: &Document) -> (Vec<u32>, Vec<u32>) {
    let mut comp = Compositor::new(SIDE, SIDE);
    comp.refresh_lut_from(&doc.colors, true);
    comp.compose_frame(doc, None, 0);
    let screen = comp.pixels().to_vec();
    (screen, flatten_document(doc, 0, &mut comp))
}

#[test]
fn a_merge_shows_what_the_two_layers_showed_for_every_mode_and_opacity() {
    merges_show_what_was_shown(Blending::Pikslar);
}

#[test]
fn a_merge_in_aseprite_blending_shows_what_the_two_layers_showed() {
    merges_show_what_was_shown(Blending::Aseprite);
}

fn merges_show_what_was_shown(blending: Blending) {
    for blend in BlendMode::ALL {
        for opacity in [1, 64, 128, 200, 255] {
            for lower in [
                (BlendMode::Normal, 255),
                (BlendMode::Multiply, 90),
                (blend, 170),
            ] {
                let mut doc = document(lower, (blend, opacity));
                doc.blending = blending;
                let before = shown(&doc);
                let mut e = Editor::new(doc);
                let layer = e.layer_id(1).expect("layer");
                e.apply(LayerCmd::MergeDown { layer }).expect("merged");
                assert_eq!(e.doc().layers().len(), 1);
                let after = shown(e.doc());
                assert!(
                    before.0 == after.0,
                    "screen, {blend:?} at {opacity} over {lower:?}"
                );
                assert!(
                    before.1 == after.1,
                    "export, {blend:?} at {opacity} over {lower:?}"
                );
            }
        }
    }
}
