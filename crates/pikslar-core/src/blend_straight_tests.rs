use super::*;

const LEVELS: [u32; 9] = [0, 1, 2, 17, 127, 128, 200, 254, 255];

fn rgba(r: u32, g: u32, b: u32, a: u32) -> u32 {
    r | (g << 8) | (b << 16) | (a << 24)
}

/// Straight colours at every interesting alpha and channel value.
fn colors() -> Vec<u32> {
    let mut out = Vec::new();
    for a in LEVELS {
        for r in LEVELS {
            for g in [0, 128, 255] {
                out.push(rgba(r, g, LEVELS[(r % 9) as usize], a));
            }
        }
    }
    out
}

fn ch(px: u32, shift: u32) -> i64 {
    i64::from((px >> shift) & 255)
}

/// pixman's MUL_UN8, written out again.
fn mul_un8(a: i64, b: i64) -> i64 {
    let t = a * b + 128;
    ((t >> 8) + t) >> 8
}

#[test]
fn mul_un8_rounds_to_nearest_for_bytes_and_floors_for_differences() {
    for a in 0..=255i32 {
        for b in [0, 1, 127, 128, 254, 255] {
            let want = (2 * a * b + 255) / 510;
            assert_eq!(mul(a, b), want, "{a} {b}");
        }
    }
    assert_eq!(mul(-255, 255), -255);
    assert_eq!(mul(-1, 255), -1, "an arithmetic shift floors");
}

#[test]
fn normal_matches_asepritess_written_formula() {
    for back in colors() {
        for src in colors() {
            for opacity in LEVELS {
                let got = normal(back, src, opacity);
                let sa = mul_un8(ch(src, 24), i64::from(opacity));
                if back >> 24 == 0 {
                    let want = (src & 0x00ff_ffff) | ((sa as u32) << 24);
                    assert_eq!(got, if sa == 0 { 0 } else { want });
                    continue;
                }
                if src >> 24 == 0 {
                    assert_eq!(got, back);
                    continue;
                }
                let ba = ch(back, 24);
                let ra = sa + ba - mul_un8(ba, sa);
                assert_eq!(i64::from(got >> 24), ra);
                for shift in [0, 8, 16] {
                    let (b, s) = (ch(back, shift), ch(src, shift));
                    // C's division truncates toward zero, as Rust's does.
                    assert_eq!(
                        ch(got, shift),
                        b + (s - b) * sa / ra,
                        "{back:08x} {src:08x}"
                    );
                }
            }
        }
    }
}

#[test]
fn an_opaque_normal_source_replaces_and_a_clear_one_changes_nothing() {
    for back in colors() {
        assert_eq!(normal(back, 0xff12_3456, 255), 0xff12_3456);
        assert_eq!(normal(back, 0, 255), tidy(back));
        for blend in BlendMode::ALL {
            assert_eq!(mix(back, 0, blend, 255), tidy(back), "{blend:?}");
        }
    }
}

#[test]
fn every_mode_over_a_clear_backdrop_is_the_source_and_over_an_opaque_one_is_single_stage() {
    for src in colors() {
        for blend in BlendMode::ALL {
            for opacity in LEVELS {
                assert_eq!(
                    mix(0, src, blend, opacity),
                    normal(0, src, opacity),
                    "{blend:?}"
                );
                let Some(f) = mode_color(blend) else {
                    continue;
                };
                for back in [0xff00_0000, 0xff80_40c0, 0xffff_ffff] {
                    let single = normal(back, f(back, src) | (src & 0xff00_0000), opacity);
                    assert_eq!(mix(back, src, blend, opacity), single, "{blend:?}");
                }
            }
        }
    }
}

#[test]
fn merge_moves_by_the_weight_and_keeps_the_side_that_shows() {
    let (a, b) = (rgba(10, 20, 30, 40), rgba(200, 100, 0, 240));
    assert_eq!(merge(a, b, 0), a);
    assert_eq!(merge(a, b, 255), b);
    assert_eq!(merge(0, b, 128), rgba(200, 100, 0, 120));
    assert_eq!(merge(a, 0x00ff_ffff, 128), rgba(10, 20, 30, 20));
    assert_eq!(merge(a, 0, 255), 0, "no alpha, no colour");
}

#[test]
fn a_mode_over_a_translucent_backdrop_is_the_two_stage_form() {
    // Multiply of white at 50% over half-clear grey: the normal result moved toward the
    // single-stage one by the backdrop's alpha, then by both alphas.
    let (back, src) = (rgba(128, 128, 128, 128), rgba(255, 64, 0, 128));
    let plain = normal(back, src, 255);
    let mode = normal(back, multiply(back, src) | 0x8000_0000, 255);
    let want = merge(merge(plain, mode, 128), mode, mul(128, 128));
    assert_eq!(mix(back, src, BlendMode::Multiply, 255), want);
    assert_ne!(want, plain);
    assert_ne!(want, mode);
}

/// One channel of a mode, as its definition writes it.
type Channel = fn(i64, i64) -> i64;

#[test]
fn the_separable_mode_colours_match_their_definitions() {
    let cases: [(BlendMode, Channel); 7] = [
        (BlendMode::Multiply, mul_un8),
        (BlendMode::Screen, |b, s| b + s - mul_un8(b, s)),
        (BlendMode::Darken, i64::min),
        (BlendMode::Lighten, i64::max),
        (BlendMode::Difference, |b, s| (b - s).abs()),
        (BlendMode::Exclusion, |b, s| b + s - 2 * mul_un8(b, s)),
        (BlendMode::Addition, |b, s| (b + s).min(255)),
    ];
    for (blend, f) in cases {
        let color = mode_color(blend).expect("a mode");
        for b in 0..=255u32 {
            for s in (0..=255u32).step_by(5) {
                assert_eq!(i64::from(color(b, s) & 255), f(i64::from(b), i64::from(s)));
            }
        }
    }
}

#[test]
fn dodge_burn_divide_and_lights_hold_their_ends() {
    let one = |blend: BlendMode, b: u32, s: u32| mode_color(blend).expect("mode")(b, s) & 255;
    assert_eq!(one(BlendMode::ColorDodge, 0, 255), 0);
    assert_eq!(one(BlendMode::ColorDodge, 10, 255), 255);
    assert_eq!(one(BlendMode::ColorDodge, 100, 0), 100);
    assert_eq!(one(BlendMode::ColorBurn, 255, 0), 255);
    assert_eq!(one(BlendMode::ColorBurn, 100, 255), 100);
    assert_eq!(one(BlendMode::ColorBurn, 100, 0), 0);
    assert_eq!(one(BlendMode::Divide, 0, 0), 0);
    assert_eq!(one(BlendMode::Divide, 50, 100), 128);
    assert_eq!(one(BlendMode::Divide, 100, 50), 255);
    assert_eq!(one(BlendMode::Subtract, 50, 100), 0);
    assert_eq!(one(BlendMode::HardLight, 200, 0), 0);
    assert_eq!(one(BlendMode::HardLight, 200, 255), 255);
    assert_eq!(one(BlendMode::Overlay, 0, 200), 0);
    assert_eq!(one(BlendMode::SoftLight, 128, 128), 128);
    assert_eq!(one(BlendMode::SoftLight, 0, 255), 0);
    assert_eq!(one(BlendMode::SoftLight, 255, 0), 255);
}

#[test]
fn every_result_is_a_tidy_straight_pixel() {
    for back in colors() {
        for src in colors() {
            for blend in BlendMode::ALL {
                let got = mix(back, src, blend, 200);
                assert!(got >> 24 != 0 || got == 0, "{blend:?} {got:08x}");
            }
        }
    }
}

#[test]
fn the_screen_sees_the_premultiplied_form_and_back() {
    for px in colors() {
        let shown = to_screen(px);
        assert_eq!(shown, crate::blend::over(0, px, 255));
        if px >> 24 == 255 {
            assert_eq!(from_screen(shown), px);
        }
        if px >> 24 == 0 {
            assert_eq!(shown, 0);
        }
    }
}
