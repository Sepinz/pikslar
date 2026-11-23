//! A frame flattened a band of rows at a time, so a large export never holds the whole image.

use crate::blend::unpremultiply;
use crate::compose::blend_chosen;
use crate::flatten::only_plain_layer;
use crate::pixel::{stored_color, Lut};
use pikslar_core::blend_straight::tidy;
use pikslar_core::tile::RawTile;
use pikslar_core::{Blending, ColorTable, Document, LayerKind, TILE, TILE_AREA};

/// Rows in every band but perhaps the last: one row of tiles.
pub const BAND_ROWS: usize = TILE;

/// The color table as the compositor reads it, built on the heap.
fn lut_for(table: &ColorTable) -> Box<Lut> {
    let mut lut: Box<Lut> = Box::new([0u32; 65536]);
    let colors = table.colors();
    lut[..colors.len()].copy_from_slice(colors);
    lut
}

/// Bands of any frame of one document, with its color table read once.
pub struct BandSource<'a> {
    doc: &'a Document,
    lut: Box<Lut>,
    plain: Option<usize>,
    /// Which layers are in the picture, by position from the bottom.
    chosen: Vec<bool>,
}

impl<'a> BandSource<'a> {
    /// The picture as it is exported: the visible layers, without reference layers.
    pub fn new(doc: &'a Document) -> Self {
        let chosen = doc.picture_layers();
        Self::of(doc, chosen)
    }

    /// The picture made of only `layers`, by position from the bottom, whether shown or hidden,
    /// each with its own opacity and blend mode, in the document's order. Positions the
    /// document does not have, reference layers, and groups, which have no pixels, are left out.
    /// A layer in a hidden group is drawn like any hidden layer chosen.
    pub fn with_layers(doc: &'a Document, layers: &[usize]) -> Self {
        let mut chosen = vec![false; doc.layers().len()];
        for &layer in layers {
            if let (Some(slot), Some(l)) = (chosen.get_mut(layer), doc.layers().get(layer)) {
                *slot = matches!(l.kind, LayerKind::Paint | LayerKind::Tilemap(_));
            }
        }
        Self::of(doc, chosen)
    }

    fn of(doc: &'a Document, chosen: Vec<bool>) -> Self {
        BandSource {
            doc,
            lut: lut_for(&doc.colors),
            plain: only_plain_layer(doc, |i, _| chosen[i]),
            chosen,
        }
    }

    pub fn doc(&self) -> &'a Document {
        self.doc
    }

    /// How many bands a frame has.
    pub fn bands(&self) -> usize {
        self.doc.height.div_ceil(BAND_ROWS)
    }

    /// Band `index` of `frame`, counted from the top, as straight RGBA: [`BAND_ROWS`] rows, or
    /// fewer for the last. The pixels are exactly those of [`crate::flatten_document`].
    pub fn band(&self, frame: usize, index: usize, band: &mut Vec<u32>) {
        let y0 = index * BAND_ROWS;
        let rows = BAND_ROWS.min(self.doc.height.saturating_sub(y0));
        match self.plain {
            Some(layer) => plain_band(band, self.doc, (layer, frame), (index, rows), &self.lut),
            None => composed_band(
                band,
                (self.doc, &self.chosen),
                frame,
                (index, rows),
                &self.lut,
            ),
        }
    }
}

/// Flattens one frame of a document to straight RGBA, handing `sink` one band at a time from
/// the top. Only one band is held. The first error from `sink` stops it.
pub fn flatten_bands<E>(
    doc: &Document,
    frame: usize,
    mut sink: impl FnMut(&[u32]) -> Result<(), E>,
) -> Result<(), E> {
    let source = BandSource::new(doc);
    let mut band = Vec::with_capacity(doc.width * BAND_ROWS.min(doc.height));
    for index in 0..source.bands() {
        source.band(frame, index, &mut band);
        sink(&band)?;
    }
    Ok(())
}

/// One row of tiles: `tile` fills canvas tile `tx` of the row, and `cell` makes each of its
/// cells straight RGBA.
fn band_of_tiles(
    band: &mut Vec<u32>,
    (width, rows): (usize, usize),
    mut tile: impl FnMut(usize, &mut RawTile),
    cell: impl Fn(u32) -> u32,
) {
    band.clear();
    band.resize(width * rows, 0);
    let mut out = [0u32; TILE_AREA];
    for tx in 0..width.div_ceil(TILE) {
        tile(tx, &mut out);
        let x0 = tx * TILE;
        let columns = TILE.min(width - x0);
        for y in 0..rows {
            let to = &mut band[y * width + x0..][..columns];
            for (p, &q) in to.iter_mut().zip(&out[y * TILE..][..columns]) {
                *p = cell(q);
            }
        }
    }
}

/// A single plain layer's stored colors, as [`crate::flatten_document`] reads them.
fn plain_band(
    band: &mut Vec<u32>,
    doc: &Document,
    (layer, frame): (usize, usize),
    (ty, rows): (usize, usize),
    lut: &Lut,
) {
    let layer = &doc.layers()[layer];
    let tile = |tx: usize, out: &mut RawTile| {
        if !doc.layer_tile(layer, frame, tx, ty, out) {
            out.fill(0);
        }
    };
    let color = |c: u32| stored_color(layer.mode, c, lut);
    // Aseprite-compatible blending keeps no colour where nothing shows, as it composites.
    match doc.blending {
        Blending::Pikslar => band_of_tiles(band, (doc.width, rows), tile, color),
        Blending::Aseprite => band_of_tiles(band, (doc.width, rows), tile, |c| tidy(color(c))),
    }
}

/// One row of tiles through the compositor's own blending, then made straight. Aseprite-
/// compatible blending builds it straight already.
fn composed_band(
    band: &mut Vec<u32>,
    (doc, chosen): (&Document, &[bool]),
    frame: usize,
    (ty, rows): (usize, usize),
    lut: &Lut,
) {
    let tiles_wide = doc.width.div_ceil(TILE);
    let tile = |tx: usize, out: &mut RawTile| {
        out.fill(0);
        let at = (tx, ty, ty * tiles_wide + tx);
        blend_chosen(out, (doc, frame), None, at, (lut, |i, _| chosen[i]), None);
    };
    match doc.blending {
        Blending::Pikslar => band_of_tiles(band, (doc.width, rows), tile, unpremultiply),
        Blending::Aseprite => band_of_tiles(band, (doc.width, rows), tile, |p| p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{flatten_document, Compositor};
    use pikslar_core::{rgba, BlendMode, ColorMode, DrawCmd, Editor, LayerCmd, Tool};

    fn joined(doc: &Document, frame: usize) -> (Vec<u32>, Vec<usize>) {
        let (mut px, mut sizes) = (Vec::new(), Vec::new());
        flatten_bands::<()>(doc, frame, |band| {
            sizes.push(band.len());
            px.extend_from_slice(band);
            Ok(())
        })
        .expect("no errors");
        (px, sizes)
    }

    fn paint(e: &mut Editor, points: &[(i32, i32)], color: u32) {
        e.color = color;
        let path = points.to_vec();
        e.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path,
        })
        .expect("stroke");
    }

    /// One layer with translucent pixels, so the shortcut runs.
    fn plain(width: usize, height: usize, mode: ColorMode) -> Editor {
        let mut e = Editor::new(Document::new(width, height, mode).expect("valid"));
        let (w, h) = (width as i32 - 1, height as i32 - 1);
        paint(&mut e, &[(0, 0), (w, h)], rgba(200, 40, 10, 255));
        paint(&mut e, &[(w, 0), (0, h)], rgba(10, 90, 200, 120));
        e
    }

    /// A translucent multiply layer over that one, so the compositor runs.
    fn layered(width: usize, height: usize, mode: ColorMode) -> Editor {
        let mut e = plain(width, height, mode);
        let h = height as i32 - 1;
        let w = width as i32 - 1;
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
            opacity: 140,
        })
        .expect("set");
        paint(&mut e, &[(0, h / 2), (w, h / 2)], rgba(0, 255, 255, 200));
        e
    }

    #[test]
    fn bands_join_into_the_flattened_frame() {
        for (w, h) in [(1, 1), (16, 16), (37, 23), (5, 70), (70, 33)] {
            for mode in [ColorMode::Rgba, ColorMode::Palette] {
                for e in [plain(w, h, mode), layered(w, h, mode)] {
                    let doc = e.doc();
                    let (px, sizes) = joined(doc, 0);
                    assert_eq!(px, flatten_document(doc, 0, &mut Compositor::new(1, 1)));
                    assert_eq!(sizes.len(), h.div_ceil(BAND_ROWS), "{w}x{h}");
                    assert!(sizes[..sizes.len() - 1].iter().all(|&s| s == w * BAND_ROWS));
                }
            }
        }
    }

    #[test]
    fn the_first_error_stops_the_bands() {
        let e = layered(20, 50, ColorMode::Rgba);
        let mut seen = 0;
        let result = flatten_bands(e.doc(), 0, |_| {
            seen += 1;
            if seen == 2 {
                Err("full")
            } else {
                Ok(())
            }
        });
        assert_eq!((result, seen), (Err("full"), 2));
    }

    /// Layers of every kind stacked: a translucent normal layer, a multiply layer at half
    /// opacity, a hidden opaque multiply layer, and an indexed one.
    fn stacked() -> Editor {
        let mut e = plain(37, 23, ColorMode::Rgba);
        let kinds = [
            (BlendMode::Multiply, 128, ColorMode::Rgba),
            (BlendMode::Multiply, 255, ColorMode::Rgba),
            (BlendMode::Normal, 200, ColorMode::Palette),
        ];
        for (at, (blend, opacity, mode)) in (1..).zip(kinds) {
            e.apply(LayerCmd::Add { at, mode }).expect("room");
            e.layer = at;
            let layer = e.layer_id(at).expect("layer");
            e.apply(LayerCmd::SetBlend { layer, blend }).expect("blend");
            e.apply(LayerCmd::SetOpacity { layer, opacity })
                .expect("opacity");
            paint(
                &mut e,
                &[(3, 20), (33, 2 + at as i32)],
                rgba(90, 200, 60, 180),
            );
            paint(
                &mut e,
                &[(0, 11), (36, 9 + at as i32)],
                rgba(250, 250, 20, 255),
            );
        }
        let layer = e.layer_id(2).expect("layer");
        e.apply(LayerCmd::SetVisible {
            layer,
            visible: false,
        })
        .expect("hide");
        e
    }

    /// What the canvas shows when exactly `layers` are visible.
    fn shown(doc: &Document, layers: &[usize]) -> Vec<u32> {
        let mut only = doc.clone();
        for (i, layer) in only.layers_mut().iter_mut().enumerate() {
            layer.visible = layers.contains(&i);
        }
        flatten_document(&only, 0, &mut Compositor::new(doc.width, doc.height))
    }

    fn from(source: &BandSource<'_>) -> Vec<u32> {
        let mut px = Vec::new();
        let mut band = Vec::new();
        for index in 0..source.bands() {
            source.band(0, index, &mut band);
            px.extend_from_slice(&band);
        }
        px
    }

    #[test]
    fn chosen_layers_are_the_picture_with_only_them_shown() {
        let e = stacked();
        let doc = e.doc();
        assert_eq!(from(&BandSource::new(doc)), shown(doc, &[0, 1, 3]));
        for layers in [
            &[0][..],
            &[1],
            &[2],
            &[3],
            &[0, 2],
            &[1, 3],
            &[0, 1, 2, 3],
            &[3, 0],
        ] {
            let source = BandSource::with_layers(doc, layers);
            assert_eq!(from(&source), shown(doc, layers), "{layers:?}");
        }
        let none = BandSource::with_layers(doc, &[9]);
        assert!(
            from(&none).iter().all(|&p| p == 0),
            "a missing layer adds nothing"
        );
        assert_ne!(
            shown(doc, &[0, 2]),
            shown(doc, &[0]),
            "the hidden layer is drawn"
        );
    }
}
