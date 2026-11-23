//! What an export draws: which layers, and at what scale. Every format takes a [`Picture`], so
//! the choice is made once, the same way, and its size is checked before any file is made.

use crate::ExportError;
use pikslar_core::{Document, MAX_DIMENSION};
use pikslar_render::BandSource;

/// The layers an export draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Layers {
    /// What the canvas shows: the visible layers.
    Visible,
    /// These layers, by position from the bottom, whether shown or hidden. Each keeps its own
    /// opacity and blend mode, and they are stacked in the document's order, not this list's.
    Only(Vec<usize>),
}

/// The layers with these names. A name must belong to exactly one layer.
#[inline(never)]
pub fn layers_named(doc: &Document, names: &[String]) -> Result<Layers, ExportError> {
    let mut chosen = Vec::with_capacity(names.len());
    for name in names {
        let mut matching = doc
            .layers()
            .iter()
            .enumerate()
            .filter(|(_, l)| l.name == *name);
        let (position, _) = matching
            .next()
            .ok_or_else(|| ExportError::NoSuchLayer(name.clone()))?;
        let others = matching.count();
        if others > 0 {
            return Err(ExportError::SameName {
                name: name.clone(),
                layers: others + 1,
            });
        }
        chosen.push(position);
    }
    Ok(Layers::Only(chosen))
}

/// One document's frames as an export draws them: its chosen layers, each pixel made a square
/// of `scale` by `scale`, nearest neighbor, after composition.
pub struct Picture<'a> {
    source: BandSource<'a>,
    scale: usize,
    indexed: bool,
}

impl<'a> Picture<'a> {
    /// The picture as the canvas shows it, at its own size.
    #[inline(never)]
    pub fn visible(doc: &'a Document) -> Self {
        Picture {
            source: BandSource::new(doc),
            scale: 1,
            indexed: false,
        }
    }

    /// `layers` of `doc` at `scale`, or why not. The scaled frame is checked against the
    /// largest image an export may make, so nothing is made that would be refused later.
    #[inline(never)]
    pub fn new(doc: &'a Document, layers: &Layers, scale: usize) -> Result<Self, ExportError> {
        check_size(doc.width, doc.height, scale)?;
        let source = match layers {
            Layers::Visible => BandSource::new(doc),
            Layers::Only(chosen) => {
                let count = doc.layers().len();
                if chosen.is_empty() {
                    return Err(ExportError::NoLayers);
                }
                if let Some(&layer) = chosen.iter().find(|&&l| l >= count) {
                    return Err(ExportError::NoLayerAt { layer, count });
                }
                BandSource::with_layers(doc, chosen)
            }
        };
        Ok(Picture {
            source,
            scale,
            indexed: false,
        })
    }

    /// The same picture, to be written as indexed PNGs if `indexed`, each with a palette of
    /// its own colours, or refused if it has more than a palette holds.
    pub fn with_indexed(self, indexed: bool) -> Self {
        Picture { indexed, ..self }
    }

    /// Whether its PNGs are indexed.
    pub fn indexed(&self) -> bool {
        self.indexed
    }

    /// The document drawn.
    pub fn doc(&self) -> &'a Document {
        self.source.doc()
    }

    /// How many exported pixels each document pixel becomes, across and down.
    pub fn scale(&self) -> usize {
        self.scale
    }

    /// A frame's width as exported.
    pub fn width(&self) -> usize {
        self.doc().width * self.scale
    }

    /// A frame's height as exported.
    pub fn height(&self) -> usize {
        self.doc().height * self.scale
    }

    /// How many bands of document rows a frame has.
    pub(crate) fn bands(&self) -> usize {
        self.source.bands()
    }

    /// Band `index` of `frame` at the document's own size.
    pub(crate) fn band(&self, frame: usize, index: usize, band: &mut Vec<u32>) {
        self.source.band(frame, index, band);
    }
}

/// Whether a `width` by `height` frame at `scale` is an image an export may make.
#[inline(never)]
pub fn check_size(width: usize, height: usize, scale: usize) -> Result<(), ExportError> {
    if scale == 0 {
        return Err(ExportError::NoScale);
    }
    let side = |n: usize| n as u128 * scale as u128;
    let (w, h) = (side(width), side(height));
    if w > MAX_DIMENSION as u128 || h > MAX_DIMENSION as u128 {
        return Err(ExportError::TooLarge {
            scale,
            width: w,
            height: h,
        });
    }
    Ok(())
}

/// Each of `cells` written `scale` times across `out`, which is `scale` times as long.
pub(crate) fn widen(cells: &[u32], scale: usize, out: &mut [u32]) {
    for (to, &cell) in out.chunks_exact_mut(scale).zip(cells) {
        to.fill(cell);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::{ColorMode, LayerCmd};

    fn named(names: &[&str]) -> Document {
        let mut e = pikslar_core::Editor::new(Document::new(4, 3, ColorMode::Rgba).expect("ok"));
        for at in 1..names.len() {
            e.apply(LayerCmd::Add {
                at,
                mode: ColorMode::Rgba,
            })
            .expect("room");
        }
        let mut doc = e.doc().clone();
        for (layer, name) in doc.layers_mut().iter_mut().zip(names) {
            layer.name = (*name).to_string();
        }
        doc
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn layers_are_found_by_name_and_a_shared_or_missing_name_is_refused() {
        let doc = named(&["ink", "color", "ink", "sky"]);
        let found = layers_named(&doc, &names(&["sky", "color"])).expect("found");
        assert_eq!(found, Layers::Only(vec![3, 1]));
        assert!(matches!(
            layers_named(&doc, &names(&["color", "Sky"])),
            Err(ExportError::NoSuchLayer(name)) if name == "Sky"
        ));
        assert!(matches!(
            layers_named(&doc, &names(&["ink"])),
            Err(ExportError::SameName { name, layers: 2 }) if name == "ink"
        ));
    }

    #[test]
    fn a_scale_is_refused_when_a_side_would_pass_the_limit() {
        check_size(8192, 8192, 1).expect("the largest document");
        check_size(1, 1, 8192).expect("one pixel made the largest image");
        check_size(2048, 1024, 4).expect("exactly the limit");
        for (w, h, scale) in [(2049, 1, 4), (1, 2731, 3), (8192, 8192, 2), (1, 1, 8193)] {
            assert!(
                matches!(check_size(w, h, scale), Err(ExportError::TooLarge { .. })),
                "{w}x{h} at {scale}"
            );
        }
        match check_size(8192, 3, usize::MAX) {
            Err(ExportError::TooLarge { width, height, .. }) => {
                assert_eq!(width, 8192 * usize::MAX as u128, "no overflow");
                assert_eq!(height, 3 * usize::MAX as u128);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(check_size(4, 4, 0), Err(ExportError::NoScale)));
        let said = check_size(2049, 3, 4).expect_err("too large").to_string();
        assert_eq!(
            said,
            "At 4x a frame would be 8196 by 12. An exported image may be at most 8192 a side"
        );
    }

    #[test]
    fn a_picture_names_what_it_cannot_draw() {
        let doc = named(&["a", "b"]);
        assert!(matches!(
            Picture::new(&doc, &Layers::Only(vec![]), 1),
            Err(ExportError::NoLayers)
        ));
        assert!(matches!(
            Picture::new(&doc, &Layers::Only(vec![0, 2]), 1),
            Err(ExportError::NoLayerAt { layer: 2, count: 2 })
        ));
        assert!(matches!(
            Picture::new(&doc, &Layers::Visible, 2049),
            Err(ExportError::TooLarge { scale: 2049, .. })
        ));
        let picture = Picture::new(&doc, &Layers::Only(vec![1]), 3).expect("drawn");
        assert_eq!(
            (picture.width(), picture.height(), picture.scale()),
            (12, 9, 3)
        );
        let plain = Picture::visible(&doc);
        assert_eq!((plain.width(), plain.height(), plain.scale()), (4, 3, 1));
    }

    #[test]
    fn widening_repeats_each_cell() {
        let mut out = [0u32; 9];
        widen(&[1, 2, 3], 3, &mut out);
        assert_eq!(out, [1, 1, 1, 2, 2, 2, 3, 3, 3]);
        let mut same = [0u32; 2];
        widen(&[7, 8], 1, &mut same);
        assert_eq!(same, [7, 8]);
    }
}
