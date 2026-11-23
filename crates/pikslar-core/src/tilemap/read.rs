//! Reading what a tilemap layer draws, for the tools that look at pixels rather than cells: the
//! wand, Select Opaque and Copy. They read through [`crate::Document::layer_tile`] and
//! [`crate::Document::layer_pixel`], so a map reads as the pixels its tiles show.

use crate::editor::Editor;
use crate::mask::Mask;
use crate::tile::{RawTile, TILE, TILE_AREA};

impl Editor {
    /// The pixel the active layer shows at a canvas position: through the tiles on a map.
    pub(crate) fn shown_pixel(&self, x: i32, y: i32) -> u32 {
        let layer = &self.doc.layers[self.layer];
        self.doc.layer_pixel(layer, self.frame, x, y)
    }

    /// Every canvas pixel the active map draws, read a storage tile at a time.
    pub(crate) fn opaque_map(&self) -> Mask {
        let (w, h) = (self.doc.width as i32, self.doc.height as i32);
        let mut mask = Mask::blank(0, 0, w, h);
        let layer = &self.doc.layers[self.layer];
        let mut raw: RawTile = [0; TILE_AREA];
        let side = TILE as i32;
        for ty in 0..(h + side - 1) / side {
            for tx in 0..(w + side - 1) / side {
                let drawn =
                    self.doc
                        .layer_tile(layer, self.frame, tx as usize, ty as usize, &mut raw);
                if !drawn {
                    continue;
                }
                for (i, _) in raw.iter().enumerate().filter(|(_, &v)| v != 0) {
                    let (x, y) = (tx * side + i as i32 % side, ty * side + i as i32 / side);
                    if x < w && y < h {
                        mask.set(x, y);
                    }
                }
            }
        }
        mask
    }

    /// The pixels the active map draws inside `shape`, row by row over its bounds, 0 outside it.
    pub(crate) fn read_selected_map(&self, shape: &Mask) -> Vec<u32> {
        let (x0, y0, w, h) = shape.bounds();
        let mut cells = vec![0u32; w.max(0) as usize * h.max(0) as usize];
        shape.for_each(|x, y| {
            cells[((y - y0) * w + (x - x0)) as usize] = self.shown_pixel(x, y);
        });
        cells
    }
}
