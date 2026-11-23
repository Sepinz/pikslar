//! M12 red team: work that fuel might not pay for. Each script runs until its fuel runs out,
//! and the time it took per fuel unit is printed (run in release with `--nocapture`). The
//! documentation says the slowest interpreter work is about 18 ns a unit, so that 50,000,000
//! units end any run within about a second. The checks compare each case with the empty loop
//! in the same build, so they hold in unoptimised and optimised builds alike.

mod common;

use common::*;
use pikslar_script::{ErrorKind, Limits};
use std::time::Instant;

const FUEL: u64 = 2_000_000;

/// The documented slowest unit (18 ns) over the empty loop (about 3 ns), with room for noise.
const MOST_TIMES_THE_LOOP: f64 = 25.0;

/// A function with 256 local slots that returns at once: each call pushes 255 nils.
fn wide_function() -> String {
    let mut s = String::from("fn wide() { return 0;");
    for i in 0..255 {
        s.push_str(&format!(" let a{i} = 0;"));
    }
    s.push_str(" }\nwhile true { wide(); }");
    s
}

/// Nanoseconds per fuel unit for a script that must run out of fuel: the best of three runs.
fn ns_per_unit(name: &str, src: &str) -> f64 {
    let limits = Limits {
        fuel: FUEL,
        ..Limits::default()
    };
    let best = (0..3)
        .map(|_| {
            let t = Instant::now();
            let r = report(src, &limits);
            let took = t.elapsed();
            let e = r.result.as_ref().map(|_| ()).expect_err(name);
            assert_eq!(e.kind, ErrorKind::Fuel, "{name}: {e}");
            took.as_secs_f64() * 1e9 / r.fuel_used.max(1) as f64
        })
        .fold(f64::MAX, f64::min);
    println!("{name}: {best:.1} ns a unit");
    best
}

fn times_the_loop(name: &str, src: &str) -> f64 {
    let base = ns_per_unit("empty loop", "while true { }");
    let ratio = ns_per_unit(name, src) / base;
    println!("{name}: {ratio:.1} times the empty loop");
    ratio
}

#[test]
fn builtin_and_allocation_work_is_paid_for() {
    let cases = [
        ("type() allocations", "while true { type(1); }"),
        (
            "str of a float",
            "let x = 1.2345678901234567; while true { str(x); }",
        ),
        (
            "slices of nothing",
            "let s = \"abc\"; while true { slice(s, 0, 0); }",
        ),
        (
            "list literals",
            "while true { let l = [nil, nil, nil, nil]; }",
        ),
        (
            "str of a list of ints",
            "let l = list(1000, 123456789); while true { str(l); }",
        ),
        (
            "equal lists of strings",
            "let a = list(1000, \"x\"); let b = list(1000, \"x\"); while true { a == b; }",
        ),
        (
            "for over a list",
            "let l = list(100, 1); while true { for v in l { } }",
        ),
        (
            "calls with sixteen arguments",
            "fn g(a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p) { return 0; }\n\
             while true { g(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16); }",
        ),
    ];
    for (name, src) in cases {
        let r = times_the_loop(name, src);
        assert!(
            r < MOST_TIMES_THE_LOOP,
            "{name}: {r:.1} times the empty loop"
        );
    }
}

#[test]
fn a_call_pays_for_the_locals_it_makes() {
    let r = times_the_loop("calls pushing 255 nils", &wide_function());
    assert!(r < MOST_TIMES_THE_LOOP, "{r:.1} times the empty loop");
}
