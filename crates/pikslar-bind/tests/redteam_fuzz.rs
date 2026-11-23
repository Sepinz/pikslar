//! M12 red team: a grammar-aware fuzzer for scripts. Each case is a generated program or a
//! mutation of an example script, run on the test project under small, varied limits and
//! capabilities. Every case must:
//!
//! - never panic (a panic shows as a `Thread` failure);
//! - end within a time bound;
//! - keep the memory meter within its cap, and the fuel within its allowance;
//! - leave the document, history, revision and saved bytes exactly as they were on failure,
//!   but for the saved record of ids it used up (`Nids`);
//! - change nothing at all when the run may only read;
//! - on success, be one undo step that takes the saved bytes back to what they were, ids
//!   used up aside.
//!
//! `PIKSLAR_FUZZ_CASES` sets the number of cases (default 2,000) and `PIKSLAR_FUZZ_SEED` the
//! seed. The totals and timings are printed; run with `--nocapture`.

mod common;
mod gen;

use common::{but_ids, project, state};
use gen::corpus::{mutate, CORPUS};
use gen::{program, Rng};
use pikslar_bind::{run_script, Capabilities, Capability, Failure, Outcome, ScriptLimits};
use pikslar_core::{Editor, StepCmd};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

const MOST_TIME: Duration = Duration::from_secs(5);

const CAPS: [Capabilities; 4] = [
    Capabilities::NONE,
    Capabilities::READ,
    Capabilities::ALL,
    Capabilities::NONE.with(Capability::EditDocument),
];

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn limits(rng: &mut Rng) -> ScriptLimits {
    ScriptLimits {
        fuel: *rng.pick(&[10_000, 50_000, 200_000, 1_000_000]),
        memory: *rng.pick(&[1 << 20, 2 << 20, 4 << 20]),
        call_depth: *rng.pick(&[1, 8, 64, 256]),
        ..ScriptLimits::DEFAULT
    }
}

#[derive(Default)]
struct Totals {
    cases: u64,
    compiled: u64,
    succeeded: u64,
    changed: u64,
    stops: u64,
    refusals: u64,
    slowest: Duration,
    slowest_case: u64,
}

/// What a successful run must leave: one undo step back to the bytes before, unless it
/// changed visibility, which undo does not take back.
/// `plain` is `before` without the saved ids.
fn check_success(
    e: &mut Editor,
    src: &str,
    (before, plain): (&common::State, &common::State),
    caps: Capabilities,
) {
    let after = state(e);
    if !caps.has(Capability::EditDocument) {
        assert_eq!(
            &after, before,
            "a run that may not edit changed the document"
        );
        return;
    }
    assert!(after.undo <= before.undo + 1, "more than one step");
    if after.revision != before.revision && after.undo == before.undo + 1 {
        e.apply(StepCmd::Undo).expect("undoes");
        if !src.contains("set_visible") {
            assert_eq!(
                but_ids(e).bytes,
                plain.bytes,
                "undo did not restore the bytes"
            );
        }
    }
}

fn one_case(n: u64, rng: &mut Rng, totals: &mut Totals) {
    let generated = rng.chance(50);
    // The corpus is written for read and edit, so it mostly gets them.
    let caps = match generated || rng.chance(25) {
        true => *rng.pick(&CAPS),
        false => Capabilities::ALL,
    };
    let src = match generated {
        true => program(rng, caps),
        false => mutate(rng.pick(&CORPUS), rng, caps),
    };
    let lim = limits(rng);
    let mut e = project();
    let before = state(&e);
    let but_ids_of_before = but_ids(&e);
    let t = Instant::now();
    let out: Outcome = run_script(&mut e, &src, caps, lim, &AtomicBool::new(false));
    let took = t.elapsed();
    let case = format!("case {n}, {caps:?}, {lim:?}:\n{src}");
    assert!(took < MOST_TIME, "{case}\ntook {took:?}");
    assert!(
        out.peak_memory <= lim.memory,
        "{case}\npeak {}",
        out.peak_memory
    );
    assert!(out.fuel_used <= lim.fuel, "{case}\nfuel {}", out.fuel_used);
    if took > totals.slowest {
        (totals.slowest, totals.slowest_case) = (took, n);
    }
    totals.cases += 1;
    match &out.result {
        Ok(_) => {
            totals.compiled += 1;
            totals.succeeded += 1;
            totals.changed += u64::from(e.revision() != before.revision);
            check_success(&mut e, &src, (&before, &but_ids_of_before), caps);
        }
        Err(f) => {
            assert!(!matches!(f, Failure::Thread(_)), "{case}\n{f}");
            let compiled = !matches!(f, Failure::Script(s) if matches!(s.kind,
                pikslar_script::ErrorKind::Syntax | pikslar_script::ErrorKind::TooLarge));
            totals.compiled += u64::from(compiled);
            totals.stops += u64::from(f.is_stop());
            totals.refusals += u64::from(matches!(f, Failure::Refused { .. }));
            if std::env::var("PIKSLAR_FUZZ_SHOW").is_ok() {
                println!("failed {n}: {f}");
            }
            assert_eq!(
                but_ids(&e),
                but_ids_of_before,
                "{case}\nfailed with {f} but changed things"
            );
        }
    }
}

#[test]
fn generated_and_mutated_scripts_hold_every_guarantee() {
    let cases = env("PIKSLAR_FUZZ_CASES", 2_000);
    let seed = env("PIKSLAR_FUZZ_SEED", 0x5eed_1234_abcd_0001) | 1;
    let mut rng = Rng(seed);
    let mut totals = Totals::default();
    let t = Instant::now();
    for n in 0..cases {
        one_case(n, &mut rng, &mut totals);
    }
    let all = t.elapsed();
    println!(
        "{} cases in {:.1} s ({:.2} ms each): {} compiled, {} succeeded ({} changed), {} stops, \
         {} refusals; slowest {:.1} ms (case {}); seed {seed:#x}",
        totals.cases,
        all.as_secs_f64(),
        all.as_secs_f64() * 1e3 / totals.cases.max(1) as f64,
        totals.compiled,
        totals.succeeded,
        totals.changed,
        totals.stops,
        totals.refusals,
        totals.slowest.as_secs_f64() * 1e3,
        totals.slowest_case,
    );
    assert!(totals.compiled * 4 > totals.cases, "too few cases compiled");
    assert!(totals.succeeded > 0 && totals.changed > 0 && totals.refusals > 0);
}
