//! Colour ramps: colours stepping from one colour to another through Oklab.
//!
//! Every step is whole-number arithmetic, so a ramp is the same on every machine and every run.
//! The definition, which the tests check against an independent script with exact maths:
//!
//! - Values are fixed point, `F = 2^32` meaning 1.0, held in `i128`.
//! - A colour byte becomes linear light by table ([`SRGB_LINEAR`]), never by `powf`.
//! - The Oklab matrices (Ottosson) are integers scaled by `10^10`; each dot product is summed
//!   and floored once by `div_euclid`.
//! - The cube root is the exact floor of the cube root of `value * F * F` ([`icbrt`], integer
//!   Newton), and the cube coming back is two floored fixed-point multiplies.
//! - The `k`-th of `steps` colours between two is `(a * (n - k) + b * k) / n` floored, with
//!   `n = steps + 1`, on each of L, a and b; alpha is the same, rounded half up.
//! - Back to a byte: linear light clamped to 0 to 1, then the nearest table entry (a tie takes
//!   the lower byte).

use super::ramp_table::SRGB_LINEAR;
use crate::color::rgba;

/// Most colours a ramp puts between its two ends.
pub const MAX_RAMP_STEPS: u32 = 254;

const ONE: i128 = 1 << 32;
const SCALE: i128 = 10_000_000_000;

type Triple = [i128; 3];

const TO_LMS: [Triple; 3] = [
    [4_122_214_708, 5_363_325_363, 514_459_929],
    [2_119_034_982, 6_806_995_451, 1_073_969_566],
    [883_024_619, 2_817_188_376, 6_299_787_005],
];
const TO_LAB: [Triple; 3] = [
    [2_104_542_553, 7_936_177_850, -40_720_468],
    [19_779_984_951, -24_285_922_050, 4_505_937_099],
    [259_040_371, 7_827_717_662, -8_086_757_660],
];
/// How l', m' and s' follow from L, a and b: L's weight is exactly 1, so only a and b are here.
const FROM_LAB: [[i128; 2]; 3] = [
    [3_963_377_774, 2_158_037_573],
    [-1_055_613_458, -638_541_728],
    [-894_841_775, -12_914_855_480],
];
const TO_LINEAR: [Triple; 3] = [
    [40_767_416_621, -33_077_115_913, 2_309_699_292],
    [-12_684_380_046, 26_097_574_011, -3_413_193_965],
    [-41_960_863, -7_034_186_147, 17_076_147_010],
];

/// The exact floor of the cube root of `n`, by integer Newton from a start above the root.
pub(crate) fn icbrt(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let bits = 128 - n.leading_zeros();
    let mut x = 1u128 << bits.div_ceil(3);
    loop {
        let next = (2 * x + n / (x * x)) / 3;
        if next >= x {
            return x;
        }
        x = next;
    }
}

fn dot(row: &Triple, v: &Triple) -> i128 {
    (row[0] * v[0] + row[1] * v[1] + row[2] * v[2]).div_euclid(SCALE)
}

/// A colour's L, a and b in fixed point. Alpha plays no part.
pub fn to_oklab(color: u32) -> [i128; 3] {
    let [r, g, b, _] = color.to_le_bytes();
    let lin = [r, g, b].map(|c| i128::from(SRGB_LINEAR[usize::from(c)]));
    let lms = TO_LMS.map(|row| dot(&row, &lin));
    // `lms` is never negative: every weight is, and every `lin` is.
    let root = lms.map(|v| icbrt((v.max(0) as u128) * (ONE * ONE) as u128) as i128);
    TO_LAB.map(|row| dot(&row, &root))
}

/// The nearest byte to a linear light value in fixed point, a tie going to the lower byte.
fn nearest_byte(light: i128) -> u8 {
    let light = light.clamp(0, ONE);
    let above = SRGB_LINEAR.partition_point(|&v| i128::from(v) <= light);
    let Some(below) = above.checked_sub(1) else {
        return 0;
    };
    let Some(&next) = SRGB_LINEAR.get(above) else {
        return below as u8;
    };
    let (near, far) = (
        light - i128::from(SRGB_LINEAR[below]),
        i128::from(next) - light,
    );
    (if far < near { above } else { below }) as u8
}

/// The opaque colour bytes for a point in Oklab, clamped into what a screen shows.
pub fn from_oklab(lab: [i128; 3]) -> [u8; 3] {
    let [l, a, b] = lab;
    let cubed = FROM_LAB.map(|[ca, cb]| {
        let v = (SCALE * l + ca * a + cb * b).div_euclid(SCALE);
        (((v * v) >> 32) * v) >> 32
    });
    TO_LINEAR.map(|row| nearest_byte(dot(&row, &cubed)))
}

/// The `steps` colours strictly between `from` and `to`, nearest `from` first. Each end keeps
/// its own alpha mixed in; the colours at the ends themselves are not made again.
pub fn ramp_colors(from: u32, to: u32, steps: u32) -> Vec<u32> {
    let (a, b) = (to_oklab(from), to_oklab(to));
    let (alpha_from, alpha_to) = (from.to_le_bytes()[3], to.to_le_bytes()[3]);
    let n = i128::from(steps) + 1;
    (1..n)
        .map(|k| {
            let lab = [0, 1, 2].map(|i| (a[i] * (n - k) + b[i] * k).div_euclid(n));
            let alpha = (i128::from(alpha_from) * (n - k) + i128::from(alpha_to) * k + n / 2) / n;
            let [r, g, b] = from_oklab(lab);
            rgba(r, g, b, alpha as u8)
        })
        .collect()
}

#[cfg(test)]
#[path = "ramp_tests.rs"]
mod tests;
