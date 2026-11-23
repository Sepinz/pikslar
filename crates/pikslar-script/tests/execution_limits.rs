//! Every hostile script stops: fuel covers statements, expressions, builtin work and host
//! work; calls have a depth limit; the cancel flag stops a run quickly. Each stop time is
//! measured and printed (run with `--nocapture`); the asserted bounds are loose so a slow
//! unoptimised build passes, and the printed figures are the evidence.

mod common;

use common::*;
use pikslar_script::{
    compile, run, ErrorKind, Limits, NoHost, RunReport, CANCEL_INTERVAL, MAX_CALL_DEPTH, MAX_NEST,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const FUEL: u64 = 10_000_000;

fn limits() -> Limits {
    Limits {
        fuel: FUEL,
        ..Limits::default()
    }
}

fn stopped(name: &str, src: &str, want: ErrorKind) -> (RunReport, Duration) {
    let t = Instant::now();
    let r = report(src, &limits());
    let took = t.elapsed();
    let e = r.result.as_ref().map(|_| ()).expect_err(name);
    assert_eq!(e.kind, want, "{name}: {e}");
    assert!(r.fuel_used <= FUEL, "{name}: burnt {}", r.fuel_used);
    assert!(took < Duration::from_secs(20), "{name} took {took:?}");
    println!(
        "{name}: {:?} after {:.1} ms, fuel {}, at {}",
        e.kind,
        took.as_secs_f64() * 1e3,
        r.fuel_used,
        e.pos.line
    );
    (r, took)
}

#[test]
fn runaway_loops_run_out_of_fuel() {
    stopped("empty loop", "while true { }", ErrorKind::Fuel);
    stopped(
        "for loop",
        "for i in 0..9223372036854775807 { }",
        ErrorKind::Fuel,
    );
    stopped(
        "loop in nested functions",
        "fn a() { b(); } fn b() { c(); } fn c() { while true { let x = 1; } } a();",
        ErrorKind::Fuel,
    );
    // Each turn adds an item, so the cap is reached before the fuel runs out.
    stopped(
        "for over a growing list",
        "let l = [1]; for v in l { push(l, v); }",
        ErrorKind::Memory,
    );
    stopped(
        "continue loop",
        "let i = 0; while true { i = i + 1; if i > 0 { continue; } }",
        ErrorKind::Fuel,
    );
}

#[test]
fn big_expressions_cost_by_their_size() {
    // One statement with many nodes burns fuel for each.
    let expr = vec!["1"; 60].join(" + ");
    let src = format!("let n = 0; while n < 1000 {{ let x = {expr}; n = n + 1; }}");
    let r = report(&src, &limits());
    assert!(r.result.is_ok());
    assert!(r.fuel_used > 1000 * 60, "burnt only {}", r.fuel_used);
}

#[test]
fn string_work_is_metered_per_byte() {
    let setup =
        "let s = repeat(\"x\", 1000000); let t = s + \"\"; let u = repeat(\"x\", 999999) + \"y\";";
    stopped(
        "search a long string",
        &format!("{setup} while true {{ let i = find(s, \"xxxxy\"); }}"),
        ErrorKind::Fuel,
    );
    stopped(
        "compare long strings",
        &format!("{setup} while true {{ let b = t == u; }}"),
        ErrorKind::Fuel,
    );
    stopped(
        "order long strings",
        &format!("{setup} while true {{ let b = t < u; }}"),
        ErrorKind::Fuel,
    );
    stopped(
        "slice long strings",
        &format!("{setup} while true {{ let p = slice(s, 1, 999999); }}"),
        ErrorKind::Fuel,
    );
    stopped(
        "render and parse",
        &format!("{setup} while true {{ let p = str(s); let q = int(\"12345\"); }}"),
        ErrorKind::Fuel,
    );
    // A search over a megabyte costs at least a unit for every 16 bytes.
    let r = report(
        &format!("{setup} let i = find(s, \"y\"); return i;"),
        &limits(),
    );
    assert!(r.fuel_used >= 1_000_000 / 16);
}

#[test]
fn list_work_is_metered_per_item() {
    let setup = "let l = list(100000, 1); let m = list(100000, 1);";
    stopped(
        "quadratic inserts",
        &format!("{setup} while true {{ insert(l, 0, 2); remove(l, 0); }}"),
        ErrorKind::Fuel,
    );
    stopped(
        "list equality",
        &format!("{setup} while true {{ let b = l == m; }}"),
        ErrorKind::Fuel,
    );
    stopped(
        "list search",
        &format!("{setup} while true {{ let i = find(l, 5); }}"),
        ErrorKind::Fuel,
    );
    stopped(
        "list slices",
        &format!("{setup} while true {{ let k = slice(l, 0, 100000); }}"),
        ErrorKind::Fuel,
    );
    stopped(
        "list rendering",
        &format!("{setup} while true {{ let k = str(l); }}"),
        ErrorKind::Fuel,
    );
}

#[test]
fn host_work_is_charged_by_the_host() {
    let (r, _) = stopped(
        "bulk fills",
        "while true { fill_rect(0, 0, 256, 256, 1); }",
        ErrorKind::Fuel,
    );
    // 4,097 units per fill: the fills, not the loop, used the fuel.
    assert!(r.fuel_used > FUEL - 5000);
}

#[test]
fn deep_recursion_hits_the_call_depth_limit() {
    on_script_thread(|| {
        stopped(
            "deep recursion",
            "fn f(n) { return f(n + 1); } f(0);",
            ErrorKind::CallDepth,
        );
        stopped(
            "mutual recursion",
            "fn a(n) { return b(n); } fn b(n) { return a(n + 1); } a(0);",
            ErrorKind::CallDepth,
        );
    });
}

/// The deepest native recursion the limits allow: the most calls, each nested as deeply as
/// the parser allows, on the recommended stack. Overflowing would abort this test binary.
#[test]
fn the_worst_case_stack_fits_the_recommended_size() {
    let depth = (MAX_NEST - 6) as usize;
    let src = format!(
        "fn f(n) {{ {} return f(n + 1); {} }} f(0);",
        "if true { ".repeat(depth),
        "}".repeat(depth)
    );
    let limits = Limits {
        call_depth: MAX_CALL_DEPTH,
        ..limits()
    };
    let p = compile(&src, &table(), &limits).expect("compiles at the nesting limit");
    let bytes = on_script_thread(move || {
        let r = run(&p, &mut NoHost, &limits, &AtomicBool::new(false));
        assert_eq!(r.result.expect_err("too deep").kind, ErrorKind::CallDepth);
        r.stack_bytes
    });
    println!(
        "worst-case stack: {bytes} B of {} B",
        pikslar_script::RECOMMENDED_STACK
    );
    assert!(bytes < pikslar_script::RECOMMENDED_STACK);
    // Unoptimised frames are large enough that the budget, not the depth, stops this run.
    assert!(!cfg!(debug_assertions) || bytes > pikslar_script::STACK_BUDGET / 2);
}

#[test]
fn cancel_stops_a_run_quickly_with_its_own_error() {
    let p = compile(
        "let i = 0; while true { i = i + 1; }",
        &table(),
        &Limits::default(),
    )
    .expect("compiles");
    let limits = Limits {
        fuel: u64::MAX,
        ..Limits::default()
    };
    let flag = Arc::new(AtomicBool::new(false));
    let seen = flag.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        tx.send(()).expect("main waits");
        let r = run(&p, &mut NoHost, &limits, &seen);
        (
            r.result.map_err(|e| e.kind).err(),
            Instant::now(),
            r.fuel_used,
        )
    });
    rx.recv().expect("worker started");
    std::thread::sleep(Duration::from_millis(50));
    let set = Instant::now();
    flag.store(true, Ordering::Relaxed);
    let (kind, stopped_at, fuel) = worker.join().expect("worker ends");
    let latency = stopped_at.duration_since(set);
    assert_eq!(kind, Some(ErrorKind::Cancelled));
    assert!(latency < Duration::from_millis(250), "took {latency:?}");
    println!("cancel: stopped {:.3} ms after the flag was set, after {fuel} fuel; checked every {CANCEL_INTERVAL} units", latency.as_secs_f64() * 1e3);
}

#[test]
fn a_cancelled_host_charge_stops_the_run() {
    let flag = AtomicBool::new(true);
    let r = report_with(
        "fill_rect(0, 0, 10, 10, 1);",
        &Limits::default(),
        &mut Canvas::new(),
        &flag,
    );
    assert_eq!(r.result.expect_err("cancelled").kind, ErrorKind::Cancelled);
}

#[test]
fn fuel_is_exact_for_simple_code() {
    // `let x = 1;` costs 1 + 1 node; `x = x + 1;` costs 1 + 4 (target, x, 1, +).
    let r = report("let x = 1; x = x + 1;", &limits());
    assert!(r.result.is_ok());
    assert_eq!(r.fuel_used, 2 + 5);
}

/// Runs `src` on a thread of its own and gives up after `most`: a builtin whose native loop
/// is not paid for would never come back.
fn within(src: &'static str, most: Duration) -> Option<Result<String, ErrorKind>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = report(src, &limits()).result;
        let _ = tx.send(r.map(|v| v.to_string()).map_err(|e| e.kind));
    });
    rx.recv_timeout(most).ok()
}

#[test]
fn builtins_pay_for_their_loop_counts_not_only_their_bytes() {
    // An empty string repeated has no bytes, but a native loop of n turns would not stop.
    let r = within(
        "return repeat(\"\", 9223372036854775807);",
        Duration::from_secs(10),
    );
    let r = r.expect("an empty repeat comes back at once");
    assert_eq!(r.expect("an empty string"), "");
    let r = within(
        "let n = 0; while true { let s = repeat(\"\", 9223372036854775807); n = n + 1; }",
        Duration::from_secs(10),
    );
    assert_eq!(
        r.expect("a loop of empty repeats stops"),
        Err(ErrorKind::Fuel)
    );
    // A list filled n times: charged per item before the loop, so the cap stops it.
    let r = within(
        "return list(9223372036854775807, 1);",
        Duration::from_secs(10),
    );
    assert_eq!(r.expect("a huge list comes back"), Err(ErrorKind::Memory));
}
