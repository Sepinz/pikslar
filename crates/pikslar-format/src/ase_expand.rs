//! Test helper: draws the map of a tilemap layer as pixels, cell by cell, so a test can compare
//! what an import made with a picture. It reads the document through its public API only and is
//! also included by `tests/ase_real.rs`, which is why it names the crate by its own name.

#![allow(dead_code)]

use pikslar_core::tilemap::MapCell;
use pikslar_core::Document;

/// The canvas of the tilemap layer `layer` at `frame`, `doc.width` by `doc.height` values, from
/// the top: the value of each tile pixel (a color, or a color table place in a palette
/// document), 0 where no tile is drawn. A cell is the pixel of the map's picture, and the cel's
/// offset is where the cell (0, 0) begins.
pub fn expand(doc: &Document, layer: usize, frame: usize) -> Vec<u32> {
    let l = &doc.layers()[layer];
    let set = doc
        .tileset(l.tileset().expect("a tilemap layer"))
        .expect("its tileset");
    let (tw, th) = set.tile_size();
    let cel = l.cel(frame);
    let (ox, oy) = cel.position();
    let mut out = vec![0; doc.width * doc.height];
    for py in 0..doc.height {
        for px in 0..doc.width {
            let (lx, ly) = (px as i32 - i32::from(ox), py as i32 - i32::from(oy));
            let (cx, tx) = (lx.div_euclid(tw as i32), lx.rem_euclid(tw as i32) as usize);
            let (cy, ty) = (ly.div_euclid(th as i32), ly.rem_euclid(th as i32) as usize);
            let cell = MapCell::from_bits(cel.image().get(cx, cy));
            let Some(tile) = cell.tile() else {
                continue;
            };
            let entry = set.tile(tile).expect("a tile the tileset has");
            let (sx, sy) = cell.orientation().inverse().apply(tx, ty, tw, th);
            out[py * doc.width + px] = entry.image.get(sx as i32, sy as i32);
        }
    }
    out
}
