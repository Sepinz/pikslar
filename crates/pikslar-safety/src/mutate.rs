//! Repeatable random damage. The same numbers always produce the same file.

/// A small, seeded generator (xorshift64*).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number below `n`, or 0 when `n` is 0.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        ((self.next_u64() >> 32) as usize * n) >> 32
    }

    /// A number from `low` to `high`, both included.
    pub fn between(&mut self, low: i32, high: i32) -> i32 {
        low + self.below((high - low + 1).max(1) as usize) as i32
    }

    pub fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }
}

/// Values most likely to break a size or an index.
const AWKWARD: [u32; 16] = [
    0,
    1,
    2,
    15,
    16,
    17,
    255,
    256,
    4096,
    8192,
    8193,
    65_536,
    0x7fff_ffff,
    0x8000_0000,
    0xffff_fff0,
    0xffff_ffff,
];

fn position(rng: &mut Rng, len: usize) -> usize {
    // Counts and sizes are near the start, and a small file has little else.
    if rng.chance(3) {
        rng.below(len.min(256))
    } else {
        rng.below(len)
    }
}

fn apply(rng: &mut Rng, out: &mut Vec<u8>) {
    if out.is_empty() {
        out.push(rng.next_u64() as u8);
        return;
    }
    let at = position(rng, out.len());
    match rng.below(8) {
        0 => out[at] ^= 1 << rng.below(8),
        1 => out[at] = rng.next_u64() as u8,
        2 => out[at] = [0u8, 1, 0x7f, 0x80, 0xff][rng.below(5)],
        3 | 4 => {
            let number = AWKWARD[rng.below(AWKWARD.len())];
            // PNG stores numbers the other way round.
            let value = if rng.chance(2) {
                number.to_le_bytes()
            } else {
                number.to_be_bytes()
            };
            let end = (at + 4).min(out.len());
            out[at..end].copy_from_slice(&value[..end - at]);
        }
        5 => {
            let end = (at + 1 + rng.below(64)).min(out.len());
            out.drain(at..end);
        }
        6 => {
            let end = (at + 1 + rng.below(64)).min(out.len());
            let copy = out[at..end].to_vec();
            let target = rng.below(out.len());
            out.splice(target..target, copy);
        }
        _ => out.truncate(at),
    }
}

/// A damaged copy of `input`.
pub fn mutate(rng: &mut Rng, input: &[u8]) -> Vec<u8> {
    let mut out = input.to_vec();
    for _ in 0..1 + rng.below(4) {
        apply(rng, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_damage() {
        let input = vec![7u8; 500];
        let first = mutate(&mut Rng::new(42), &input);
        assert_eq!(first, mutate(&mut Rng::new(42), &input));
    }

    #[test]
    fn different_seeds_give_different_damage() {
        let input = vec![7u8; 500];
        let found = (0..32)
            .map(|seed| mutate(&mut Rng::new(seed), &input))
            .filter(|out| *out != input)
            .count();
        assert!(found > 24);
    }

    #[test]
    fn ranges_are_respected() {
        let mut rng = Rng::new(9);
        for _ in 0..1000 {
            assert!(rng.below(7) < 7);
            assert!((-3..=3).contains(&rng.between(-3, 3)));
        }
        assert_eq!(rng.below(0), 0);
    }

    #[test]
    fn empty_input_survives() {
        let mut rng = Rng::new(1);
        for _ in 0..64 {
            let _ = mutate(&mut rng, &[]);
        }
    }
}
