//! A whole run is one transaction: one undo step on success, and on any error, refusal,
//! limit or cancel, the document, undo and redo, revision and saved bytes exactly as before,
//! but for the saved record of the ids the run used up (`Nids`), which stay used up.

mod common;

use common::*;
use pikslar_bind::{run_script, Capabilities, Failure, ScriptLimits};
use pikslar_core::{ColorMode, Document, Editor, StepCmd};
use pikslar_script::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const PRELUDE: &str =
    "fn deeper(n) { return deeper(n + 1); }\nlet ls = layers();\nlet fs = frames();\n";

/// One of every kind of edit, waiting pixels and their writing among them.
const OPS: &[&str] = &[
    "rename_layer(ls[0], \"A\");",
    "add_frame(1);",
    "set_pixel(ls[0], fs[0], 1, 1, 0xff0000ff);",
    "fill_rect(ls[1], fs[1], 0, 0, 8, 8, 0xff00ff00);",
    "set_opacity(ls[0], 100);",
    "add_swatch(0xff123456);",
    "draw_line(ls[0], fs[1], 0, 0, 31, 23, 0xffffffff);",
    "set_visible(ls[1], false);",
    "add_tag(\"t\", fs[0], fs[1]);",
    "let d = duplicate_layer(ls[0]);",
    "set_pixel(ls[1], fs[0], 3, 3, 0xff00ffff);",
    "set_pixel(ls[1], fs[0], 30, 20, 0xff00ffff);",
    "fill(ls[0], fs[1], 5, 20, 0xff445566);",
    "move_layer(ls[1], 0);",
    "set_duration(fs[0], 300);",
    "recolor(swatches()[0], 0xff998877);",
    "write_rect(ls[0], fs[0], 0, 0, 2, [0xff111111, 0xff222222]);",
    "erase_pixel(ls[0], fs[0], 2, 3);",
    "set_blend(ls[0], \"Multiply\");",
    "add_layer_above(ls[0], \"Over\");",
    "set_pixel(ls[0], fs[1], 9, 9, 0xff777777);",
];

/// Ways a run can end without keeping anything, with the kind each must report.
const FAILURES: &[(&str, Option<ErrorKind>)] = &[
    ("remove_layer(999999);", None),
    ("set_pixel(ls[0], fs[0], 99, 0, 0xffffffff);", None),
    ("let z = 1 / 0;", Some(ErrorKind::Arithmetic)),
    ("rename_layer(ls[0], 5);", Some(ErrorKind::Host)),
    (
        "let big = []; while true { push(big, 1); }",
        Some(ErrorKind::Memory),
    ),
    ("deeper(0);", Some(ErrorKind::CallDepth)),
    ("while true { }", Some(ErrorKind::Fuel)),
];

fn script(ops: &[&str], insert: Option<(usize, &str)>) -> String {
    let mut lines: Vec<&str> = ops.to_vec();
    if let Some((at, line)) = insert {
        lines.insert(at, line);
    }
    format!("{PRELUDE}{}\n", lines.join("\n"))
}

fn limits() -> ScriptLimits {
    ScriptLimits {
        fuel: 2_000_000,
        memory: 2 << 20,
        call_depth: 64,
        ..ScriptLimits::DEFAULT
    }
}

#[test]
fn the_whole_script_is_one_step_that_undoes_and_redoes_exactly() {
    let mut e = project();
    let before = but_ids(&e);
    // Visibility is never undone, as for the commands, so it is left out here.
    let ops: Vec<&str> = OPS
        .iter()
        .copied()
        .filter(|op| !op.starts_with("set_visible"))
        .collect();
    let out = run_with(&mut e, &script(&ops, None), Capabilities::ALL, limits());
    assert!(out.result.is_ok(), "{:?}", out.result);
    let after = save(&e);
    assert_ne!(after, before.bytes);
    assert_eq!(e.history().undo_depth(), before.undo + 1);
    assert_eq!(e.history().redo_depth(), 0);
    for _ in 0..3 {
        e.apply(StepCmd::Undo).expect("undoes");
        assert_eq!(but_ids(&e).bytes, before.bytes);
        e.apply(StepCmd::Redo).expect("redoes");
        assert_eq!(save(&e), after);
    }
}

#[test]
fn a_failure_at_every_operation_leaves_everything_as_it_was() {
    let mut checked = 0;
    for &(failure, kind) in FAILURES {
        for at in 0..=OPS.len() {
            let mut e = project();
            let before = but_ids(&e);
            let src = script(OPS, Some((at, failure)));
            let out = run_with(&mut e, &src, Capabilities::ALL, limits());
            let got = out.result.as_ref().err().map(Failure::kind);
            match kind {
                Some(k) => assert_eq!(got, Some(Some(k)), "{failure} at {at}"),
                None => assert!(
                    matches!(out.result, Err(Failure::Refused { .. })),
                    "{failure} at {at}: {:?}",
                    out.result
                ),
            }
            assert_eq!(but_ids(&e), before, "{failure} at {at}");
            // The run left redo as it was: the undone step still comes back.
            e.apply(StepCmd::Redo).expect("redoes");
            assert_eq!(e.doc().frames.len(), 3, "{failure} at {at}");
            checked += 1;
        }
    }
    assert_eq!(checked, FAILURES.len() * (OPS.len() + 1));
}

/// The script with work between its operations, so fuel can run out anywhere.
fn padded() -> String {
    let ops: Vec<String> = OPS
        .iter()
        .map(|op| format!("{op}\nfor i in 0..4000 {{ }}"))
        .collect();
    let refs: Vec<&str> = ops.iter().map(String::as_str).collect();
    script(&refs, None)
}

#[test]
fn running_out_of_fuel_anywhere_keeps_nothing() {
    let src = padded();
    let mut e = project();
    let whole = run_with(&mut e, &src, Capabilities::ALL, limits());
    assert!(whole.result.is_ok(), "{:?}", whole.result);
    let needed = whole.fuel_used;
    assert!(needed > 40_000, "enough work to stop in: {needed}");
    let mut stops = 0;
    for step in 0..80u64 {
        let fuel = 10_000 + (needed - 10_000) * step / 80;
        let mut e = project();
        let before = but_ids(&e);
        let lim = ScriptLimits { fuel, ..limits() };
        let out = run_with(&mut e, &src, Capabilities::ALL, lim);
        assert_eq!(
            out.result.map_err(|f| f.kind()),
            Err(Some(ErrorKind::Fuel)),
            "fuel {fuel}"
        );
        assert!(out.fuel_used <= fuel);
        assert_eq!(but_ids(&e), before, "fuel {fuel}");
        stops += 1;
    }
    assert_eq!(stops, 80);
}

/// A small deterministic generator, so a failure can be repeated.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below
    }
}

#[test]
fn a_cancel_at_any_moment_keeps_nothing() {
    let ops: Vec<String> = OPS
        .iter()
        .map(|op| format!("{op}\nfor i in 0..30000 {{ }}"))
        .collect();
    let refs: Vec<&str> = ops.iter().map(String::as_str).collect();
    let src = script(&refs, None);
    let unlimited = ScriptLimits {
        fuel: 1_000_000_000,
        ..limits()
    };
    let mut e = project();
    let start = std::time::Instant::now();
    assert!(run_with(&mut e, &src, Capabilities::ALL, unlimited)
        .result
        .is_ok());
    let whole = start.elapsed().as_micros().max(1) as u64;
    let mut rng = Lcg(7);
    let (mut cancelled, mut finished) = (0, 0);
    for _ in 0..40 {
        let mut e = project();
        let before = but_ids(&e);
        let flag = AtomicBool::new(false);
        let wait = Duration::from_micros(rng.next(whole * 5 / 4));
        let out = std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(wait);
                flag.store(true, Ordering::Relaxed);
            });
            run_script(&mut e, &src, Capabilities::ALL, unlimited, &flag)
        });
        match out.result {
            Ok(_) => {
                finished += 1;
                assert_eq!(e.history().undo_depth(), before.undo + 1);
            }
            Err(f) => {
                assert_eq!(f.kind(), Some(ErrorKind::Cancelled), "{f}");
                assert_eq!(but_ids(&e), before);
                cancelled += 1;
            }
        }
    }
    assert!(
        cancelled > 10,
        "most cancels land during the run: {cancelled} of 40, {finished} finished"
    );
}

#[test]
fn a_cancel_stops_bulk_host_work_quickly() {
    let mut e = pikslar_core::Editor::new(
        pikslar_core::Document::new(1024, 1024, pikslar_core::ColorMode::Rgba).expect("a canvas"),
    );
    let before = but_ids(&e);
    let src = "let l = layers()[0]; let f = frames()[0]; let c = 0xff000000;\nwhile true { fill_rect(l, f, 0, 0, 1024, 1024, c); c = c + 1; }";
    let flag = AtomicBool::new(false);
    let unlimited = ScriptLimits {
        fuel: 2_000_000_000,
        ..ScriptLimits::default()
    };
    let (out, late) = std::thread::scope(|s| {
        let setter = s.spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            flag.store(true, Ordering::Relaxed);
            std::time::Instant::now()
        });
        let out = run_script(&mut e, src, Capabilities::ALL, unlimited, &flag);
        let ended = std::time::Instant::now();
        let set = setter.join().expect("the setter");
        (out, ended.saturating_duration_since(set))
    });
    assert_eq!(
        out.result.map_err(|f| f.kind()),
        Err(Some(ErrorKind::Cancelled))
    );
    assert!(
        late < Duration::from_millis(500),
        "stopped {late:?} after the cancel"
    );
    assert_eq!(but_ids(&e), before);
}

/// Fills `frames` new frames of a 256 by 256 canvas with pixels that pack into nothing
/// smaller: about 256 KiB of tiles a frame.
fn grow(frames: usize) -> String {
    format!(
        "let l = layers()[0]; let b = [];\n\
         for i in 0..4096 {{ push(b, 0xff000000 + (i * 4099) % 0x1000000); }}\n\
         for n in 0..{frames} {{ let f = add_frame(0);\n\
           for y in 0..4 {{ for x in 0..4 {{ write_rect(l, f, x * 64, y * 64, 64, b); }} }} }}"
    )
}

#[test]
fn a_run_may_grow_the_document_only_so_far() {
    let mut e = Editor::new(Document::new(256, 256, ColorMode::Rgba).expect("a canvas"));
    let limits = ScriptLimits {
        fuel: 200_000_000,
        growth: 16 << 20,
        ..ScriptLimits::default()
    };
    assert_eq!(limits.growth(), 16 << 20);
    let before = but_ids(&e);
    let out = run_with(&mut e, &grow(100), Capabilities::ALL, limits);
    let failure = out.result.expect_err("more than 16 MiB of new pixels");
    assert!(
        failure.to_string().contains("grew the document"),
        "{failure}"
    );
    assert_eq!(but_ids(&e), before, "taken back");
    // Within the budget the same work is kept.
    let out = run_with(&mut e, &grow(8), Capabilities::ALL, limits);
    assert!(out.result.is_ok(), "{:?}", out.result);
}
