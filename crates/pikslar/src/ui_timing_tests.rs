//! Headless timings of the whole interface draw, for package R1's before-and-after measurements
//! (docs/ui-refinement/issue-list.md). Nothing opens a window: the interface is drawn into a
//! plain pixel buffer through the same entry point the window uses.
//!
//! Ignored by default, because timings mean nothing in a debug build or on a busy machine. Run:
//! `cargo test --release -p pikslar --bin pikslar ui_timing -- --ignored --nocapture --test-threads 1`
//! The numbers are printed, not judged: no baseline or limit is read or written.

use crate::app::App;
use crate::workspace::model::Preset;
use std::time::Instant;

/// Draws of each case; the median is reported, with the slowest.
const DRAWS: usize = 40;

fn app_with(frames: usize, layers: usize, (w, h, scale): (i32, i32, i32), preset: Preset) -> App {
    let mut a = App::launch(None, Some(64), None);
    a.resized(w, h, scale);
    a.choose_preset(preset);
    for _ in 1..layers {
        a.layer_action(0);
    }
    for _ in 1..frames {
        a.new_frame(false);
    }
    a.set_frame(frames / 2);
    a
}

/// Median and slowest time of a whole interface draw, in microseconds.
fn time_draws(a: &mut App, w: i32, h: i32) -> (f64, f64) {
    let mut px = vec![0u32; (w * h) as usize];
    let mut times = Vec::with_capacity(DRAWS);
    for i in 0..DRAWS + 3 {
        a.ui_dirty = true;
        let mut surface = pikslar_render::Surface { px: &mut px, w, h };
        let started = Instant::now();
        a.draw_interface(
            &mut surface,
            pikslar_platform::Window::detached(),
            &mut Vec::new(),
        );
        // The first draws settle the layout and fill caches; they are not counted.
        if i >= 3 {
            times.push(started.elapsed().as_secs_f64() * 1e6);
        }
    }
    times.sort_by(|a, b| a.total_cmp(b));
    (times[times.len() / 2], times[times.len() - 1])
}

fn report(label: &str, a: &mut App, (w, h, _): (i32, i32, i32)) {
    let (median, worst) = time_draws(a, w, h);
    eprintln!("ui_timing | {label:<44} | median {median:>9.1} us | worst {worst:>9.1} us");
}

#[test]
#[ignore = "timing; run in release with --ignored"]
fn ui_timing() {
    const ONE: (i32, i32, i32) = (1366, 705, 1);
    const TWO: (i32, i32, i32) = (1920, 1003, 2);
    for (name, size) in [("1x", ONE), ("2x", TWO)] {
        for (preset, pname) in [
            (Preset::Draw, "Draw"),
            (Preset::Animate, "Animate"),
            (Preset::Tiles, "Tiles"),
            (Preset::Palette, "Palette"),
        ] {
            let mut a = app_with(6, 3, size, preset);
            report(&format!("{pname} {name}, 6 frames, 3 layers"), &mut a, size);
        }
        let mut a = app_with(6, 3, size, Preset::Draw);
        a.set_large_text(true);
        report(&format!("Draw {name}, Large text"), &mut a, size);
        a.set_large_text(false);
    }
    // The timeline at scale: Animate with every layer's cels shown.
    for (name, size) in [("1x", ONE), ("2x", TWO)] {
        for frames in [50, 100, 200, 1000] {
            for layers in [3, 10, 30] {
                let mut a = app_with(frames, layers, size, Preset::Animate);
                if !a.matrix.open {
                    a.toggle_matrix();
                }
                report(
                    &format!("Animate {name}, cels, {frames} frames x {layers} layers"),
                    &mut a,
                    size,
                );
            }
        }
    }
}
