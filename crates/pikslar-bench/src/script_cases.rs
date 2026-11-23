//! The script cases: start on first use, ordinary scripts, heavy pixel work on the large
//! document, and hostile scripts with the default limits.

use crate::process_memory::private;
use crate::script_bench::{
    measure, median_ms, peak_ceiling_kb, report, timed_median, HOSTILE_REPS,
};
use crate::script_large as large;
use crate::stress::{large_document, stress_document};
use pikslar_bind::ScriptLimits;
use pikslar_core::{ColorMode, Document, Editor};
use std::time::Instant;

type Case = fn(&str);

pub const CASES: &[(&str, Case)] = &[
    ("first_use", first_use),
    ("ordinary", ordinary),
    ("pixels_256", pixels_256),
    ("pixels_1024", pixels_1024),
    ("bulk", bulk),
    ("hostile_loop", hostile),
    ("hostile_memory", hostile),
    ("hostile_small_then_large", hostile),
    ("hostile_recursion", hostile),
    ("hostile_calls", hostile_calls),
    ("hostile_frames", hostile_stress),
    ("hostile_layers", hostile_stress),
    ("hostile_lines", hostile_large),
    ("hostile_fills", hostile_large),
    ("hostile_scatter", hostile_large),
    ("hostile_rects", hostile_large),
    ("hostile_writes", hostile_large),
    ("hostile_reads", hostile_large),
    ("hostile_recolor", hostile_stress),
    ("hostile_frame_reads", hostile_wide),
    ("hostile_layer_reads", hostile_wide),
    ("hostile_swatch_edits", hostile_wide),
    ("hostile_growth", hostile_large),
    ("timeline_ordinary", large::timeline_ordinary),
    ("canvas_ordinary", large::canvas_ordinary),
    ("map_ordinary", large::map_ordinary),
    ("hostile_timeline", large::timeline_hostile),
    ("hostile_timeline_reads", large::timeline_reads_hostile),
    ("hostile_canvas", large::canvas_hostile),
    ("hostile_map", large::map_hostile),
];

pub fn case(name: &str) -> Option<Case> {
    CASES.iter().find(|(n, _)| *n == name).map(|&(_, c)| c)
}

fn small() -> Editor {
    Editor::new(Document::new(128, 128, ColorMode::Rgba).unwrap_or_else(|_| unreachable!()))
}

/// The first run in a process starts the interpreter's thread: nothing exists before it.
fn first_use(name: &str) {
    let mut e = small();
    let before = private();
    let t = Instant::now();
    let first = pikslar_bind::run_script(
        &mut e,
        "return 1;",
        pikslar_bind::Capabilities::ALL,
        ScriptLimits::DEFAULT,
        &std::sync::atomic::AtomicBool::new(false),
    );
    let first_ms = t.elapsed().as_secs_f64() * 1e3;
    let after_kb = private().saturating_sub(before) as f64 / 1024.0;
    report(name, "first_ms", first_ms, &first);
    report(name, "private_after_kb", after_kb, &first);
    let (ms, out) = median_ms(&mut e, "return 1;", 21);
    report(name, "next_ms", ms, &out);
}

const ORDINARY: &str = r#"
    let ls = layers();
    for i in 0..len(ls) { rename_layer(ls[i], "Part " + str(i + 1)); }
    let fs = frames();
    for i in 0..10 { let f = add_frame(len(frames())); set_duration(f, 80); }
    let all = frames();
    for t in 0..8 { add_tag("tag " + str(t), all[t * 10], all[t * 10 + 9]); }
"#;

/// Renaming layers, adding frames and tags on the stress document (12 layers, 400 frames).
fn ordinary(name: &str) {
    let mut e = Editor::new(stress_document());
    let (ms, out) = median_ms(&mut e, ORDINARY, 5);
    report(name, "stress_ms", ms, &out);
}

fn per_pixel(side: u32) -> String {
    format!(
        "let l = layers()[0]; let f = frames()[0];\n\
         for y in 0..{side} {{ for x in 0..{side} {{ set_pixel(l, f, x, y, 0xff000000 + x + y * 256); }} }}"
    )
}

fn pixels_256(name: &str) {
    let mut e = Editor::new(large_document());
    let (ms, out) = median_ms(&mut e, &per_pixel(256), 5);
    report(name, "large_ms", ms, &out);
}

fn pixels_1024(name: &str) {
    let mut e = Editor::new(large_document());
    // The first run is the cold one: its peak counts what a later run would find kept.
    let src = per_pixel(1024);
    let m = measure(&mut e, &src, ScriptLimits::DEFAULT);
    report(name, "peak_private_kb", m.peak_kb, &m.out);
    let (ms, out) = timed_median(&mut e, &src, HOSTILE_REPS, Vec::new());
    report(name, "large_ms", ms, &out);
}

/// Bulk work on the large document (2048 x 3040).
fn bulk(name: &str) {
    let mut e = Editor::new(large_document());
    let cases = [
        ("fill_rect_ms", "fill_rect(layers()[0], frames()[0], 0, 0, 2048, 3040, 0xff336699);"),
        ("lines_100_ms", "let l = layers()[0]; let f = frames()[0]; for i in 0..100 { draw_line(l, f, 0, i * 30, 2047, 3039 - i * 30, 0xff00ff00); }"),
        ("fill_whole_ms", "let l = layers()[0]; let f = frames()[0]; fill_rect(l, f, 0, 0, 2048, 3040, 0xff112233); fill(l, f, 5, 5, 0xff445566);"),
        ("read_rect_256_ms", "let b = read_rect(layers()[0], frames()[0], 0, 0, 256, 256); return len(b);"),
        ("map_rect_256_ms", "let l = layers()[0]; let f = frames()[0]; let b = read_rect(l, f, 0, 0, 256, 256); for i in 0..len(b) { b[i] = 0xff000000 + (b[i] % 0x1000000) / 2; } write_rect(l, f, 0, 0, 256, b);"),
    ];
    for (what, src) in cases {
        let (ms, out) = median_ms(&mut e, src, 5);
        report(name, what, ms, &out);
    }
}

fn hostile_source(name: &str) -> &'static str {
    match name {
        "hostile_loop" => "while true { }",
        "hostile_memory" => "let l = []; while true { push(l, 1); }",
        "hostile_small_then_large" => "let keep = []; for i in 0..50000 { push(keep, str(i) + \"abcdefgh\"); } keep = 0; let s = repeat(\"x\", 15000000); return len(s);",
        "hostile_recursion" => "fn down(n) { return down(n + 1); } down(0);",
        "hostile_frames" => "while true { let f = add_frame(0); remove_frame(f); }",
        "hostile_layers" => "let l = layers()[0]; while true { let d = duplicate_layer(l); remove_layer(d); }",
        "hostile_lines" => "let l = layers()[0]; let f = frames()[0]; let c = 0xff000000; while true { draw_line(l, f, 0, 0, 2047, 3039, c); c = c + 1; }",
        "hostile_fills" => "let l = layers()[0]; let f = frames()[0]; fill_rect(l, f, 0, 0, 2048, 3040, 0xff000000); let c = 0xff000001; while true { fill(l, f, 0, 0, c); c = c + 1; }",
        "hostile_scatter" => "let l = layers()[0]; let f = frames()[0]; let i = 0; while true { set_pixel(l, f, (i * 4099) % 2048, (i * 7919) % 3040, 0xff00ff00); i = i + 1; }",
        "hostile_recolor" => "let s = add_swatch(0xff00ff00); let l = layers()[0]; let fs = frames(); for i in 0..64 { fill_rect(l, fs[i], 0, 0, 128, 128, 0xff00ff00); } let c = 0xff000000; while true { recolor(s, c); c = c + 1; }",
        "hostile_rects" => "let l = layers()[0]; let f = frames()[0]; let c = 0xff000000; while true { fill_rect(l, f, 0, 0, 2048, 3040, c); c = c + 1; }",
        "hostile_writes" => "let l = layers()[0]; let f = frames()[0]; let b = read_rect(l, f, 0, 0, 512, 512); while true { write_rect(l, f, 0, 0, 512, b); b[0] = b[0] + 1; }",
        "hostile_frame_reads" => "let l = layers()[0]; let f = frames()[4095]; while true { get_pixel(l, f, 0, 0); }",
        "hostile_layer_reads" => "let l = layers()[255]; while true { layer_visible(l); }",
        "hostile_swatch_edits" => "let s = swatches(); let last = s[len(s) - 1]; let c = 0xff000000; while true { recolor(last, c); c = c + 1; }",
        "hostile_growth" => "let f = frames()[0]; let c = 0xff000000; while true { let l = add_layer(\"x\"); fill_rect(l, f, 0, 0, 2048, 3040, c); c = c + 1; }",
        _ => "let l = layers()[0]; let f = frames()[0]; while true { let b = read_rect(l, f, 0, 0, 512, 512); }",
    }
}

/// A hostile script on `e`: the first, cold run's peak and what it left, then the median
/// stop time of that run and [`HOSTILE_REPS`] - 1 more, with the ceiling its peak is held to.
pub fn hostile_on(name: &str, src: &str, mut e: Editor) {
    let ceiling = peak_ceiling_kb(e.doc());
    let m = measure(&mut e, src, ScriptLimits::DEFAULT);
    report(name, "peak_private_kb", m.peak_kb, &m.out);
    report(name, "private_after_kb", m.after_kb, &m.out);
    report(name, "peak_ceiling_kb", ceiling, &m.out);
    let (ms, out) = timed_median(&mut e, src, HOSTILE_REPS - 1, vec![m.ms]);
    report(name, "stop_ms", ms, &out);
}

fn hostile(name: &str) {
    hostile_on(name, hostile_source(name), small());
}

/// Calls to a function of 255 locals that returns at once: each call makes and drops them.
fn hostile_calls(name: &str) {
    let lets: String = (0..255).map(|i| format!(" let a{i} = 0;")).collect();
    let src = format!("fn wide() {{ return 0;{lets} }}\nwhile true {{ wide(); }}");
    hostile_on(name, &src, small());
}

fn hostile_stress(name: &str) {
    hostile_on(name, hostile_source(name), Editor::new(stress_document()));
}

fn hostile_large(name: &str) {
    hostile_on(name, hostile_source(name), Editor::new(large_document()));
}

/// The most a document holds of what reads look through: 4096 frames, 256 layers and
/// 65,536 colors, all of them swatches, on a small canvas.
fn widest() -> Editor {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).unwrap_or_else(|_| unreachable!());
    for at in 1..pikslar_core::MAX_FRAMES {
        doc.add_frame(at, false).unwrap_or_else(|_| unreachable!());
    }
    for at in 1..256 {
        doc.add_layer(at, ColorMode::Rgba)
            .unwrap_or_else(|_| unreachable!());
    }
    let colors = std::sync::Arc::make_mut(&mut doc.colors);
    let mut c = 0xff00_0000u32;
    while colors.add_swatch(c).is_ok() {
        c += 1;
    }
    Editor::new(doc)
}

fn hostile_wide(name: &str) {
    hostile_on(name, hostile_source(name), widest());
}
