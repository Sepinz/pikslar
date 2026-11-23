//! One frame as a finished image, which is what every export starts from.

use crate::blend::unpremultiply;
use crate::compose::{Compositor, InProgress};
use crate::pixel::stored_color;
use pikslar_core::blend_straight::tidy;
use pikslar_core::{Blending, Document, Editor, Layer, TILE, TILE_AREA};

/// The layer whose stored colors already are the picture, if there is just one.
pub(crate) fn single_plain_layer(doc: &Document) -> Option<usize> {
    let picture = doc.picture_layers();
    only_plain_layer(doc, |i, _| picture[i])
}

/// The layer whose stored colors already are the picture, if `include` takes just that one.
pub(crate) fn only_plain_layer(
    doc: &Document,
    include: impl Fn(usize, &Layer) -> bool,
) -> Option<usize> {
    let layers = doc.layers();
    let mut taken = layers.iter().enumerate().filter(|(i, l)| include(*i, l));
    let (index, layer) = taken.next()?;
    (taken.next().is_none() && layer.opacity == 255).then_some(index)
}

/// Flattens one frame to straight RGBA, as the canvas shows it without onion skin.
///
/// Floating pixels and a stroke in progress show on the frame being edited, as on the canvas.
pub fn flatten(editor: &mut Editor, frame: usize, comp: &mut Compositor) -> Vec<u32> {
    editor.settle_floating();
    let over = editor.overlay().filter(|_| frame == editor.frame);
    flatten_with(editor.doc(), over, frame, comp)
}

/// Flattens one frame of a document on its own, with nothing in progress. It needs no editor,
/// so a program without a window can export.
pub fn flatten_document(doc: &Document, frame: usize, comp: &mut Compositor) -> Vec<u32> {
    flatten_with(doc, None, frame, comp)
}

/// The layer's stored cells over the canvas, top to bottom: a paint layer's drawing as it is,
/// a tilemap layer's tiles through [`Document::layer_tile`].
fn layer_cells(doc: &Document, layer: &Layer, frame: usize) -> Vec<u32> {
    let (width, height) = (doc.width, doc.height);
    if !layer.is_tilemap() {
        return layer.cel(frame).to_cells(width, height);
    }
    let mut cells = vec![0u32; width * height];
    let mut raw = [0u32; TILE_AREA];
    for ty in 0..height.div_ceil(TILE) {
        for tx in 0..width.div_ceil(TILE) {
            if !doc.layer_tile(layer, frame, tx, ty, &mut raw) {
                continue;
            }
            let (x0, y0) = (tx * TILE, ty * TILE);
            let columns = TILE.min(width - x0);
            for y in 0..TILE.min(height - y0) {
                let to = (y0 + y) * width + x0;
                cells[to..to + columns].copy_from_slice(&raw[y * TILE..][..columns]);
            }
        }
    }
    cells
}

/// A single plain layer is read straight from its stored colors, because converting to
/// premultiplied and back would lose precision in translucent pixels. Everything else goes
/// through the compositor, which leaves Aseprite-compatible blending's picture straight.
fn flatten_with(
    doc: &Document,
    over: InProgress<'_>,
    frame: usize,
    comp: &mut Compositor,
) -> Vec<u32> {
    let (width, height) = (doc.width, doc.height);
    if comp.width() != width || comp.height() != height {
        *comp = Compositor::new(width, height);
    }
    comp.refresh_lut_from(&doc.colors, true);
    if let Some(index) = single_plain_layer(doc).filter(|_| over.is_none()) {
        let layer = &doc.layers()[index];
        let lut = &comp.lut;
        let cells = layer_cells(doc, layer, frame).into_iter();
        let colors = cells.map(|c| stored_color(layer.mode, c, lut));
        // Aseprite-compatible blending keeps no colour where nothing shows, as it composites.
        return match doc.blending {
            Blending::Pikslar => colors.collect(),
            Blending::Aseprite => colors.map(tidy).collect(),
        };
    }
    comp.compose_frame(doc, over, frame);
    match doc.blending {
        Blending::Pikslar => comp.pixels().iter().map(|&p| unpremultiply(p)).collect(),
        Blending::Aseprite => comp.pixels().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::{rgba, BlendMode, ColorMode, DrawCmd, LayerCmd, Tool};

    fn dot(e: &mut Editor, x: i32, color: u32, commit: bool) {
        e.color = color;
        e.apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x,
            y: 0,
        })
        .expect("begin");
        if commit {
            e.apply(DrawCmd::Commit).expect("commit");
        }
    }

    /// Two layers, one translucent multiply, so the compositor runs and not the shortcut.
    fn stacked() -> Editor {
        let mut e = Editor::new(Document::new(4, 1, ColorMode::Rgba).expect("valid"));
        dot(&mut e, 1, rgba(200, 40, 10, 255), true);
        e.apply(LayerCmd::Add {
            at: 1,
            mode: ColorMode::Palette,
        })
        .expect("room");
        e.layer = 1;
        let id = e.layer_id(1).expect("layer");
        e.apply(LayerCmd::SetBlend {
            layer: id,
            blend: BlendMode::Multiply,
        })
        .expect("set");
        e.apply(LayerCmd::SetOpacity {
            layer: id,
            opacity: 128,
        })
        .expect("set");
        dot(&mut e, 1, rgba(0, 255, 255, 255), true);
        e
    }

    /// A tilemap layer full of cells, over a picture. Its cells are tile numbers, not colours.
    fn with_map(doc: &mut Document) {
        let set = doc
            .add_tileset("T", ColorMode::Rgba, (2, 2))
            .expect("tileset");
        let image = pikslar_core::Grid::from_cells(2, 2, &[rgba(1, 2, 3, 255); 4]).into_image();
        doc.add_tile(set, std::sync::Arc::new(image)).expect("tile");
        doc.add_tilemap_layer(1, set).expect("layer");
        let cells = [1u32; 2];
        let grid = pikslar_core::Grid::from_cells(2, 1, &cells);
        doc.put_cel(1, 0, pikslar_core::Cel::new(grid))
            .expect("cel");
    }

    /// A map layer draws the tiles its cells place, not its cell values, in the picture and on
    /// the screen. Alone it is read straight from its tiles' stored colours, so a translucent
    /// colour comes out exactly.
    #[test]
    fn a_tilemap_layer_draws_its_tiles_in_the_picture_and_on_the_screen() {
        let mut doc = Document::new(4, 1, ColorMode::Rgba).expect("valid");
        with_map(&mut doc);
        doc.layers_mut()[0].visible = false;
        assert!(doc.layers()[1].is_tilemap());
        let color = rgba(1, 2, 3, 255);
        let flat = flatten_document(&doc, 0, &mut Compositor::new(4, 1));
        assert_eq!(flat, [color; 4]);
        let mut e = Editor::new(doc);
        let mut comp = Compositor::new(4, 1);
        let damage = e.take_damage();
        comp.update(&e, damage, &mut Vec::new());
        assert_eq!(comp.pixels(), [0xff01_0203; 4]);
    }

    #[test]
    fn a_document_flattens_as_its_editor_does() {
        for mut e in [
            stacked(),
            Editor::new(Document::new(4, 1, ColorMode::Palette).expect("ok")),
        ] {
            let mut comp = Compositor::new(1, 1);
            let alone = flatten_document(e.doc(), 0, &mut comp);
            assert_eq!(alone, flatten(&mut e, 0, &mut comp));
        }
    }

    #[test]
    fn only_the_editor_shows_a_stroke_in_progress_and_only_on_its_frame() {
        let mut e = stacked();
        e.apply(pikslar_core::FrameCmd::Add { at: 1, copy: true })
            .expect("room");
        dot(&mut e, 3, rgba(0, 0, 255, 255), false);
        let mut comp = Compositor::new(4, 1);
        let frame = e.frame;
        assert_ne!(flatten(&mut e, frame, &mut comp)[3], 0, "the stroke shows");
        assert_eq!(flatten_document(e.doc(), frame, &mut comp)[3], 0);
        let other = 1 - frame;
        assert_eq!(
            flatten(&mut e, other, &mut comp)[3],
            0,
            "not on another frame"
        );
    }

    #[test]
    fn a_single_layer_shows_its_stroke_in_progress_too() {
        let mut e = Editor::new(Document::new(4, 1, ColorMode::Rgba).expect("valid"));
        dot(&mut e, 2, rgba(0, 0, 255, 255), false);
        let mut comp = Compositor::new(4, 1);
        assert_eq!(flatten(&mut e, 0, &mut comp)[2], rgba(0, 0, 255, 255));
        assert_eq!(flatten_document(e.doc(), 0, &mut comp)[2], 0);
    }

    #[test]
    fn a_compositor_used_for_another_document_takes_that_documents_colors() {
        let painted = |color| {
            let mut e = Editor::new(Document::new(4, 1, ColorMode::Palette).expect("valid"));
            dot(&mut e, 0, color, true);
            e.doc().clone()
        };
        let (red, blue) = (painted(rgba(255, 0, 0, 255)), painted(rgba(0, 0, 255, 255)));
        assert_eq!(
            red.colors.version(),
            blue.colors.version(),
            "the case that matters"
        );
        let mut comp = Compositor::new(4, 1);
        assert_eq!(
            flatten_document(&red, 0, &mut comp)[0],
            rgba(255, 0, 0, 255)
        );
        assert_eq!(
            flatten_document(&blue, 0, &mut comp)[0],
            rgba(0, 0, 255, 255)
        );
    }
}
