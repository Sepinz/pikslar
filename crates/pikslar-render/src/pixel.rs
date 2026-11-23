//! The one definition of how a stored cell becomes part of a picture.
//!
//! The arithmetic is in [`crate::blend`]. This says which arithmetic applies to which layer, and
//! what a neighbouring frame looks like. The compositor works a tile at a time and the drag
//! preview a pixel at a time, so both forms live here side by side and a test keeps them equal.
//!
//! A document's [`Blending`] says which arithmetic: Pikslar's own builds a premultiplied
//! picture, Aseprite-compatible blending ([`pikslar_core::blend_straight`]) a straight one,
//! which the `_in` functions here take and [`to_screen_tile`] makes premultiplied for the screen.

use crate::blend::{fade_over, ghost_over, mix, multiply, over, separable, separable_of};
use crate::onion::OnionStyle;
use pikslar_core::blend_straight as straight;
use pikslar_core::tile::RawTile;
use pikslar_core::{BlendMode, Blending, ColorMode, Layer};

pub(crate) type Lut = [u32; 65536];

#[inline(always)]
fn palette_color(cell: u32, lut: &Lut) -> u32 {
    lut[(cell & 0xffff) as usize]
}

/// The straight color a cell stands for on a layer of this mode.
#[inline(always)]
pub(crate) fn stored_color(mode: ColorMode, cell: u32, lut: &Lut) -> u32 {
    match mode {
        ColorMode::Rgba => cell,
        ColorMode::Palette => palette_color(cell, lut),
    }
}

/// One cell of a layer over a composited pixel.
#[inline]
pub(crate) fn blend_cell(dst: u32, cell: u32, layer: &Layer, lut: &Lut) -> u32 {
    let color = stored_color(layer.mode, cell, lut);
    let opacity = u32::from(layer.opacity);
    mix(dst, color, layer.blend, opacity)
}

/// [`blend_cell`] in the document's blending: over a straight pixel for Aseprite-compatible
/// blending.
#[cfg(test)]
pub(crate) fn blend_cell_in(
    blending: Blending,
    dst: u32,
    cell: u32,
    layer: &Layer,
    lut: &Lut,
) -> u32 {
    match blending {
        Blending::Pikslar => blend_cell_as::<false>(dst, cell, layer, lut),
        Blending::Aseprite => blend_cell_as::<true>(dst, cell, layer, lut),
    }
}

/// [`blend_cell_in`] with the blending a constant, Aseprite-compatible when `STRAIGHT`, for
/// loops chosen once per blending, so Pikslar's loop has no test of it.
#[inline(always)]
pub(crate) fn blend_cell_as<const STRAIGHT: bool>(
    dst: u32,
    cell: u32,
    layer: &Layer,
    lut: &Lut,
) -> u32 {
    if STRAIGHT {
        let color = stored_color(layer.mode, cell, lut);
        straight::mix(dst, color, layer.blend, u32::from(layer.opacity))
    } else {
        blend_cell(dst, cell, layer, lut)
    }
}

/// A pixel of the document's blending as the screen shows it: premultiplied. For the tests.
#[cfg(test)]
pub(crate) fn screen_in(blending: Blending, px: u32) -> u32 {
    match blending {
        Blending::Pikslar => screen_as::<false>(px),
        Blending::Aseprite => screen_as::<true>(px),
    }
}

/// [`screen_in`] with the blending a constant, as [`blend_cell_as`].
#[inline(always)]
pub(crate) fn screen_as<const STRAIGHT: bool>(px: u32) -> u32 {
    if STRAIGHT {
        straight::to_screen(px)
    } else {
        px
    }
}

/// True if this cell hides everything beneath it. The same in both blendings: an opaque
/// normal cell at full opacity is itself over anything.
#[inline]
pub(crate) fn covers(layer: &Layer, cell: u32, lut: &Lut) -> bool {
    layer.blend == BlendMode::Normal
        && layer.opacity == 255
        && stored_color(layer.mode, cell, lut) >> 24 == 255
}

/// One tile of a layer over a composited tile. The branches sit outside the pixel loop.
#[inline(always)]
pub(crate) fn blend_tile(out: &mut RawTile, raw: &RawTile, layer: &Layer, lut: &Lut) {
    let opacity = u32::from(layer.opacity);
    // A mode only Aseprite-compatible blending has is drawn here as its stand-in.
    match (layer.mode, layer.blend.stand_in()) {
        (ColorMode::Rgba, BlendMode::Normal) => each_cell(out, raw, |c| c, over, opacity),
        (ColorMode::Rgba, BlendMode::Multiply) => each_cell(out, raw, |c| c, multiply, opacity),
        (ColorMode::Palette, BlendMode::Normal) => {
            each_cell(out, raw, |c| palette_color(c, lut), over, opacity)
        }
        (ColorMode::Palette, BlendMode::Multiply) => {
            each_cell(out, raw, |c| palette_color(c, lut), multiply, opacity)
        }
        (mode, blend) => blend_tile_separable(out, raw, (mode, blend), lut, opacity),
    }
}

/// [`blend_tile`] in the document's blending: over a straight tile for Aseprite-compatible
/// blending.
#[inline(always)]
pub(crate) fn blend_tile_in(
    blending: Blending,
    out: &mut RawTile,
    raw: &RawTile,
    layer: &Layer,
    lut: &Lut,
) {
    match blending {
        Blending::Pikslar => blend_tile(out, raw, layer, lut),
        Blending::Aseprite => blend_tile_straight(out, raw, layer, lut),
    }
}

/// Aseprite-compatible blending of one tile, out of line so Pikslar's loops stay as they were.
/// Normal, by far the most common, has a loop of its own; every other mode reads its colour
/// function once for the tile.
#[inline(never)]
fn blend_tile_straight(out: &mut RawTile, raw: &RawTile, layer: &Layer, lut: &Lut) {
    let opacity = u32::from(layer.opacity);
    let resolve = |c: u32| stored_color(layer.mode, c, lut);
    match straight::mode_color(layer.blend) {
        None => each_cell(out, raw, resolve, straight::normal, opacity),
        Some(f) => {
            let mix = |d: u32, s: u32, o: u32| straight::blended(d, s, o, f);
            each_cell(out, raw, resolve, mix, opacity);
        }
    }
}

/// A straight tile as the screen shows it: premultiplied.
pub(crate) fn to_screen_tile(out: &mut RawTile) {
    for px in out.iter_mut() {
        *px = straight::to_screen(*px);
    }
}

/// A premultiplied tile made straight, for Aseprite-compatible blending to draw on.
pub(crate) fn from_screen_tile(out: &mut RawTile) {
    for px in out.iter_mut() {
        *px = straight::from_screen(*px);
    }
}

/// The separable modes, out of line. Inside [`blend_tile`] their arithmetic kept it from being
/// inlined into the compositor, which lost the vectorized loops every composite runs.
#[inline(never)]
fn blend_tile_separable(
    out: &mut RawTile,
    raw: &RawTile,
    (mode, blend): (ColorMode, BlendMode),
    lut: &Lut,
    opacity: u32,
) {
    let Some(mix) = separable_of(blend) else {
        return;
    };
    let mix = |d: u32, s: u32, o: u32| separable(d, s, o, mix);
    match mode {
        ColorMode::Rgba => each_cell(out, raw, |c| c, mix, opacity),
        ColorMode::Palette => each_cell(out, raw, |c| palette_color(c, lut), mix, opacity),
    }
}

#[inline(always)]
fn each_cell(
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

/// One flattened pixel of a neighbouring frame over what is beneath it.
#[inline]
pub(crate) fn ghost_cell(dst: u32, flat: u32, distance: i32, style: &OnionStyle) -> u32 {
    match style.ghost(distance) {
        (Some(tint), opacity) => ghost_over(dst, flat, tint, opacity),
        (None, opacity) => fade_over(dst, flat, opacity),
    }
}

/// One flattened tile of a neighbouring frame over what is beneath it. The branch on the
/// colours sits outside the pixel loop.
#[inline]
pub(crate) fn ghost_tile(out: &mut RawTile, flat: &RawTile, distance: i32, style: &OnionStyle) {
    match style.ghost(distance) {
        (Some(tint), opacity) => {
            for (dst, &ghost) in out.iter_mut().zip(flat.iter()) {
                *dst = ghost_over(*dst, ghost, tint, opacity);
            }
        }
        (None, opacity) => {
            for (dst, &ghost) in out.iter_mut().zip(flat.iter()) {
                *dst = fade_over(*dst, ghost, opacity);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pikslar_core::TILE_AREA;

    /// Empty, opaque, translucent, invisible and out-of-table cells in every position.
    fn cells(seed: u32) -> Box<RawTile> {
        let mut raw = Box::new([0u32; TILE_AREA]);
        for (i, cell) in raw.iter_mut().enumerate() {
            let n = (i as u32 ^ seed).wrapping_mul(2_654_435_761);
            *cell = match n >> 29 {
                0 | 1 => 0,
                2 | 3 => n | 0xff00_0000,
                4 => (n & 0x00ff_ffff) | 0x8000_0000,
                5 => n & 0x00ff_ffff,
                _ => n >> 24,
            };
        }
        raw
    }

    fn table() -> Box<Lut> {
        let mut lut = Box::new([0u32; 65536]);
        for (i, color) in lut.iter_mut().enumerate().take(200).skip(1) {
            let alpha = [255, 128, 1, 0][i % 4];
            *color = (i as u32 * 40_503) & 0x00ff_ffff | (alpha << 24);
        }
        lut
    }

    #[test]
    fn a_tile_blends_exactly_as_its_pixels_do() {
        let lut = table();
        for mode in [ColorMode::Rgba, ColorMode::Palette] {
            for blend in [BlendMode::Normal, BlendMode::Multiply] {
                for opacity in [255, 200, 128, 1, 0] {
                    let mut layer = Layer::new("layer".into(), mode);
                    (layer.blend, layer.opacity) = (blend, opacity);
                    let (raw, under) = (cells(1), cells(2));
                    let mut out = *under;
                    blend_tile(&mut out, &raw, &layer, &lut);
                    for i in 0..TILE_AREA {
                        let single = blend_cell(under[i], raw[i], &layer, &lut);
                        assert_eq!(out[i], single, "{mode:?} {blend:?} {opacity} at {i}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_tile_blends_exactly_as_its_pixels_do_in_either_blending() {
        let lut = table();
        for blending in [Blending::Pikslar, Blending::Aseprite] {
            for mode in [ColorMode::Rgba, ColorMode::Palette] {
                for blend in BlendMode::ALL {
                    for opacity in [255, 128, 0] {
                        let mut layer = Layer::new("layer".into(), mode);
                        (layer.blend, layer.opacity) = (blend, opacity);
                        let (raw, under) = (cells(5), cells(6));
                        let mut out = *under;
                        blend_tile_in(blending, &mut out, &raw, &layer, &lut);
                        for i in 0..TILE_AREA {
                            let single = blend_cell_in(blending, under[i], raw[i], &layer, &lut);
                            assert_eq!(out[i], single, "{blending:?} {blend:?} {opacity} at {i}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn pikslar_blending_draws_a_mode_only_aseprite_has_as_its_stand_in() {
        let lut = table();
        for blend in BlendMode::ALL {
            let mut layer = Layer::new("layer".into(), ColorMode::Rgba);
            let mut stand_in = layer.clone();
            (layer.blend, stand_in.blend) = (blend, blend.stand_in());
            let (raw, under) = (cells(7), cells(8));
            let (mut out, mut want) = (*under, *under);
            blend_tile(&mut out, &raw, &layer, &lut);
            blend_tile(&mut want, &raw, &stand_in, &lut);
            assert_eq!(out[..], want[..], "{blend:?}");
            for i in 0..TILE_AREA {
                assert_eq!(
                    blend_cell(under[i], raw[i], &layer, &lut),
                    want[i],
                    "{blend:?}"
                );
            }
        }
    }

    #[test]
    fn a_straight_tile_reaches_the_screen_premultiplied() {
        let mut out = *cells(9);
        let straight_tile = out;
        to_screen_tile(&mut out);
        for i in 0..TILE_AREA {
            assert_eq!(out[i], screen_in(Blending::Aseprite, straight_tile[i]));
            assert_eq!(
                screen_in(Blending::Pikslar, straight_tile[i]),
                straight_tile[i]
            );
        }
        from_screen_tile(&mut out);
        for i in (0..TILE_AREA).filter(|&i| straight_tile[i] >> 24 == 255) {
            assert_eq!(
                out[i], straight_tile[i],
                "an opaque pixel comes back exactly"
            );
        }
    }

    #[test]
    fn a_ghost_tile_blends_exactly_as_its_pixels_do() {
        use crate::onion::OnionColors;
        for colors in OnionColors::ALL {
            for strength in 0..crate::onion::STRENGTHS.len() {
                let style = OnionStyle {
                    colors,
                    strength,
                    ..OnionStyle::default()
                };
                for distance in [-4, -3, -2, -1, 1, 2, 3, 4] {
                    let (flat, under) = (cells(3), cells(4));
                    let mut out = *under;
                    ghost_tile(&mut out, &flat, distance, &style);
                    for i in 0..TILE_AREA {
                        let single = ghost_cell(under[i], flat[i], distance, &style);
                        assert_eq!(out[i], single, "{style:?} {distance} at {i}");
                    }
                }
            }
        }
    }

    #[test]
    fn only_a_fully_opaque_normal_cell_covers() {
        let lut = table();
        let opaque = 0xff12_3456;
        let mut layer = Layer::new("layer".into(), ColorMode::Rgba);
        assert!(covers(&layer, opaque, &lut));
        assert!(!covers(&layer, 0x8012_3456, &lut));
        layer.opacity = 254;
        assert!(!covers(&layer, opaque, &lut));
        layer.opacity = 255;
        layer.blend = BlendMode::Multiply;
        assert!(!covers(&layer, opaque, &lut));
    }
}
