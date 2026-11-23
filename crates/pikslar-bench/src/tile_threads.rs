//! `pikslar-bench tile-threads`: the numbers behind compositing on threads. It prints the cost
//! of starting scoped threads, the time of compositing a run of canvas tiles on one thread and
//! on several, by the number of tiles, on the stress map and on its paint layer alone (cheap
//! tiles, where threads pay least), and the window frame steps of `tile_steps.rs` on one thread
//! and on the program's threads. Not gate lines.

use crate::stress_map::{self, SIDE};
use crate::tile_steps::{self, view, Zoom, WINDOW_CANVAS};
use crate::workloads::best_of;
use pikslar_core::{Damage, Document, Editor, TILE};
use pikslar_render::{shown_region, Compositor, MAX_THREADS, PARALLEL_WORK};
use std::hint::black_box;

const SPAWNS: usize = 2_000;
const SWEEP: [usize; 6] = [0, 512, 1024, 2048, 4096, 8192];
const RUNS: [usize; 11] = [8, 16, 32, 64, 96, 128, 192, 256, 512, 1024, 3400];

/// Microseconds for one `std::thread::scope` that starts `threads` threads doing nothing.
fn spawn_micros(threads: usize) -> f64 {
    best_of(|| {
        for _ in 0..SPAWNS {
            std::thread::scope(|scope| {
                for _ in 0..threads {
                    scope.spawn(|| black_box(0));
                }
            });
        }
    }) / SPAWNS as f64
}

/// The first `count` canvas tiles, row by row, of what a window at zoom 1 shows.
fn shown_tiles(count: usize) -> Vec<u32> {
    let r = shown_region(view(Zoom::One, SIDE), WINDOW_CANVAS, (SIDE, SIDE));
    let side = TILE as i32;
    let wide = SIDE / TILE;
    let rows = (r.y / side)..((r.bottom() - 1) / side + 1);
    let columns = (r.x / side)..((r.right() - 1) / side + 1);
    rows.flat_map(|ty| {
        columns
            .clone()
            .map(move |tx| (ty as usize * wide + tx as usize) as u32)
    })
    .take(count)
    .collect()
}

/// Microseconds to composite `tiles` of `doc`'s first frame on at most `threads` threads,
/// on more than one from `min_work` on.
fn run_micros(doc: &Document, tiles: &[u32], (threads, min_work): (usize, usize)) -> f64 {
    let editor = Editor::new(doc.clone());
    let mut comp = Compositor::new(doc.width, doc.height);
    comp.set_threads(threads, min_work);
    let mut changed = Vec::new();
    comp.update(&editor, Damage::All, &mut changed);
    best_of(|| {
        for _ in 0..8 {
            changed.clear();
            comp.update(&editor, Damage::Tiles(tiles.to_vec()), &mut changed);
            black_box(comp.pixels());
        }
    }) / 8.0
}

fn crossover(name: &str, doc: &Document) {
    println!("{name}: tiles, one thread us, {MAX_THREADS} threads us, ratio, the program's us");
    for count in RUNS {
        let tiles = shown_tiles(count);
        let one = run_micros(doc, &tiles, (1, 0));
        let many = run_micros(doc, &tiles, (MAX_THREADS, 0));
        let chosen = run_micros(doc, &tiles, (MAX_THREADS, PARALLEL_WORK));
        println!(
            "  {count:5} {one:10.1} {many:10.1} {:6.2} {chosen:10.1}",
            many / one
        );
    }
}

fn steps() {
    let maps = [
        ("stress", stress_map::stress_map_document()),
        ("realistic", tile_steps::realistic_map()),
        ("worst", tile_steps::worst_map()),
    ];
    println!("window frame step, ms: one thread / the program's threads");
    for (name, doc) in &maps {
        let ms = |zoom, threads| tile_steps::window_step_micros(doc, zoom, threads) / 1e3;
        println!(
            "  {name:9} zoom 1 {:7.2} / {:7.2}   fit {:7.2} / {:7.2}",
            ms(Zoom::One, Some((1, 0))),
            ms(Zoom::One, None),
            ms(Zoom::Fit, Some((1, 0))),
            ms(Zoom::Fit, None)
        );
    }
}

/// The fitted frame step of the realistic and stress maps on at most four threads, by the least
/// work that uses more than one: where a threshold starts to cost.
fn sweep() {
    let maps = [
        ("realistic", tile_steps::realistic_map()),
        ("stress", stress_map::stress_map_document()),
    ];
    println!("fitted frame step, ms, by the least work on threads: {SWEEP:?} and never");
    for (name, doc) in &maps {
        let ms =
            |min| tile_steps::window_step_micros(doc, Zoom::Fit, Some((MAX_THREADS, min))) / 1e3;
        let times: Vec<String> = SWEEP
            .iter()
            .chain([usize::MAX].iter())
            .map(|&min| format!("{:6.3}", ms(min)))
            .collect();
        println!("  {name:9} {}", times.join(" "));
    }
}

pub fn run() {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!(
        "available parallelism {threads}, at most {MAX_THREADS}, from {PARALLEL_WORK} layer tiles"
    );
    for spawned in 1..MAX_THREADS {
        println!(
            "scope starting {spawned} idle threads: {:.1} us",
            spawn_micros(spawned)
        );
    }
    let doc = stress_map::stress_map_document();
    crossover("stress map, four layers", &doc);
    let mut paint = doc.clone();
    for (index, layer) in paint.layers_mut().iter_mut().enumerate() {
        layer.visible = index == 0;
    }
    crossover("its paint layer alone", &paint);
    steps();
    sweep();
}
