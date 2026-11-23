//! Pikslar's own blending is what it was before Aseprite-compatible blending joined it.
//!
//! The per-layer tile blending below is copied from `pixel.rs` and `compose.rs` as they were at
//! commit `d84bd22`, before the change, and is never to be edited. Random scenes with every one
//! of Pikslar's modes, tilemap layers, groups, opacities and holds go through the compositor,
//! the export and the bands of today, which must give the frozen copy's pixels bit for bit.

use crate::blend::{self, multiply, over, separable, unpremultiply};
use crate::tilemap_oracle::{random_scene, Rng};
use crate::{flatten_document, BandSource, Compositor};
use pikslar_core::tile::RawTile;
use pikslar_core::{BlendMode, ColorMode, Damage, Document, Editor, Layer, TILE, TILE_AREA};

type Lut = [u32; 65536];

// ---- Frozen at d84bd22: do not edit. ----

fn frozen_separable_of(blend: BlendMode) -> Option<blend::Separable> {
    Some(match blend {
        BlendMode::Normal | BlendMode::Multiply => return None,
        BlendMode::Screen => blend::screen,
        BlendMode::Overlay => blend::overlay,
        BlendMode::Darken => blend::darken,
        BlendMode::Lighten => blend::lighten,
        BlendMode::Addition => blend::addition,
        BlendMode::Subtract => blend::subtract,
        BlendMode::Difference => blend::difference,
        _ => unreachable!("not a mode of d84bd22"),
    })
}

fn frozen_each_cell(
    out: &mut RawTile,
    raw: &RawTile,
    resolve: impl Fn(u32) -> u32,
    mix: impl Fn(u32, u32, u32) -> u32,
    opacity: u32,
) {
    for (dst, &cell) in out.iter_mut().zip(raw.iter()) {
        *dst = mix(*dst, resolve(cell), opacity);
    }
}

fn frozen_blend_tile(out: &mut RawTile, raw: &RawTile, layer: &Layer, lut: &Lut) {
    let opacity = u32::from(layer.opacity);
    let palette = |c: u32| lut[(c & 0xffff) as usize];
    match (layer.mode, layer.blend) {
        (ColorMode::Rgba, BlendMode::Normal) => frozen_each_cell(out, raw, |c| c, over, opacity),
        (ColorMode::Rgba, BlendMode::Multiply) => {
            frozen_each_cell(out, raw, |c| c, multiply, opacity)
        }
        (ColorMode::Palette, BlendMode::Normal) => {
            frozen_each_cell(out, raw, palette, over, opacity)
        }
        (ColorMode::Palette, BlendMode::Multiply) => {
            frozen_each_cell(out, raw, palette, multiply, opacity)
        }
        (mode, blend) => {
            let Some(mix) = frozen_separable_of(blend) else {
                return;
            };
            let mix = |d: u32, s: u32, o: u32| separable(d, s, o, mix);
            match mode {
                ColorMode::Rgba => frozen_each_cell(out, raw, |c| c, mix, opacity),
                ColorMode::Palette => frozen_each_cell(out, raw, palette, mix, opacity),
            }
        }
    }
}

// ---- End of the frozen copy. ----

fn lut_of(doc: &Document) -> Box<Lut> {
    let mut lut = Box::new([0u32; 65536]);
    for (id, color) in lut.iter_mut().enumerate() {
        *color = doc.colors.color(id as u32);
    }
    lut
}

/// The frame as the frozen copy composites it: premultiplied, the layers `drawn` takes.
fn frozen_frame(doc: &Document, frame: usize, drawn: &[bool]) -> Vec<u32> {
    let lut = lut_of(doc);
    let (w, h) = (doc.width, doc.height);
    let mut px = vec![0u32; w * h];
    for ty in 0..h.div_ceil(TILE) {
        for tx in 0..w.div_ceil(TILE) {
            let mut out = [0u32; TILE_AREA];
            let mut raw = [0u32; TILE_AREA];
            for (i, layer) in doc.layers().iter().enumerate() {
                if drawn[i] && doc.layer_tile(layer, frame, tx, ty, &mut raw) {
                    frozen_blend_tile(&mut out, &raw, layer, &lut);
                }
            }
            for y in 0..TILE.min(h - ty * TILE) {
                let columns = TILE.min(w - tx * TILE);
                let to = (ty * TILE + y) * w + tx * TILE;
                px[to..to + columns].copy_from_slice(&out[y * TILE..][..columns]);
            }
        }
    }
    px
}

fn band_picture(source: &BandSource<'_>, frame: usize) -> Vec<u32> {
    let (mut all, mut band) = (Vec::new(), Vec::new());
    for index in 0..source.bands() {
        source.band(frame, index, &mut band);
        all.extend_from_slice(&band);
    }
    all
}

/// Every layer gets one of Pikslar's nine modes and a random opacity, so each mode's arithmetic
/// runs under translucent layers too.
fn mixed_modes(doc: &mut Document, rng: &mut Rng) {
    for layer in doc.layers_mut().iter_mut() {
        layer.blend = BlendMode::PIKSLAR[rng.below(9) as usize];
        layer.opacity = [255, 255, 200, 128, 1][rng.below(5) as usize];
    }
}

#[test]
fn pikslar_blending_draws_bit_for_bit_what_it_drew_before() {
    for seed in 1..=200u64 {
        let mut doc = random_scene(seed).doc;
        mixed_modes(&mut doc, &mut Rng(seed ^ 0xf00d));
        let (w, h) = (doc.width, doc.height);
        for frame in 0..doc.frames.len() {
            let what = format!("seed {seed} frame {frame}");
            let screen = frozen_frame(&doc, frame, &doc.screen_layers());
            let mut e = Editor::new(doc.clone());
            e.frame = frame;
            let mut comp = Compositor::new(w, h);
            comp.update(&e, Damage::All, &mut Vec::new());
            assert!(comp.pixels() == &screen[..], "screen {what}");
            let picture = frozen_frame(&doc, frame, &doc.picture_layers());
            let want: Vec<u32> = picture.iter().map(|&p| unpremultiply(p)).collect();
            let single = crate::flatten::single_plain_layer(&doc).is_some();
            let flat = flatten_document(&doc, frame, &mut Compositor::new(1, 1));
            // One plain layer is read from its stored colours, as before, not composited.
            if !single {
                assert!(flat == want, "export {what}");
                assert!(
                    band_picture(&BandSource::new(&doc), frame) == want,
                    "bands {what}"
                );
            }
        }
    }
}
