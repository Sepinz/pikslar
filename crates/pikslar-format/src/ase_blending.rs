//! Whether an imported Aseprite file needs Aseprite-compatible blending to look as Aseprite
//! shows it.
//!
//! The rule, in `docs/ase-compatibility.md`: a file needs it when a paint or tilemap layer,
//! shown or hidden, has a mode other than Normal or an opacity below full, or when two or more
//! such layers hold any translucent pixel. Without these, every pixel is an opaque colour laid
//! over the ones below or nothing, which both blendings draw the same, and the file keeps
//! Pikslar's own blending. Cel opacity is in the pixels' alpha by now, so it counts as
//! translucent pixels.

use pikslar_core::tile::Tile;
use pikslar_core::{BlendMode, Blending, ColorMode, Document, Image, Layer, LayerKind};
use std::collections::HashSet;

use crate::FormatError;

/// How many tiles of pixels are looked at between asking whether to stop: a mebibyte of pixels.
const STOP_EVERY_TILES: usize = 4096;

/// The blending an imported document is given. `stop` is asked as the pixels are looked at; a
/// true answer ends it with [`FormatError::Stopped`].
pub(crate) fn blending_for(
    doc: &Document,
    stop: &dyn Fn() -> bool,
) -> Result<Blending, FormatError> {
    let drawn: Vec<&Layer> = doc
        .layers()
        .iter()
        .filter(|l| matches!(l.kind, LayerKind::Paint | LayerKind::Tilemap(_)))
        .collect();
    let styled = drawn
        .iter()
        .any(|l| l.blend != BlendMode::Normal || l.opacity < u8::MAX);
    if styled {
        return Ok(Blending::Aseprite);
    }
    if drawn.len() > 1 {
        let mut scan = Scan::new(stop);
        for layer in drawn {
            if scan.layer(doc, layer)? {
                return Ok(Blending::Aseprite);
            }
        }
    }
    Ok(Blending::Pikslar)
}

/// The search for a translucent pixel. Each tileset and each image is looked at once for each
/// way of reading its values, however many layers show it: the search ends at the first
/// translucent pixel, so all it has seen is opaque or clear.
struct Scan<'s> {
    stop: &'s dyn Fn() -> bool,
    /// Tiles of pixels looked at so far.
    looked: usize,
    /// Tileset numbers and image ids seen, one set for each way of reading values (indexed
    /// at 1): sets of plain numbers, a kind of set the program has anyway.
    tilesets: [HashSet<u32>; 2],
    images: [HashSet<usize>; 2],
}

impl<'s> Scan<'s> {
    fn new(stop: &'s dyn Fn() -> bool) -> Self {
        Scan {
            stop,
            looked: 0,
            tilesets: Default::default(),
            images: Default::default(),
        }
    }

    /// True if any pixel the layer can show is neither clear nor opaque: its drawings, or for
    /// a tilemap layer its tileset's tiles.
    fn layer(&mut self, doc: &Document, layer: &Layer) -> Result<bool, FormatError> {
        let indexed = layer.mode == ColorMode::Palette;
        let color = |cell: u32| match indexed {
            false => cell,
            true => doc.colors.color(cell & 0xffff),
        };
        if let Some(id) = layer.tileset() {
            let Ok(set) = doc.tileset(id) else {
                return Ok(false);
            };
            if !self.tilesets[usize::from(indexed)].insert(id.get()) {
                return Ok(false);
            }
            for entry in set.tiles() {
                if self.image(&entry.image, color)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        for cel in layer.cels() {
            if self.images[usize::from(indexed)].insert(cel.image_id())
                && self.image(cel.image(), color)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn image(&mut self, image: &Image, color: impl Fn(u32) -> u32) -> Result<bool, FormatError> {
        let (x0, y0, wide, high) = image.table();
        let translucent = |c: &u32| !matches!(color(*c) >> 24, 0 | 255);
        for ty in y0..y0 + high as i32 {
            for tx in x0..x0 + wide as i32 {
                self.looked += 1;
                if self.looked % STOP_EVERY_TILES == 0 && (self.stop)() {
                    return Err(FormatError::Stopped);
                }
                // A packed tile's list holds the values it uses (or, from elsewhere, more,
                // which only errs towards Aseprite blending), so no tile is decoded.
                let found = match image.tile(tx, ty) {
                    None => false,
                    Some(Tile::Solid(v)) => translucent(v),
                    Some(Tile::Packed { palette, .. }) => palette.iter().any(translucent),
                    Some(Tile::Raw(raw)) => raw.iter().any(translucent),
                };
                if found {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::{rgba, Cel, Grid};
    use std::cell::Cell;

    fn two_layers(top: u32) -> Document {
        let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
        doc.add_layer(1, ColorMode::Rgba).expect("room");
        let cells = |c: u32| Cel::new(Grid::from_cells(4, 4, &[c; 16]));
        doc.put_cel(0, 0, cells(rgba(10, 20, 30, 255)))
            .expect("cel");
        doc.put_cel(1, 0, cells(top)).expect("cel");
        doc
    }

    fn unstopped(doc: &Document) -> Blending {
        blending_for(doc, &|| false).expect("not stopped")
    }

    /// `layers` layers of a `side` pixel square document that all show one opaque cel.
    fn sharing(side: usize, layers: usize) -> Document {
        let mut doc = Document::new(side, side, ColorMode::Rgba).expect("valid");
        let cel = Cel::new(Grid::from_cells(
            side,
            side,
            &vec![rgba(5, 6, 7, 255); side * side],
        ));
        for n in 0..layers {
            if n > 0 {
                doc.add_layer(n, ColorMode::Rgba).expect("room");
            }
            doc.put_cel(n, 0, cel.clone()).expect("cel");
        }
        doc
    }

    #[test]
    fn opaque_normal_layers_keep_pikslars_blending() {
        assert_eq!(
            unstopped(&two_layers(rgba(1, 2, 3, 255))),
            Blending::Pikslar
        );
    }

    #[test]
    fn a_mode_an_opacity_or_a_translucent_pixel_over_another_layer_needs_aseprites() {
        assert_eq!(
            unstopped(&two_layers(rgba(1, 2, 3, 128))),
            Blending::Aseprite
        );
        let mut doc = two_layers(rgba(1, 2, 3, 255));
        doc.layers_mut()[0].blend = BlendMode::Hue;
        doc.layers_mut()[0].visible = false;
        assert_eq!(unstopped(&doc), Blending::Aseprite, "hidden layers count");
        let mut doc = two_layers(rgba(1, 2, 3, 255));
        doc.layers_mut()[1].opacity = 254;
        assert_eq!(unstopped(&doc), Blending::Aseprite);
    }

    #[test]
    fn one_layer_of_translucent_pixels_draws_the_same_either_way() {
        let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
        let cel = Cel::new(Grid::from_cells(4, 4, &[rgba(9, 9, 9, 100); 16]));
        doc.put_cel(0, 0, cel).expect("cel");
        assert_eq!(unstopped(&doc), Blending::Pikslar);
    }

    #[test]
    fn an_image_many_layers_show_is_looked_at_once() {
        let doc = sharing(64, 3);
        let never = || false;
        let mut scan = Scan::new(&never);
        for layer in doc.layers() {
            assert!(matches!(scan.layer(&doc, layer), Ok(false)));
        }
        assert_eq!(scan.looked, 16, "the 16 tiles of one image, not 48");
        assert_eq!(unstopped(&doc), Blending::Pikslar);
    }

    #[test]
    fn looking_asks_whether_to_stop_every_mebibyte_of_pixels() {
        // 66 by 66 tiles of 16 pixels: more than 4,096 of them.
        let doc = sharing(1056, 2);
        let asked = Cell::new(0);
        let count = || {
            asked.set(asked.get() + 1);
            false
        };
        assert!(matches!(blending_for(&doc, &count), Ok(Blending::Pikslar)));
        assert_eq!(asked.get(), 66 * 66 / STOP_EVERY_TILES);
        let stop = || true;
        assert!(matches!(
            blending_for(&doc, &stop),
            Err(FormatError::Stopped)
        ));
    }
}
