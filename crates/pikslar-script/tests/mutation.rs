//! Mutated scripts never panic: each one either fails to compile with an error, or runs to
//! a result or an error within small limits. Deterministic: a fixed xorshift sequence.

mod common;

use common::*;
use pikslar_script::{compile, run, Limits};
use std::sync::atomic::AtomicBool;

const SEEDS: [&str; 5] = [
    "fn f(n) { if n < 2 { return n; } return f(n - 1) + f(n - 2); } let l = [1, 2.5, \"a\"]; for v in l { print(str(v)); } return f(10);",
    "let s = repeat(\"ab\", 10); let i = find(s, \"ba\"); let t = slice(s, 1, 5) + str(i); while len(t) < 50 { t = t + t; } return t;",
    "let l = list(10, 0); for i in 0..10 { l[i] = i * i % 7; } insert(l, 0, -1); remove(l, 3); return [pop(l), min(1, 2.0), abs(-3), floor(2.7)];",
    "let y = 0; while y < 8 { let x = 0; while x < 8 { set_pixel(x, y, x + y * 256); x = x + 1; } y = y + 1; } fill_rect(0, 0, 4, 4, 0xff00ff); return get_pixel(1, 1);",
    "fn g(a, b) { return a && !b || false; } let k = 0; for i in 0..20 { if g(i % 2 == 0, i > 10) { k = k + 1; continue; } else if i == 15 { break; } } return int(\"12\") + k;",
];

const PIECES: [&str; 16] = [
    "(",
    ")",
    "{",
    "}",
    "[",
    "]",
    ";",
    ",",
    "\"",
    "..",
    "fn",
    "let",
    "return",
    "-",
    "9223372036854775807",
    "0x",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn mutate(src: &str, rng: &mut Rng) -> String {
    let mut s = src.to_string();
    for _ in 0..1 + rng.below(2) {
        let mut at = rng.below(s.len() + 1);
        while !s.is_char_boundary(at) {
            at -= 1;
        }
        match rng.below(3) {
            0 => s.insert_str(at, PIECES[rng.below(PIECES.len())]),
            1 => {
                let end = (at + 1 + rng.below(6)).min(s.len());
                if s.is_char_boundary(end) {
                    s.replace_range(at..end, "");
                }
            }
            _ => s.insert(at, char::from(32 + rng.below(95) as u8)),
        }
    }
    s
}

#[test]
fn mutated_scripts_fail_cleanly() {
    let limits = Limits {
        fuel: 20_000,
        memory: 1 << 20,
        ..Limits::default()
    };
    for seed in SEEDS {
        let p = compile(seed, &table(), &limits).unwrap_or_else(|e| panic!("{seed}: {e}"));
        let r = run(&p, &mut Canvas::new(), &limits, &AtomicBool::new(false));
        assert!(r.result.is_ok(), "{seed}: {:?}", r.result);
    }
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut compiled, mut total) = (0, 0);
    for round in 0..20_000 {
        let src = mutate(SEEDS[round % SEEDS.len()], &mut rng);
        total += 1;
        if let Ok(p) = compile(&src, &table(), &limits) {
            compiled += 1;
            let r = run(&p, &mut Canvas::new(), &limits, &AtomicBool::new(false));
            assert!(r.peak_memory <= limits.memory);
            assert!(r.fuel_used <= limits.fuel);
        }
    }
    println!("{total} mutated scripts, {compiled} compiled and ran");
    assert!(compiled > 500);
}
