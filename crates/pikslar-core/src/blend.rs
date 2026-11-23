//! Pixel blending.
//!
//! Sources are straight-alpha RGBA, as stored in layers. The destination is premultiplied
//! BGRA, which is what the screen wants, so opaque pixels need only a channel swap.
//!
//! The compositor in `pikslar-render` and merging layers here both use these functions, so a
//! merged layer is exactly what the screen showed.

use crate::document::BlendMode;

/// Rounded division of a 0..=65025 product by 255.
#[inline(always)]
fn div255(v: u32) -> u32 {
    let t = v + 128;
    (t + (t >> 8)) >> 8
}

#[inline(always)]
fn bgra(b: u32, g: u32, r: u32, a: u32) -> u32 {
    b | (g << 8) | (r << 16) | (a << 24)
}

/// Splits a straight RGBA cell into premultiplied channels at the given opacity.
#[inline(always)]
fn premultiply(src: u32, opacity: u32) -> (u32, u32, u32, u32) {
    let a = div255((src >> 24) * opacity);
    (
        div255(((src >> 16) & 255) * a),
        div255(((src >> 8) & 255) * a),
        div255((src & 255) * a),
        a,
    )
}

/// Normal blending of a straight source over a premultiplied destination.
#[inline(always)]
pub fn over(dst: u32, src: u32, opacity: u32) -> u32 {
    // Pixel art is almost entirely empty or opaque, so those cases skip the arithmetic.
    if src == 0 {
        return dst;
    }
    if src >> 24 == 255 && opacity == 255 {
        return (src & 0xff00_ff00) | ((src & 255) << 16) | ((src >> 16) & 255);
    }
    let (b, g, r, a) = premultiply(src, opacity);
    if a == 0 {
        return dst;
    }
    let inv = 255 - a;
    bgra(
        b + div255((dst & 255) * inv),
        g + div255(((dst >> 8) & 255) * inv),
        r + div255(((dst >> 16) & 255) * inv),
        a + div255((dst >> 24) * inv),
    )
}

/// Lays a flattened premultiplied image over `dst`, its colour pulled halfway to `tint`.
#[inline(always)]
pub fn ghost_over(dst: u32, ghost: u32, tint: u32, opacity: u32) -> u32 {
    let a = ghost >> 24;
    if a == 0 {
        return dst;
    }
    let channel = |shift: u32| {
        let tinted = div255(((tint >> shift) & 255) * a);
        div255((((ghost >> shift) & 255) + tinted) / 2 * opacity)
    };
    let alpha = div255(a * opacity);
    let inv = 255 - alpha;
    bgra(
        channel(0) + div255((dst & 255) * inv),
        channel(8) + div255(((dst >> 8) & 255) * inv),
        channel(16) + div255(((dst >> 16) & 255) * inv),
        alpha + div255((dst >> 24) * inv),
    )
}

/// Lays a flattened premultiplied image over `dst` in its own colours, faded to `opacity`.
#[inline(always)]
pub fn fade_over(dst: u32, ghost: u32, opacity: u32) -> u32 {
    let a = ghost >> 24;
    if a == 0 {
        return dst;
    }
    let channel = |shift: u32| div255(((ghost >> shift) & 255) * opacity);
    let alpha = div255(a * opacity);
    let inv = 255 - alpha;
    bgra(
        channel(0) + div255((dst & 255) * inv),
        channel(8) + div255(((dst >> 8) & 255) * inv),
        channel(16) + div255(((dst >> 16) & 255) * inv),
        alpha + div255((dst >> 24) * inv),
    )
}

/// Multiply blending, using the premultiplied form of the standard formula.
#[inline(always)]
pub fn multiply(dst: u32, src: u32, opacity: u32) -> u32 {
    let (sb, sg, sr, sa) = premultiply(src, opacity);
    if sa == 0 {
        return dst;
    }
    let da = dst >> 24;
    let channel =
        |s: u32, d: u32| (div255(s * d) + div255(s * (255 - da)) + div255(d * (255 - sa))).min(255);
    bgra(
        channel(sb, dst & 255),
        channel(sg, (dst >> 8) & 255),
        channel(sr, (dst >> 16) & 255),
        (sa + da - div255(sa * da)).min(255),
    )
}

/// A separable blend function on straight channels from 0 to 255: the backdrop, then the source.
pub type Separable = fn(u32, u32) -> u32;

pub fn screen(b: u32, s: u32) -> u32 {
    b + s - div255(b * s)
}

/// Hard light with the two layers swapped, as the W3C specification defines it.
pub fn overlay(b: u32, s: u32) -> u32 {
    if 2 * b <= 255 {
        div255(2 * b * s)
    } else {
        screen(2 * b - 255, s)
    }
}

pub fn darken(b: u32, s: u32) -> u32 {
    b.min(s)
}

pub fn lighten(b: u32, s: u32) -> u32 {
    b.max(s)
}

pub fn addition(b: u32, s: u32) -> u32 {
    (b + s).min(255)
}

pub fn subtract(b: u32, s: u32) -> u32 {
    b.saturating_sub(s)
}

pub fn difference(b: u32, s: u32) -> u32 {
    b.abs_diff(s)
}

/// A separable mode composited source over, from the W3C Compositing and Blending Level 1
/// specification: `co = cs (1 - ab) + cb (1 - as) + as ab B(Cb, Cs)` and
/// `ao = as + ab - as ab`, where lower case is premultiplied and upper case straight.
#[inline(always)]
pub fn separable(dst: u32, src: u32, opacity: u32, mix: Separable) -> u32 {
    let (sb, sg, sr, sa) = premultiply(src, opacity);
    if sa == 0 {
        return dst;
    }
    let da = dst >> 24;
    let both = div255(sa * da);
    let straight = |d: u32| match da {
        0 => 0,
        _ => ((d * 255 + da / 2) / da).min(255),
    };
    // Summed in 255ths and rounded once, so faint pixels do not gather three roundings.
    let channel = |s: u32, s_straight: u32, d: u32| {
        let mixed = (sa * da * mix(straight(d), s_straight) + 127) / 255;
        ((s * (255 - da) + d * (255 - sa) + mixed + 127) / 255).min(255)
    };
    bgra(
        channel(sb, (src >> 16) & 255, dst & 255),
        channel(sg, (src >> 8) & 255, (dst >> 8) & 255),
        channel(sr, src & 255, (dst >> 16) & 255),
        (sa + da - both).min(255),
    )
}

/// The blend function of a separable mode. Normal and Multiply have their own faster paths.
/// A mode only Aseprite-compatible blending has is drawn here as its stand-in.
pub fn separable_of(blend: BlendMode) -> Option<Separable> {
    Some(match blend.stand_in() {
        BlendMode::Normal | BlendMode::Multiply => return None,
        BlendMode::Screen => screen,
        BlendMode::Overlay => overlay,
        BlendMode::Darken => darken,
        BlendMode::Lighten => lighten,
        BlendMode::Addition => addition,
        BlendMode::Subtract => subtract,
        BlendMode::Difference => difference,
        _ => return None,
    })
}

/// A straight colour over a premultiplied pixel, in a layer's mode and at its opacity.
#[inline]
pub fn mix(dst: u32, color: u32, blend: BlendMode, opacity: u32) -> u32 {
    match blend {
        BlendMode::Normal => over(dst, color, opacity),
        BlendMode::Multiply => multiply(dst, color, opacity),
        _ => mix_separable(dst, color, blend, opacity),
    }
}

/// The separable modes of [`mix`], out of line: inlined, their arithmetic bloated every cell
/// loop that blends one cell at a time.
#[inline(never)]
fn mix_separable(dst: u32, color: u32, blend: BlendMode, opacity: u32) -> u32 {
    match (separable_of(blend), blend.stand_in()) {
        (Some(mode), _) => separable(dst, color, opacity, mode),
        (None, BlendMode::Multiply) => multiply(dst, color, opacity),
        (None, _) => over(dst, color, opacity),
    }
}

/// A straight colour over a pixel of a picture built with `blending`: premultiplied for
/// Pikslar's own, straight for Aseprite-compatible blending.
#[inline]
pub fn mix_in(
    blending: crate::Blending,
    dst: u32,
    color: u32,
    blend: BlendMode,
    opacity: u32,
) -> u32 {
    match blending {
        crate::Blending::Pikslar => mix(dst, color, blend, opacity),
        crate::Blending::Aseprite => crate::blend_straight::mix(dst, color, blend, opacity),
    }
}

/// Places a premultiplied pixel on an opaque backdrop color.
#[inline(always)]
pub fn on_backdrop(px: u32, backdrop: u32) -> u32 {
    let a = px >> 24;
    if a == 255 {
        return px;
    }
    if a == 0 {
        return backdrop;
    }
    let inv = 255 - a;
    bgra(
        (px & 255) + div255((backdrop & 255) * inv),
        ((px >> 8) & 255) + div255(((backdrop >> 8) & 255) * inv),
        ((px >> 16) & 255) + div255(((backdrop >> 16) & 255) * inv),
        255,
    )
}

/// Converts premultiplied BGRA back to straight RGBA, for export.
pub fn unpremultiply(px: u32) -> u32 {
    let a = px >> 24;
    if a == 0 {
        return 0;
    }
    let undo = |c: u32| ((c * 255 + a / 2) / a).min(255);
    undo((px >> 16) & 255) | (undo((px >> 8) & 255) << 8) | (undo(px & 255) << 16) | (a << 24)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RGBA_RED: u32 = 0xff00_00ff;
    const BGRA_RED: u32 = 0xffff_0000;
    const BGRA_BLUE: u32 = 0xff00_00ff;

    #[test]
    fn opaque_source_replaces() {
        assert_eq!(over(BGRA_BLUE, RGBA_RED, 255), BGRA_RED);
    }

    #[test]
    fn transparent_source_keeps_destination() {
        assert_eq!(over(BGRA_BLUE, 0, 255), BGRA_BLUE);
        assert_eq!(over(BGRA_BLUE, RGBA_RED, 0), BGRA_BLUE);
        assert_eq!(multiply(BGRA_BLUE, 0, 255), BGRA_BLUE);
    }

    #[test]
    fn half_opacity_blends_evenly() {
        let out = over(BGRA_BLUE, RGBA_RED, 128);
        assert_eq!(out >> 24, 255);
        assert!(((out >> 16) & 255).abs_diff(128) <= 1);
        assert!((out & 255).abs_diff(127) <= 1);
    }

    #[test]
    fn multiply_by_white_changes_nothing() {
        let dst = 0xff40_8020;
        assert_eq!(multiply(dst, 0xffff_ffff, 255), dst);
    }

    #[test]
    fn multiply_on_empty_is_the_source() {
        assert_eq!(multiply(0, RGBA_RED, 255), BGRA_RED);
    }

    #[test]
    fn straight_alpha_survives_premultiply_round_trip() {
        for a in [1u32, 17, 128, 254, 255] {
            let src = 0x00ff_ffff | (a << 24);
            assert_eq!(unpremultiply(over(0, src, 255)), src, "alpha {a}");
        }
        let opaque = 0xff12_3456;
        assert_eq!(unpremultiply(over(0, opaque, 255)), opaque);
    }

    /// The nearest whole number to `v / 255`. Exact halves cannot happen, 255 being odd.
    fn nearest(v: u32) -> u32 {
        (2 * v + 255) / 510
    }

    fn channel(px: u32, shift: u32) -> u32 {
        (px >> shift) & 255
    }

    const SHIFTS: [u32; 3] = [0, 8, 16];
    const LEVELS: [u32; 9] = [0, 1, 2, 17, 127, 128, 200, 254, 255];

    /// Straight colors at every interesting alpha and channel value.
    fn straight_colors() -> Vec<u32> {
        let mut colors = Vec::new();
        for a in LEVELS {
            for r in LEVELS {
                for g in [0, 128, 255] {
                    colors.push(r | (g << 8) | (LEVELS[(r % 9) as usize] << 16) | (a << 24));
                }
            }
        }
        colors
    }

    /// Every destination that compositing can actually build.
    fn premultiplied_colors() -> Vec<u32> {
        straight_colors().iter().map(|&c| over(0, c, 255)).collect()
    }

    #[test]
    fn division_by_255_rounds_to_nearest_across_its_whole_range() {
        for v in 0..=65_025 {
            assert_eq!(div255(v), nearest(v), "{v}");
        }
    }

    #[test]
    fn over_matches_the_written_formula() {
        for dst in premultiplied_colors() {
            for src in straight_colors() {
                for opacity in LEVELS {
                    let sa = nearest((src >> 24) * opacity);
                    let got = over(dst, src, opacity);
                    if sa == 0 {
                        assert_eq!(got, dst, "{dst:08x} {src:08x} {opacity}");
                        continue;
                    }
                    let inv = 255 - sa;
                    assert_eq!(got >> 24, sa + nearest((dst >> 24) * inv));
                    for shift in SHIFTS {
                        // The source is RGBA and the destination BGRA: red and blue swap.
                        let wanted = nearest(channel(src, 16 - shift) * sa)
                            + nearest(channel(dst, shift) * inv);
                        assert_eq!(channel(got, shift), wanted, "{dst:08x} {src:08x} {opacity}");
                    }
                }
            }
        }
    }

    #[test]
    fn multiply_matches_the_written_formula() {
        for dst in premultiplied_colors() {
            for src in straight_colors() {
                for opacity in LEVELS {
                    let sa = nearest((src >> 24) * opacity);
                    let da = dst >> 24;
                    let got = multiply(dst, src, opacity);
                    if sa == 0 {
                        assert_eq!(got, dst, "{dst:08x} {src:08x} {opacity}");
                        continue;
                    }
                    assert_eq!(got >> 24, (sa + da - nearest(sa * da)).min(255));
                    for shift in SHIFTS {
                        let s = nearest(channel(src, 16 - shift) * sa);
                        let d = channel(dst, shift);
                        let wanted =
                            (nearest(s * d) + nearest(s * (255 - da)) + nearest(d * (255 - sa)))
                                .min(255);
                        assert_eq!(channel(got, shift), wanted, "{dst:08x} {src:08x} {opacity}");
                    }
                }
            }
        }
    }

    /// The published formulas, in floating point on values from 0 to 1.
    fn reference(mode: &str, b: f64, s: f64) -> f64 {
        let screen = |b: f64, s: f64| b + s - b * s;
        match mode {
            "screen" => screen(b, s),
            "overlay" if b <= 0.5 => 2.0 * b * s,
            "overlay" => screen(2.0 * b - 1.0, s),
            "darken" => b.min(s),
            "lighten" => b.max(s),
            "addition" => (b + s).min(1.0),
            "subtract" => (b - s).max(0.0),
            _ => (b - s).abs(),
        }
    }

    #[test]
    fn separable_modes_match_the_published_formulas_within_one_level() {
        let modes: [(&str, Separable); 7] = [
            ("screen", screen),
            ("overlay", overlay),
            ("darken", darken),
            ("lighten", lighten),
            ("addition", addition),
            ("subtract", subtract),
            ("difference", difference),
        ];
        for (name, mix) in modes {
            for dst in premultiplied_colors() {
                for src in straight_colors() {
                    for opacity in LEVELS {
                        let got = separable(dst, src, opacity, mix);
                        // The source alpha at this opacity, rounded as every mode rounds it.
                        let sa = nearest((src >> 24) * opacity) as f64 / 255.0;
                        if nearest((src >> 24) * opacity) == 0 {
                            assert_eq!(got, dst, "{name}");
                            continue;
                        }
                        let da = (dst >> 24) as f64 / 255.0;
                        let alpha = sa + da - sa * da;
                        let near = |got: u32, want: f64| (got as f64 - want * 255.0).abs() <= 1.0;
                        assert!(near(got >> 24, alpha), "{name} alpha {dst:08x} {src:08x}");
                        for shift in SHIFTS {
                            let cs = channel(src, 16 - shift) as f64 / 255.0;
                            let cb_pre = channel(dst, shift) as f64 / 255.0;
                            // A premultiplied backdrop holds its straight colour only to the
                            // nearest level, so the formula is given that level.
                            let cb = match da == 0.0 {
                                true => 0.0,
                                false => ((cb_pre / da).min(1.0) * 255.0).round() / 255.0,
                            };
                            let want = sa * cs * (1.0 - da)
                                + cb_pre * (1.0 - sa)
                                + sa * da * reference(name, cb, cs);
                            let ch = channel(got, shift);
                            assert!(
                                near(ch, want.min(1.0)),
                                "{name} {dst:08x} {src:08x} {opacity}: {ch} against {}",
                                want * 255.0
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn ghost_is_exact_and_leaves_empty_pixels_alone() {
        // White halfway to the warm tint, at the opacity of the nearest frame.
        assert_eq!(ghost_over(0, 0xffff_ffff, 0x00e8_5a48, 104), 0x6863_4642);
        assert_eq!(ghost_over(BGRA_BLUE, 0, 0x00e8_5a48, 104), BGRA_BLUE);
    }

    #[test]
    fn an_untinted_ghost_is_its_own_colour_faded() {
        assert_eq!(fade_over(0, 0xffff_ffff, 104), 0x6868_6868);
        assert_eq!(fade_over(0, 0xff00_00ff, 255), 0xff00_00ff);
        assert_eq!(fade_over(BGRA_BLUE, 0, 104), BGRA_BLUE);
    }

    #[test]
    fn showing_an_exported_pixel_gives_the_shown_pixel_for_every_alpha_and_value() {
        for a in 1..=255u32 {
            for value in 0..=255u32 {
                let straight = value | (value << 8) | (value << 16) | (a << 24);
                let shown = over(0, straight, 255);
                assert_eq!(over(0, unpremultiply(shown), 255), shown, "{a} {value}");
            }
        }
    }

    #[test]
    fn backdrop_shows_through_transparency() {
        assert_eq!(on_backdrop(0, 0xff11_2233), 0xff11_2233);
        assert_eq!(on_backdrop(BGRA_RED, 0xff11_2233), BGRA_RED);
    }
}
