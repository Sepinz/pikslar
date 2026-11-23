//! Color conversions for the interface. The document stores RGBA; artists think in HSV.

use pikslar_core::rgba;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsva {
    pub h: f32,
    pub s: f32,
    pub v: f32,
    pub a: f32,
}

/// Channels (r, g, b, a) of a stored color.
pub fn channels(color: u32) -> (u8, u8, u8, u8) {
    let [r, g, b, a] = color.to_le_bytes();
    (r, g, b, a)
}

/// Converts a straight RGBA color to opaque BGRA for display.
pub fn to_screen(color: u32) -> u32 {
    let (r, g, b, _) = channels(color);
    0xff00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

pub fn hex(color: u32) -> String {
    let (r, g, b, a) = channels(color);
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

pub fn to_hsva(color: u32) -> Hsva {
    let (r, g, b, a) = channels(color);
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    Hsva {
        h,
        s: if max == 0.0 { 0.0 } else { d / max },
        v: max,
        a: a as f32 / 255.0,
    }
}

pub fn from_hsva(c: Hsva) -> u32 {
    let h = c.h.rem_euclid(1.0) * 6.0;
    let chroma = c.v * c.s;
    let x = chroma * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as i32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = c.v - chroma;
    let byte = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    rgba(
        byte(r),
        byte(g),
        byte(b),
        (c.a * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_channel_extreme_round_trips() {
        for r in (0..=255).step_by(51) {
            for g in (0..=255).step_by(51) {
                for b in (0..=255).step_by(51) {
                    let c = rgba(r, g, b, 200);
                    assert_eq!(from_hsva(to_hsva(c)), c, "{r} {g} {b}");
                }
            }
        }
    }

    #[test]
    fn screen_format_swaps_red_and_blue() {
        assert_eq!(to_screen(rgba(1, 2, 3, 77)), 0xff01_0203);
    }

    #[test]
    fn hex_hides_full_alpha() {
        assert_eq!(hex(rgba(255, 0, 16, 255)), "#FF0010");
        assert_eq!(hex(rgba(255, 0, 16, 128)), "#FF001080");
    }
}
