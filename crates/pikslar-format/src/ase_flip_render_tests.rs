//! The flips of `.ase` tiles held against how Aseprite draws them.
//!
//! The oracle below is written from Aseprite's MIT-licensed renderer,
//! `src/render/render.cpp`, `composite_image_general_with_tile_flags`, at aseprite commit
//! 9cf3c128f582044689656422c96d2d6904ce66f4, and the flag values from `src/doc/tile.h` at the
//! same commit. It works from the destination, as that function does, and shares nothing with
//! Pikslar's orientations. No file here was saved or drawn by Aseprite itself.

use crate::ase_build::*;
use crate::ase_tests::open;
use crate::AseWarning;
use pikslar_core::orient::Orientation;
use pikslar_core::tile::{RawTile, TILE, TILE_AREA};
use pikslar_core::{rgba, Document};

/// `doc::tile_i_mask`, `tile_f_xflip`, `tile_f_yflip` and `tile_f_dflip` of `src/doc/tile.h`.
const ASEPRITE_MASKS: [u32; 4] = [0x1fff_ffff, 0x8000_0000, 0x4000_0000, 0x2000_0000];

/// Tile 1 of a `w` by `h` tileset: every pixel a different colour, so no flip looks like another.
fn tile_pixels(w: usize, h: usize) -> Vec<u32> {
    (0..w * h)
        .map(|i| rgba(1 + (i % w) as u8 * 20, 1 + (i / w) as u8 * 30, 7, 255))
        .collect()
}

fn bytes_of(pixels: &[u32]) -> Vec<u8> {
    let mut out = vec![0u8; pixels.len() * 4]; // tile 0, the empty tile
    for &p in pixels {
        out.extend_from_slice(&p.to_le_bytes());
    }
    out
}

/// What Aseprite draws for one tile at scale 1: for each destination pixel (x, y) the source x
/// is `w - 1 - x` under the x flip, the source y `h - 1 - y` under the y flip, the two trade
/// places under the diagonal flip, and a source place outside the tile (outside its top-left
/// square, under the diagonal flip) is drawn clear.
fn aseprite_draws(
    src: &[u32],
    (w, h): (usize, usize),
    (xf, yf, df): (bool, bool, bool),
) -> Vec<u32> {
    let mut out = vec![0; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sx = if xf { w - 1 - x } else { x };
            let mut sy = if yf { h - 1 - y } else { y };
            let (lw, lh) = if df {
                std::mem::swap(&mut sx, &mut sy);
                (w.min(h), w.min(h))
            } else {
                (w, h)
            };
            if sx < lw && sy < lh {
                out[y * w + x] = src[sy * w + sx];
            }
        }
    }
    out
}

fn flags_of(bits: u32) -> (bool, bool, bool) {
    (bits & 4 != 0, bits & 2 != 0, bits & 1 != 0)
}

/// A one-cell map of tile 1 with the flags `bits` (x, y, diagonal), opened by Pikslar.
fn opened(size: (usize, usize), bits: u32) -> (Document, Vec<AseWarning>) {
    let (xf, yf, df) = flags_of(bits);
    let pick = |on: bool, mask: u32| if on { mask } else { 0 };
    let value = 1 | pick(xf, ASEPRITE_MASKS[1]) | pick(yf, ASEPRITE_MASKS[2]);
    let value = value | pick(df, ASEPRITE_MASKS[3]);
    let (w, h) = (size.0 as u16, size.1 as u16);
    let chunks = vec![
        tileset_chunk(
            0,
            4,
            2,
            (w, h),
            "t",
            &bytes_of(&tile_pixels(size.0, size.1)),
        ),
        tilemap_layer(3, 0, "m", 0),
        cel_tilemap(0, (0, 0), (1, 1), 32, ASEPRITE_MASKS, &[value]),
    ];
    open(&Ase::new(w, h, 32).frame(100, chunks).bytes())
}

/// The cell as Pikslar shows it, read through both of the sampler's ways: pixel by pixel, and
/// a whole canvas tile at a time (the compositor's way). The two must agree.
fn pikslar_draws(doc: &Document, (w, h): (usize, usize)) -> Vec<u32> {
    let layer = &doc.layers()[0];
    let mut run: RawTile = [0; TILE_AREA];
    doc.layer_tile(layer, 0, 0, 0, &mut run);
    let mut out = vec![0; w * h];
    for y in 0..h {
        for x in 0..w {
            let one = doc.layer_pixel(layer, 0, x as i32, y as i32);
            assert_eq!(one, run[y * TILE + x], "the two ways differ at ({x}, {y})");
            out[y * w + x] = one;
        }
    }
    out
}

#[test]
fn our_masks_are_aseprites_flag_values() {
    assert_eq!(MASKS_NEW, ASEPRITE_MASKS);
    for bits in 0..8u32 {
        let (xf, yf, df) = flags_of(bits);
        let value = u32::from(xf) << 31 | u32::from(yf) << 30 | u32::from(df) << 29;
        assert_eq!(Orientation::from_tiled_bits(value).index(), bits as u8);
    }
}

#[test]
fn all_eight_flips_of_a_square_tile_match_aseprites_renderer() {
    for size in [(3, 3), (4, 4), (1, 1)] {
        let src = tile_pixels(size.0, size.1);
        for bits in 0..8 {
            let (doc, warnings) = opened(size, bits);
            assert!(warnings.is_empty(), "{size:?} {bits}: {warnings:?}");
            let want = aseprite_draws(&src, size, flags_of(bits));
            assert_eq!(
                pikslar_draws(&doc, size),
                want,
                "{size:?} flags {:?}",
                flags_of(bits)
            );
        }
    }
}

#[test]
fn the_flips_of_an_oblong_tile_match_and_a_diagonal_one_differs_and_is_told() {
    for size in [(3, 2), (2, 3), (5, 2)] {
        let src = tile_pixels(size.0, size.1);
        for bits in 0..8 {
            let (xf, yf, df) = flags_of(bits);
            let (doc, warnings) = opened(size, bits);
            let got = pikslar_draws(&doc, size);
            let aseprite = aseprite_draws(&src, size, (xf, yf, df));
            if !df {
                assert!(warnings.is_empty(), "{size:?} {bits}: {warnings:?}");
                assert_eq!(got, aseprite, "{size:?} flags {:?}", (xf, yf));
                continue;
            }
            // Aseprite draws only the top-left square, turned, and the rest clear. A document
            // cannot turn an oblong tile, so Pikslar keeps the flips without the turn.
            assert_eq!(warnings, [AseWarning::DiagonalOnOblong("m".into())]);
            assert_eq!(got, aseprite_draws(&src, size, (xf, yf, false)));
            let clear = aseprite.iter().filter(|&&p| p == 0).count();
            let (w, h) = size;
            assert_eq!(clear, w * h - w.min(h) * w.min(h), "{size:?} {bits}");
            assert_ne!(got, aseprite, "{size:?} {bits}");
        }
    }
}

#[test]
fn the_oracle_is_tileds_order_on_a_square() {
    // The same mapping, from the source: the diagonal flip first, then x, then y.
    let n = 3;
    let src = tile_pixels(n, n);
    for bits in 0..8u8 {
        let turn = Orientation::from_index(bits).expect("0 to 7");
        let mut placed = vec![0; n * n];
        for sy in 0..n {
            for sx in 0..n {
                let (x, y) = turn.apply(sx, sy, n, n);
                placed[y * n + x] = src[sy * n + sx];
            }
        }
        assert_eq!(
            aseprite_draws(&src, (n, n), flags_of(u32::from(bits))),
            placed
        );
    }
}
