//! The position tween: offsets between two keys, eased over the frames' durations.
//!
//! The arithmetic is in whole numbers only, so every machine gives the same offsets. The rules
//! are in `docs/links-and-tween.md`, section 11.

/// How the share of the move grows over time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ease {
    /// The same speed throughout.
    Linear,
    /// Starts slow and speeds up.
    In,
    /// Starts fast and slows down.
    Out,
    /// Slow at both ends.
    InOut,
}

impl Ease {
    /// Every ease, in the order menus offer them.
    pub const ALL: [Ease; 4] = [Ease::Linear, Ease::In, Ease::Out, Ease::InOut];

    /// The share of the move made `n` units into a run `d` units long, as `(num, den)`.
    /// `0 <= n <= d` and `d > 0`.
    fn share(self, n: i128, d: i128) -> (i128, i128) {
        let den = d * d;
        let num = match self {
            Ease::Linear => return (n, d),
            Ease::In => n * n,
            Ease::Out => n * (2 * d - n),
            Ease::InOut if 2 * n <= d => 2 * n * n,
            Ease::InOut => den - 2 * (d - n) * (d - n),
        };
        (num, den)
    }
}

/// `a / den` to the nearest whole number, halves away from zero. `den > 0`.
fn round_div(a: i128, den: i128) -> i128 {
    let away = (2 * a.abs() + den) / (2 * den);
    a.signum() * away
}

/// One axis: `from + round(delta * share)`.
fn between(from: i32, to: i32, (num, den): (i128, i128)) -> i32 {
    let delta = i128::from(to) - i128::from(from);
    let moved = round_div(delta * num, den);
    // The share is from 0 to 1, so the result lies between the keys and fits.
    i32::try_from(i128::from(from) + moved).unwrap_or(from)
}

/// The offset of every frame of a run, keys included, given how long each frame of the run
/// shows. The first and last frames are the keys and keep `first` and `last`. The last frame's
/// own duration does not matter: the move ends when it starts. Durations of 0 count as 1.
pub fn tween_offsets(
    durations: &[u32],
    first: (i32, i32),
    last: (i32, i32),
    ease: Ease,
) -> Vec<(i32, i32)> {
    let Some((_, before_last)) = durations.split_last() else {
        return Vec::new();
    };
    let length = |units: &[u32]| units.iter().map(|&u| i128::from(u.max(1))).sum::<i128>();
    let d = length(before_last).max(1);
    let mut n = 0i128;
    let mut offsets = Vec::with_capacity(durations.len());
    for (i, &units) in durations.iter().enumerate() {
        let share = match i {
            0 => (0, 1),
            _ if i + 1 == durations.len() => (1, 1),
            _ => ease.share(n.min(d), d),
        };
        offsets.push((
            between(first.0, last.0, share),
            between(first.1, last.1, share),
        ));
        n += i128::from(units.max(1));
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xs(durations: &[u32], to: i32, ease: Ease) -> Vec<i32> {
        tween_offsets(durations, (0, 0), (to, 0), ease)
            .into_iter()
            .map(|(x, y)| {
                assert_eq!(y, 0, "y is unchanged");
                x
            })
            .collect()
    }

    const FIVE: [u32; 5] = [100; 5];

    #[test]
    fn the_tables_of_the_plan_for_five_frames_of_100_ms() {
        assert_eq!(xs(&FIVE, 4, Ease::Linear), [0, 1, 2, 3, 4]);
        assert_eq!(xs(&FIVE, 3, Ease::Linear), [0, 1, 2, 2, 3]);
        assert_eq!(xs(&FIVE, -3, Ease::Linear), [0, -1, -2, -2, -3]);
        assert_eq!(xs(&FIVE, 8, Ease::In), [0, 1, 2, 5, 8]);
        assert_eq!(xs(&FIVE, 8, Ease::Out), [0, 4, 6, 8, 8]);
        assert_eq!(xs(&FIVE, 8, Ease::InOut), [0, 1, 4, 7, 8]);
    }

    #[test]
    fn a_long_frame_moves_further_than_a_short_one() {
        assert_eq!(xs(&[100, 300, 100, 100], 10, Ease::Linear), [0, 2, 8, 10]);
    }

    #[test]
    fn both_axes_are_rounded_on_their_own() {
        let got = tween_offsets(&FIVE, (0, 0), (4, -2), Ease::Linear);
        assert_eq!(got, [(0, 0), (1, -1), (2, -1), (3, -2), (4, -2)]);
    }

    #[test]
    fn equal_keys_put_every_frame_at_that_place() {
        let got = tween_offsets(&FIVE, (7, -3), (7, -3), Ease::InOut);
        assert_eq!(got, [(7, -3); 5]);
    }

    #[test]
    fn halves_go_away_from_zero() {
        assert_eq!(round_div(1, 2), 1);
        assert_eq!(round_div(-1, 2), -1);
        assert_eq!(round_div(3, 2), 2);
        assert_eq!(round_div(-3, 2), -2);
        assert_eq!(round_div(1, 3), 0);
        assert_eq!(round_div(-2, 3), -1);
        assert_eq!(round_div(0, 7), 0);
    }

    #[test]
    fn the_largest_move_over_the_longest_run_does_not_overflow() {
        let limit = crate::layer::OFFSET_LIMIT;
        let durations = vec![crate::animation::MAX_DURATION_MS; crate::document::MAX_FRAMES];
        for ease in Ease::ALL {
            let got = tween_offsets(&durations, (-limit, limit), (limit, -limit), ease);
            assert_eq!(got.len(), durations.len());
            assert_eq!(got[0], (-limit, limit));
            assert_eq!(got[got.len() - 1], (limit, -limit));
            assert!(got.windows(2).all(|w| w[0].0 <= w[1].0 && w[0].1 >= w[1].1));
        }
    }

    /// The exact share as a rational, worked out a second way, from the plan's curves in terms
    /// of `t = n / d`, so a slip in the expanded forms above shows.
    fn oracle(ease: Ease, n: i128, d: i128, delta: i128) -> i128 {
        // Each curve as `p(t) / q` with t = n / d, over the common denominator d².
        let (num, den) = match ease {
            Ease::Linear => (n * d, d * d),
            Ease::In => (n * n, d * d),
            Ease::Out => (d * d - (d - n) * (d - n), d * d),
            Ease::InOut if 2 * n <= d => (2 * n * n, d * d),
            Ease::InOut => (d * d - 2 * (d - n) * (d - n), d * d),
        };
        // Nearest, halves away from zero, by comparing twice the remainder.
        let a = delta * num;
        let (q, r) = (a.abs() / den, a.abs() % den);
        let q = if 2 * r >= den { q + 1 } else { q };
        if a < 0 {
            -q
        } else {
            q
        }
    }

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
    }

    #[test]
    fn a_rational_oracle_agrees_on_100_000_random_runs() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let limit = i64::from(crate::layer::OFFSET_LIMIT);
        let span = (2 * limit + 1) as u64;
        for case in 0..100_000 {
            let frames = 3 + rng.below(8) as usize;
            let durations: Vec<u32> = (0..frames)
                .map(|_| 1 + rng.below(if case % 2 == 0 { 400 } else { 60_000 }) as u32)
                .collect();
            let key = |rng: &mut Rng| (rng.below(span) as i64 - limit) as i32;
            let (a, b) = (
                (key(&mut rng), key(&mut rng)),
                (key(&mut rng), key(&mut rng)),
            );
            let ease = Ease::ALL[rng.below(4) as usize];
            let got = tween_offsets(&durations, a, b, ease);
            let d: i128 = durations[..frames - 1].iter().map(|&u| i128::from(u)).sum();
            let mut n = 0i128;
            for (i, &(x, y)) in got.iter().enumerate() {
                let want = |from: i32, to: i32| {
                    let delta = i128::from(to) - i128::from(from);
                    (i128::from(from) + oracle(ease, n, d, delta)) as i32
                };
                assert_eq!(
                    (x, y),
                    (want(a.0, b.0), want(a.1, b.1)),
                    "case {case}, frame {i}"
                );
                n += i128::from(durations[i]);
            }
        }
    }

    #[test]
    fn offsets_never_go_back_and_left_mirrors_right() {
        let mut rng = Rng(12345);
        for _ in 0..2_000 {
            let frames = 3 + rng.below(20) as usize;
            let durations: Vec<u32> = (0..frames).map(|_| 1 + rng.below(500) as u32).collect();
            let to = rng.below(2_000) as i32;
            for ease in Ease::ALL {
                let right = xs(&durations, to, ease);
                let left = xs(&durations, -to, ease);
                assert!(right.windows(2).all(|w| w[0] <= w[1]), "{ease:?} {right:?}");
                let mirrored: Vec<i32> = right.iter().map(|x| -x).collect();
                assert_eq!(left, mirrored);
                assert_eq!((right[0], right[frames - 1]), (0, to), "the keys stay");
            }
        }
    }
}
