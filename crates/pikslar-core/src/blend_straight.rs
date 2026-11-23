//! Aseprite-compatible blending, for documents whose [`crate::Blending`] is `Aseprite`.
//!
//! Both sides are straight-alpha RGBA, as layers store it (red in the low byte), and so is the
//! result: a picture built here is exported as it is, and made premultiplied only for the
//! screen ([`to_screen`]). The arithmetic is Aseprite's, from its MIT-licensed `src/doc`
//! (`blend_funcs.cpp` and pixman's `MUL_UN8` / `DIV_UN8`, Aseprite commit `9cf3c128`), with
//! its new blending method: a mode other than normal over a partly clear backdrop is the normal
//! result moved toward the mode's twice, by the backdrop's alpha and then by the backdrop's alpha
//! times the source's. Projects made in Pikslar never come here; see [`crate::blend`].

use crate::document::BlendMode;

/// `a * b / 255`, rounded as pixman's `MUL_UN8`; `a` may be negative, the shift flooring.
#[inline(always)]
fn mul(a: i32, b: i32) -> i32 {
    let t = a * b + 128;
    ((t >> 8) + t) >> 8
}

/// `a * 255 / b`, rounded as pixman's `DIV_UN8`. `b` is never 0 where it is called.
#[inline(always)]
fn div(a: i32, b: i32) -> i32 {
    (a * 255 + b / 2) / b
}

#[inline(always)]
fn channel(px: u32, shift: u32) -> i32 {
    ((px >> shift) & 255) as i32
}

#[inline(always)]
fn alpha(px: u32) -> i32 {
    (px >> 24) as i32
}

#[inline(always)]
fn pack(r: i32, g: i32, b: i32, a: i32) -> u32 {
    (r as u32 & 255) | ((g as u32 & 255) << 8) | ((b as u32 & 255) << 16) | ((a as u32) << 24)
}

/// A colour with no alpha is stored as 0, so the picture never keeps colours nobody sees.
#[inline(always)]
pub fn tidy(px: u32) -> u32 {
    match px >> 24 {
        0 => 0,
        _ => px,
    }
}

/// Aseprite's normal blender: `Ra = Sa + Ba - Ba Sa`, `Rc = Bc + (Sc - Bc) Sa / Ra`,
/// truncated, with the source alpha at `opacity` first.
#[inline(always)]
pub fn normal(back: u32, src: u32, opacity: u32) -> u32 {
    if back >> 24 == 0 {
        let a = mul(alpha(src), opacity as i32);
        return tidy((src & 0x00ff_ffff) | ((a as u32) << 24));
    }
    if src >> 24 == 0 {
        return back;
    }
    let sa = mul(alpha(src), opacity as i32);
    let ba = alpha(back);
    let ra = sa + ba - mul(ba, sa);
    let mix = |shift: u32| {
        let (b, s) = (channel(back, shift), channel(src, shift));
        b + (s - b) * sa / ra
    };
    pack(mix(0), mix(8), mix(16), ra)
}

/// Aseprite's merge: from `back` toward `src` by `w` / 255, in colour and alpha. Where one side
/// is clear the other's colour is kept.
#[inline(always)]
pub fn merge(back: u32, src: u32, w: i32) -> u32 {
    let (ba, sa) = (alpha(back), alpha(src));
    let ra = ba + mul(sa - ba, w);
    if ra == 0 {
        return 0;
    }
    let mix = |shift: u32| {
        let (b, s) = (channel(back, shift), channel(src, shift));
        match (ba, sa) {
            (0, _) => s,
            (_, 0) => b,
            _ => b + mul(s - b, w),
        }
    };
    pack(mix(0), mix(8), mix(16), ra)
}

/// A mode's colour: the backdrop's and the source's straight colours in, the colour out, its
/// alpha byte left 0.
pub type ModeColor = fn(u32, u32) -> u32;

/// A mode laid on the backdrop as Aseprite's new blending lays it: normal over a clear
/// backdrop; otherwise the normal result, moved toward the mode's colour laid on normally by
/// the backdrop's alpha, then by the backdrop's alpha times the source's at `opacity`.
#[inline(always)]
pub fn blended(back: u32, src: u32, opacity: u32, color: impl Fn(u32, u32) -> u32) -> u32 {
    let ba = alpha(back);
    if ba == 0 || src == 0 {
        return normal(back, src, opacity);
    }
    let plain = normal(back, src, opacity);
    let mode = normal(back, color(back, src) | (src & 0xff00_0000), opacity);
    let toward = merge(plain, mode, ba);
    let both = mul(ba, mul(alpha(src), opacity as i32));
    tidy(merge(toward, mode, both))
}

/// Applies a function of two channels to each of the three colour channels.
#[inline(always)]
fn each(back: u32, src: u32, f: impl Fn(i32, i32) -> i32) -> u32 {
    let one = |shift: u32| f(channel(back, shift), channel(src, shift));
    pack(one(0), one(8), one(16), 0)
}

fn screen_of(b: i32, s: i32) -> i32 {
    b + s - mul(b, s)
}

fn hard_light_of(b: i32, s: i32) -> i32 {
    match s < 128 {
        true => mul(b, s << 1),
        false => screen_of(b, (s << 1) - 255),
    }
}

fn dodge_of(b: i32, s: i32) -> i32 {
    let s = 255 - s;
    match b {
        0 => 0,
        _ if b >= s => 255,
        _ => div(b, s),
    }
}

fn burn_of(b: i32, s: i32) -> i32 {
    if b == 255 {
        return 255;
    }
    let b = 255 - b;
    match b >= s {
        true => 0,
        false => 255 - div(b, s),
    }
}

fn divide_of(b: i32, s: i32) -> i32 {
    match b {
        0 => 0,
        _ if b >= s => 255,
        _ => div(b, s),
    }
}

fn soft_light_of(b: i32, s: i32) -> i32 {
    let (b, s) = (f64::from(b) / 255.0, f64::from(s) / 255.0);
    let d = match b <= 0.25 {
        true => ((16.0 * b - 12.0) * b + 4.0) * b,
        false => b.sqrt(),
    };
    let r = match s <= 0.5 {
        true => b - (1.0 - 2.0 * s) * b * (1.0 - b),
        false => b + (2.0 * s - 1.0) * (d - b),
    };
    (r * 255.0 + 0.5) as i32
}

pub fn multiply(b: u32, s: u32) -> u32 {
    each(b, s, mul)
}

fn screen(b: u32, s: u32) -> u32 {
    each(b, s, screen_of)
}

fn overlay(b: u32, s: u32) -> u32 {
    each(b, s, |b, s| hard_light_of(s, b))
}

fn darken(b: u32, s: u32) -> u32 {
    each(b, s, i32::min)
}

fn lighten(b: u32, s: u32) -> u32 {
    each(b, s, i32::max)
}

fn color_dodge(b: u32, s: u32) -> u32 {
    each(b, s, dodge_of)
}

fn color_burn(b: u32, s: u32) -> u32 {
    each(b, s, burn_of)
}

fn hard_light(b: u32, s: u32) -> u32 {
    each(b, s, hard_light_of)
}

fn soft_light(b: u32, s: u32) -> u32 {
    each(b, s, soft_light_of)
}

fn difference(b: u32, s: u32) -> u32 {
    each(b, s, |b, s| (b - s).abs())
}

fn exclusion(b: u32, s: u32) -> u32 {
    each(b, s, |b, s| b + s - 2 * mul(b, s))
}

fn addition(b: u32, s: u32) -> u32 {
    each(b, s, |b, s| (b + s).min(255))
}

fn subtract(b: u32, s: u32) -> u32 {
    each(b, s, |b, s| (b - s).max(0))
}

fn divide(b: u32, s: u32) -> u32 {
    each(b, s, divide_of)
}

/// The colour function of every mode but normal.
pub fn mode_color(blend: BlendMode) -> Option<ModeColor> {
    Some(match blend {
        BlendMode::Normal => return None,
        BlendMode::Multiply => multiply,
        BlendMode::Screen => screen,
        BlendMode::Overlay => overlay,
        BlendMode::Darken => darken,
        BlendMode::Lighten => lighten,
        BlendMode::Addition => addition,
        BlendMode::Subtract => subtract,
        BlendMode::Difference => difference,
        BlendMode::ColorDodge => color_dodge,
        BlendMode::ColorBurn => color_burn,
        BlendMode::HardLight => hard_light,
        BlendMode::SoftLight => soft_light,
        BlendMode::Exclusion => exclusion,
        BlendMode::Hue => crate::blend_hsl::hue,
        BlendMode::Saturation => crate::blend_hsl::saturation,
        BlendMode::Color => crate::blend_hsl::color,
        BlendMode::Luminosity => crate::blend_hsl::luminosity,
        BlendMode::Divide => divide,
    })
}

/// A straight colour over a straight pixel, in a layer's mode and at its opacity.
#[inline]
pub fn mix(dst: u32, color: u32, blend: BlendMode, opacity: u32) -> u32 {
    match mode_color(blend) {
        None => normal(dst, color, opacity),
        Some(f) => blended(dst, color, opacity, f),
    }
}

/// A straight pixel as the screen wants it: premultiplied BGRA, as [`crate::blend`] builds it.
#[inline(always)]
pub fn to_screen(px: u32) -> u32 {
    crate::blend::over(0, px, 255)
}

/// A premultiplied screen pixel as a straight one, for laying straight layers over it.
#[inline(always)]
pub fn from_screen(px: u32) -> u32 {
    crate::blend::unpremultiply(px)
}

#[cfg(test)]
#[path = "blend_straight_tests.rs"]
mod tests;
