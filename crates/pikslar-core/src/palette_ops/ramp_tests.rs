//! Ramps against colours computed by `ramp-ref.py`: an independent script of exact integer and
//! decimal maths following the definition in `ramp.rs`.

use super::*;

const fn c(r: u8, g: u8, b: u8, a: u8) -> u32 {
    rgba(r, g, b, a)
}

fn expect(from: u32, to: u32, steps: u32, want: &[(u8, u8, u8, u8)]) {
    let want: Vec<u32> = want.iter().map(|&(r, g, b, a)| c(r, g, b, a)).collect();
    assert_eq!(ramp_colors(from, to, steps), want);
}

#[test]
fn black_to_white_in_three_steps() {
    let want = [(34, 34, 34, 255), (99, 99, 99, 255), (174, 174, 174, 255)];
    expect(c(0, 0, 0, 255), c(255, 255, 255, 255), 3, &want);
}

#[test]
fn red_to_blue_in_three_steps() {
    let want = [(198, 73, 109, 255), (140, 83, 162, 255), (81, 71, 210, 255)];
    expect(c(255, 0, 0, 255), c(0, 0, 255, 255), 3, &want);
}

#[test]
fn alpha_steps_along_with_the_colour() {
    let want = [
        (181, 76, 74, 229),
        (161, 92, 109, 203),
        (139, 104, 140, 178),
        (114, 112, 170, 152),
        (82, 117, 200, 126),
    ];
    expect(c(200, 50, 20, 255), c(20, 120, 230, 100), 5, &want);
}

#[test]
fn one_step_is_the_middle() {
    expect(
        c(0, 255, 0, 255),
        c(255, 255, 0, 255),
        1,
        &[(176, 255, 0, 255)],
    );
}

#[test]
fn the_same_bits_every_run() {
    let (a, b) = (c(12, 200, 99, 255), c(250, 3, 77, 255));
    let first = ramp_colors(a, b, MAX_RAMP_STEPS);
    assert_eq!(first.len(), MAX_RAMP_STEPS as usize);
    for _ in 0..5 {
        assert_eq!(ramp_colors(a, b, MAX_RAMP_STEPS), first);
    }
}

#[test]
fn the_cube_root_is_the_exact_floor() {
    for n in (0..5000u128).chain([u128::MAX, u128::MAX - 1, 1 << 96, (1 << 96) - 1]) {
        let r = icbrt(n);
        assert!(r.pow(3) <= n, "{n}");
        let next = r + 1;
        assert!(next.checked_pow(3).is_none_or(|cube| cube > n), "{n}");
    }
    for r in [1u128, 2, 1_000, 4_294_967_296, 1 << 42] {
        assert_eq!(icbrt(r.pow(3)), r);
        assert_eq!(icbrt(r.pow(3) - 1), r - 1);
    }
}

#[test]
fn the_light_table_is_sound() {
    assert_eq!(SRGB_LINEAR[0], 0);
    assert_eq!(SRGB_LINEAR[255], 1 << 32);
    assert!(SRGB_LINEAR.windows(2).all(|w| w[0] < w[1]));
    // The straight segment of the curve: 1 / 255 / 12.92 of 2^32, rounded.
    assert_eq!(SRGB_LINEAR[1], 1_303_638);
}

#[test]
fn a_colour_survives_the_trip_through_oklab() {
    for color in [
        c(0, 0, 0, 255),
        c(255, 255, 255, 255),
        c(1, 2, 3, 255),
        c(250, 128, 7, 255),
    ] {
        let [r, g, b] = from_oklab(to_oklab(color));
        assert_eq!(rgba(r, g, b, 255), color);
    }
}

#[test]
fn a_ramp_between_equal_colours_is_that_colour() {
    let color = c(90, 40, 200, 255);
    assert!(ramp_colors(color, color, 4).iter().all(|&m| m == color));
}
