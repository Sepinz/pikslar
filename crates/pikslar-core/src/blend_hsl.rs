//! The four non-separable modes of Aseprite-compatible blending: hue, saturation, color and
//! luminosity, in `f64` as Aseprite's MIT-licensed `src/doc/blend_funcs.cpp` (commit
//! `9cf3c128`) computes them, each channel truncated to 8 bits at the end. Every operation is
//! in the same order as there, so the results are the same to the last bit.

type Rgb = (f64, f64, f64);

fn unit(px: u32) -> Rgb {
    let c = |shift: u32| f64::from((px >> shift) & 255) / 255.0;
    (c(0), c(8), c(16))
}

/// Truncated toward zero, as C's `int(255.0 * v)`.
fn byte(v: f64) -> u32 {
    ((255.0 * v) as i32).clamp(0, 255) as u32
}

fn pack((r, g, b): Rgb) -> u32 {
    byte(r) | (byte(g) << 8) | (byte(b) << 16)
}

fn lum((r, g, b): Rgb) -> f64 {
    0.3 * r + 0.59 * g + 0.11 * b
}

fn sat((r, g, b): Rgb) -> f64 {
    r.max(g.max(b)) - r.min(g.min(b))
}

fn clip_color((mut r, mut g, mut b): Rgb) -> Rgb {
    let l = lum((r, g, b));
    let n = r.min(g.min(b));
    let x = r.max(g.max(b));
    if n < 0.0 {
        r = l + (((r - l) * l) / (l - n));
        g = l + (((g - l) * l) / (l - n));
        b = l + (((b - l) * l) / (l - n));
    }
    if x > 1.0 {
        r = l + (((r - l) * (1.0 - l)) / (x - l));
        g = l + (((g - l) * (1.0 - l)) / (x - l));
        b = l + (((b - l) * (1.0 - l)) / (x - l));
    }
    (r, g, b)
}

fn set_lum((r, g, b): Rgb, l: f64) -> Rgb {
    let d = l - lum((r, g, b));
    clip_color((r + d, g + d, b + d))
}

/// Every channel scaled from the least to `s`; two equal channels stay equal.
fn set_sat((r, g, b): Rgb, s: f64) -> Rgb {
    let min = r.min(g).min(b);
    let max = r.max(g).max(b);
    let range = max - min;
    if range > 0.0 {
        (
            ((r - min) * s) / range,
            ((g - min) * s) / range,
            ((b - min) * s) / range,
        )
    } else {
        (0.0, 0.0, 0.0)
    }
}

/// The source's hue with the backdrop's saturation and luminosity.
pub fn hue(back: u32, src: u32) -> u32 {
    let b = unit(back);
    pack(set_lum(set_sat(unit(src), sat(b)), lum(b)))
}

/// The source's saturation with the backdrop's hue and luminosity.
pub fn saturation(back: u32, src: u32) -> u32 {
    let b = unit(back);
    pack(set_lum(set_sat(b, sat(unit(src))), lum(b)))
}

/// The source's hue and saturation with the backdrop's luminosity.
pub fn color(back: u32, src: u32) -> u32 {
    pack(set_lum(unit(src), lum(unit(back))))
}

/// The source's luminosity with the backdrop's hue and saturation.
pub fn luminosity(back: u32, src: u32) -> u32 {
    pack(set_lum(unit(back), lum(unit(src))))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: u32 = 0x0000_00ff;
    const GREY: u32 = 0x0080_8080;

    #[test]
    fn a_grey_backdrop_gives_a_grey_hue_and_saturation() {
        // No saturation to give: the source's hue at zero saturation is the backdrop's grey.
        let grey = |px: u32| {
            let (r, g, b) = (px & 255, (px >> 8) & 255, (px >> 16) & 255);
            r == g && g == b && r.abs_diff(128) <= 1
        };
        assert!(grey(hue(GREY, RED)), "{:06x}", hue(GREY, RED));
        assert!(grey(saturation(GREY, RED)), "{:06x}", saturation(GREY, RED));
    }

    #[test]
    fn color_keeps_the_backdrops_luminosity_and_luminosity_the_sources() {
        let out = color(GREY, RED);
        let l = |px: u32| lum(unit(px));
        assert!((l(out) - l(GREY)).abs() < 2.0 / 255.0, "{out:06x}");
        let out = luminosity(RED, GREY);
        assert!((l(out) - l(GREY)).abs() < 2.0 / 255.0, "{out:06x}");
    }

    #[test]
    fn two_equal_channels_stay_equal_under_saturation() {
        // Green and blue tie; Aseprite before 2025 set one of them to 0 (issue 2661).
        let back = 0x0040_40c0;
        let out = saturation(back, 0x0000_80ff);
        assert_eq!((out >> 8) & 255, (out >> 16) & 255, "{out:06x}");
    }

    #[test]
    fn results_are_bytes_with_no_alpha() {
        for (b, s) in [(0xffff_ffff, 0), (0, 0xffff_ffff), (0x12_3456, 0xab_cdef)] {
            for f in [hue, saturation, color, luminosity] {
                assert_eq!(f(b, s) >> 24, 0);
            }
        }
    }
}
