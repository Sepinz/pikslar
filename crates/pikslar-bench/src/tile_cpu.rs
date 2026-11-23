//! `pikslar-bench tile-cpu [rounds] [blend|window|tiles]`: processor time against elapsed time
//! for the window's frame steps (`tile_steps.rs`: damage, composite what shows, blit), on one
//! thread and on several, with bands fixed to threads or shared among them. `limits.ps1`
//! divides the program's processor time, summed over every thread, by the steps; the wait for a
//! step is the elapsed time. Threads lower the second and raise the first. Steps come back to
//! back, and paced 20 ms apart as `limits.ps1` posts the keys, so that threads start cold as
//! they do in the window. `blend` composites `creative-measure`'s 1024 by 1024 blend frame
//! whole. The configurations alternate in every round; each line gives the lowest and the
//! median over the rounds. Not gate lines.

use crate::cpu::{cycles_per_ms, Mark, Spent};
use crate::creative_blend;
use crate::stress::stress_document;
use crate::stress_map;
use crate::tile_steps::{self, view, WindowStep, Zoom, WINDOW_CANVAS};
use pikslar_core::{Blending, Damage, Document, Editor, TILE};
use pikslar_render::{shown_region, Compositor, MAX_THREADS, PARALLEL_WORK};
use std::hint::black_box;
use std::time::{Duration, Instant};

/// The gap between two keys in `limits.ps1`.
const PACE: Duration = Duration::from_millis(20);
const BACK_TO_BACK_STEPS: usize = 64;
const PACED_STEPS: usize = 40;
const BLEND_UPDATES: usize = 12;
const ROUNDS: usize = 5;
const TILE_RUNS: [usize; 7] = [64, 128, 256, 512, 1024, 2048, 3400];

/// Threads, the least work (tiles times layers) that uses more than one, and bands a thread;
/// `None` is the program's rule.
type Split = Option<(usize, usize, usize)>;

/// One thread; threads at the program's threshold with one band each (bands fixed to threads,
/// as before bands were shared) or several shared bands each; and the program's rule.
const CONFIGS: [(&str, Split); 7] = [
    ("one", Some((1, usize::MAX, 1))),
    ("4 x 1", Some((4, PARALLEL_WORK, 1))),
    ("4 x 4", Some((4, PARALLEL_WORK, 4))),
    ("6 x 1", Some((6, PARALLEL_WORK, 1))),
    ("6 x 4", Some((6, PARALLEL_WORK, 4))),
    ("8 x 4", Some((8, PARALLEL_WORK, 4))),
    ("program", None),
];

fn apply(comp: &mut Compositor, split: Split) {
    if let Some((threads, min_work, per_thread)) = split {
        comp.set_threads(threads, min_work);
        comp.set_bands_per_thread(per_thread);
    }
}

/// `steps` window steps of `doc` at `zoom`, `pace` apart, per step. The elapsed time counts
/// the steps alone; processor time is read over the run, sleeping costs none.
fn window_run(
    doc: &Document,
    (zoom, split): (Zoom, Split),
    (steps, pace): (usize, Option<Duration>),
    per_ms: f64,
) -> Spent {
    let mut window = WindowStep::new(doc, zoom);
    if let Some((threads, min_work, per_thread)) = split {
        window.set_threads((threads, min_work));
        window.set_bands_per_thread(per_thread);
    }
    (0..doc.frames.len().min(8)).for_each(|_| window.step());
    let mut wall = 0.0;
    let mark = Mark::now();
    for _ in 0..steps {
        if let Some(pace) = pace {
            std::thread::sleep(pace);
        }
        let start = Instant::now();
        window.step();
        wall += start.elapsed().as_secs_f64() * 1e3;
    }
    let spent = mark.spent(per_ms);
    Spent { wall, ..spent }.per(steps)
}

/// The lowest and the median of each part.
fn low_and_median(runs: &mut [Spent]) -> (Spent, Spent) {
    let pick = |runs: &mut [Spent], f: fn(&Spent) -> f64, at: usize| {
        runs.sort_by(|a, b| f(a).total_cmp(&f(b)));
        f(&runs[at])
    };
    let mut both = [Spent::default(); 2];
    for (out, at) in both.iter_mut().zip([0, runs.len() / 2]) {
        *out = Spent {
            wall: pick(runs, |s| s.wall, at),
            cpu: pick(runs, |s| s.cpu, at),
            main: pick(runs, |s| s.main, at),
            charged: pick(runs, |s| s.charged, at),
        };
    }
    (both[0], both[1])
}

fn print_line(case: &str, config: &str, runs: &mut [Spent]) {
    let (low, mid) = low_and_median(runs);
    println!(
        "  {case:<22} {config:<10} wall {:7.3} / {:7.3}  cpu {:7.3} / {:7.3}  main {:7.3}  charged {:7.3}",
        low.wall, mid.wall, low.cpu, mid.cpu, mid.main, mid.charged
    );
}

fn window_cases(rounds: usize, per_ms: f64) {
    let paint = stress_document();
    let maps = [
        ("stress", stress_map::stress_map_document()),
        ("realistic", tile_steps::realistic_map()),
        ("worst", tile_steps::worst_map()),
    ];
    let mut cases: Vec<(String, &Document, Zoom)> =
        vec![("paint stress zoom 4".into(), &paint, Zoom::Times(4))];
    for (name, doc) in &maps {
        cases.push((format!("{name} zoom 1"), doc, Zoom::One));
        cases.push((format!("{name} fit"), doc, Zoom::Fit));
    }
    let pacing = [
        ("back to back", BACK_TO_BACK_STEPS, None),
        ("paced 20 ms", PACED_STEPS, Some(PACE)),
    ];
    for (label, steps, pace) in pacing {
        println!("window frame step, {label}, ms per step: lowest / median of {rounds} rounds");
        for (case, doc, zoom) in &cases {
            let mut runs = vec![Vec::new(); CONFIGS.len()];
            for _ in 0..rounds {
                for (i, (_, threads)) in CONFIGS.iter().enumerate() {
                    runs[i].push(window_run(doc, (*zoom, *threads), (steps, pace), per_ms));
                }
            }
            for ((config, _), runs) in CONFIGS.iter().zip(runs.iter_mut()) {
                print_line(case, config, runs);
            }
        }
    }
}

/// `creative-measure`'s `blend_compose_1024_*` frame (4,096 tiles of four layers) composited
/// whole, back to back, per update.
fn blend_run(doc: &Document, split: Split, per_ms: f64) -> Spent {
    let editor = Editor::new(doc.clone());
    let mut comp = Compositor::new(doc.width, doc.height);
    apply(&mut comp, split);
    let mut changed = Vec::new();
    comp.update(&editor, Damage::All, &mut changed);
    let mut wall = 0.0;
    let mark = Mark::now();
    for _ in 0..BLEND_UPDATES {
        changed.clear();
        let start = Instant::now();
        comp.update(&editor, Damage::All, &mut changed);
        black_box(comp.pixels());
        wall += start.elapsed().as_secs_f64() * 1e3;
    }
    Spent {
        wall,
        ..mark.spent(per_ms)
    }
    .per(BLEND_UPDATES)
}

fn blend_cases(rounds: usize, per_ms: f64) {
    let top = creative_blend::MODES[creative_blend::MODES.len() - 1];
    let docs = [
        ("blend 1024 pikslar", Blending::Pikslar),
        ("blend 1024 aseprite", Blending::Aseprite),
    ];
    println!("whole 1024 frame, back to back, ms per update: lowest / median of {rounds} rounds");
    for (case, blending) in docs {
        let doc = creative_blend::document(blending, top);
        let mut runs = vec![Vec::new(); CONFIGS.len()];
        for _ in 0..rounds {
            for (i, (_, split)) in CONFIGS.iter().enumerate() {
                runs[i].push(blend_run(&doc, *split, per_ms));
            }
        }
        for ((config, _), runs) in CONFIGS.iter().zip(runs.iter_mut()) {
            print_line(case, config, runs);
        }
    }
}

/// The first `count` canvas tiles, row by row, that a window at `zoom` shows of `doc`.
fn shown_tiles(doc: &Document, zoom: Zoom, count: usize) -> Vec<u32> {
    let size = (doc.width, doc.height);
    let r = shown_region(view(zoom, doc.width), WINDOW_CANVAS, size);
    let side = TILE as i32;
    let wide = doc.width.div_ceil(TILE);
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

/// `tiles` of `doc`'s first frame composited, paced as the window's keys, per update.
fn tiles_run(doc: &Document, tiles: &[u32], threads: (usize, usize), per_ms: f64) -> Spent {
    let editor = Editor::new(doc.clone());
    let mut comp = Compositor::new(doc.width, doc.height);
    comp.set_threads(threads.0, threads.1);
    let mut changed = Vec::new();
    comp.update(&editor, Damage::All, &mut changed);
    let mut wall = 0.0;
    let mark = Mark::now();
    for _ in 0..PACED_STEPS {
        std::thread::sleep(PACE);
        changed.clear();
        let start = Instant::now();
        comp.update(&editor, Damage::Tiles(tiles.to_vec()), &mut changed);
        black_box(comp.pixels());
        wall += start.elapsed().as_secs_f64() * 1e3;
    }
    Spent {
        wall,
        ..mark.spent(per_ms)
    }
    .per(PACED_STEPS)
}

/// Runs of shown tiles composited on one thread and on four, paced: where threads cut the wait
/// and what they cost in processor time.
fn tile_runs(rounds: usize, per_ms: f64) {
    let stress = stress_map::stress_map_document();
    let mut paint_alone = stress.clone();
    for (index, layer) in paint_alone.layers_mut().iter_mut().enumerate() {
        layer.visible = index == 0;
    }
    let paint = stress_document();
    let docs = [
        ("stress map, 4 layers", &stress, Zoom::One),
        ("its paint layer alone", &paint_alone, Zoom::One),
        ("paint stress, 12 layers", &paint, Zoom::Times(4)),
    ];
    println!("tiles composited, paced 20 ms, ms: one thread wall cpu, {MAX_THREADS} threads wall cpu (medians)");
    for (name, doc, zoom) in docs {
        println!("  {name}");
        for count in TILE_RUNS {
            let tiles = shown_tiles(doc, zoom, count);
            if tiles.len() < count {
                continue;
            }
            let mut one = Vec::new();
            let mut many = Vec::new();
            for _ in 0..rounds {
                one.push(tiles_run(doc, &tiles, (1, usize::MAX), per_ms));
                many.push(tiles_run(doc, &tiles, (MAX_THREADS, 0), per_ms));
            }
            let (_, one) = low_and_median(&mut one);
            let (_, many) = low_and_median(&mut many);
            println!(
                "    {count:5} {:7.3} {:7.3}   {:7.3} {:7.3}   saves {:6.3} wall, costs {:6.3} cpu",
                one.wall,
                one.cpu,
                many.wall,
                many.cpu,
                one.wall - many.wall,
                many.cpu - one.cpu
            );
        }
    }
}

pub fn run() {
    let rounds = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(ROUNDS)
        .max(1);
    let per_ms = cycles_per_ms();
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!("cycles per ms {per_ms:.0}, available parallelism {threads}, at most {MAX_THREADS}");
    let only = std::env::args().nth(3).unwrap_or_default();
    if only.is_empty() || only == "blend" {
        blend_cases(rounds, per_ms);
    }
    if only.is_empty() || only == "window" {
        window_cases(rounds, per_ms);
    }
    if only.is_empty() || only == "tiles" {
        tile_runs(rounds, per_ms);
    }
}
