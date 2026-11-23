//! M12 red team: a run stopped at many points, by its fuel running out at each of hundreds of
//! places and by a cancel from another thread at varied times, must leave the document, its
//! history, its revision and its saved bytes exactly as they were. And a run that may not
//! edit must be refused every edit.

mod common;

use common::{but_ids, layer_ids, project, run_with, state};
use pikslar_bind::{functions, run_script, Capabilities, Capability, Failure, ScriptLimits};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Every kind of edit, again and again, with pixels left waiting between them.
const BUSY: &str = "let l = layers()[0]; let f = frames()[0];
for round in 0..1000000 {
    let n = add_layer(\"r\" + str(round));
    for i in 0..40 { set_pixel(l, f, i % 32, (i + round) % 24, 4278190335 + i); }
    rename_layer(n, \"s\" + str(round));
    set_visible(n, round % 2 == 0);
    fill_rect(n, f, 0, 0, 8, 8, 4278190335);
    let g = add_frame(1);
    set_duration(g, 50 + round);
    draw_line(l, g, 0, 0, 31, 23, 4278255360);
    let s = add_swatch(4278190335 + round);
    recolor(s, 4278190080);
    remove_frame(g);
    if round % 3 == 0 { remove_layer(n); }
}";

fn stopped_changes_nothing(limits: ScriptLimits, cancel_after: Option<Duration>) -> bool {
    let mut e = project();
    let before = but_ids(&e);
    let flag = Arc::new(AtomicBool::new(false));
    let timer = cancel_after.map(|d| {
        let flag = flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(d);
            flag.store(true, Ordering::Relaxed);
        })
    });
    let out = run_script(&mut e, BUSY, Capabilities::ALL, limits, &flag);
    if let Some(t) = timer {
        t.join().expect("the timer");
    }
    let f = out.result.expect_err("a run this long must stop");
    assert!(!matches!(f, Failure::Thread(_)), "{f}");
    assert_eq!(but_ids(&e), before, "stopped by {f} but changed things");
    f.is_cancel()
}

#[test]
fn running_out_of_fuel_anywhere_changes_nothing() {
    for k in 0..300 {
        let limits = ScriptLimits {
            fuel: 10_000 + k * 997,
            ..ScriptLimits::default()
        };
        stopped_changes_nothing(limits, None);
    }
}

#[test]
fn a_cancel_at_any_time_changes_nothing() {
    let mut cancels = 0;
    for k in 0..60 {
        let limits = ScriptLimits {
            fuel: 1 << 40,
            ..ScriptLimits::default()
        };
        let after = Duration::from_micros(k * k * 50);
        cancels += usize::from(stopped_changes_nothing(limits, Some(after)));
    }
    println!("{cancels} runs ended by the cancel");
    assert!(cancels > 20, "the cancel ended only {cancels} runs");
}

/// Every edit function called by a run that may only read, or not even that.
#[test]
fn a_run_that_may_not_edit_is_refused_every_edit() {
    let e = project();
    let layer = layer_ids(&e)[0];
    let frame = common::frames(&e)[0];
    let edits = functions()
        .into_iter()
        .filter(|f| f.needs == Some(Capability::EditDocument));
    let mut checked = 0;
    for f in edits {
        let mut args = vec![layer.to_string(), frame.to_string()];
        args.extend(["1", "1", "2", "2", "4278190335"].map(String::from));
        args.truncate(f.arity);
        let src = format!("{}({});", f.name, args.join(", "));
        for caps in [Capabilities::NONE, Capabilities::READ] {
            let mut e = project();
            let before = state(&e);
            let out = run_with(&mut e, &src, caps, ScriptLimits::default());
            assert!(
                matches!(out.result, Err(Failure::Script(_))),
                "{src} under {caps:?}: {:?}",
                out.result
            );
            assert_eq!(state(&e), before, "{src} under {caps:?}");
            checked += 1;
        }
    }
    assert!(checked >= 30, "only {checked} edit calls");
}
