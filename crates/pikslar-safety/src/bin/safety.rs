//! Long safety runs. Built with `--profile safety`, which keeps overflow checks and debug
//! assertions on, because a wrapped size is exactly what damaged files cause.

use pikslar_core::{ColorMode, Editor};
use pikslar_format::piks;
use pikslar_safety::fuzz::{self, Seeds, Tally, Target};
use pikslar_safety::picture::frame_hashes;
use pikslar_safety::undo_model::{self, Counts, Plan};
use std::ops::Range;
use std::process::ExitCode;

const MB: f64 = 1_048_576.0;

struct Options {
    iterations: u64,
    seed: u64,
    only: Option<u64>,
    target: Option<String>,
    /// Threads to run on; all the machine has unless given, as when other work shares it.
    workers: Option<u64>,
}

fn parse_options(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options {
        iterations: 100_000,
        seed: 1,
        only: None,
        target: None,
        workers: None,
    };
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let number = || value.parse::<u64>().map_err(|e| format!("{flag}: {e}"));
        match flag.as_str() {
            "--iterations" => options.iterations = number()?,
            "--seed" => options.seed = number()?,
            "--only" => options.only = Some(number()?),
            "--target" => options.target = Some(value),
            "--workers" => options.workers = Some(number()?.max(1)),
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    Ok(options)
}

fn workers(options: &Options) -> u64 {
    let all = std::thread::available_parallelism().map_or(1, |n| n.get()) as u64;
    options.workers.unwrap_or(all)
}

fn in_parallel(
    workers: u64,
    target: Target,
    seeds: &Seeds,
    seed: u64,
    cases: Range<u64>,
) -> Result<Tally, String> {
    let share = ((cases.end - cases.start) / workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let start = cases.start + worker * share;
                let end = if worker + 1 == workers {
                    cases.end
                } else {
                    start + share
                };
                let range = start.min(cases.end)..end.min(cases.end);
                scope.spawn(move || fuzz::run(target, seeds, seed, range))
            })
            .collect();
        let mut total = Tally::default();
        for handle in handles {
            let tally = handle.join().map_err(|_| "a worker crashed".to_owned())??;
            total.merge(&tally);
        }
        Ok(total)
    })
}

fn mutate_all(options: &Options) -> Result<(), String> {
    let seeds = Seeds::load()?;
    let ase = &seeds.tiled[1];
    let real = pikslar_safety::fuzz_tiles::fixture_count(ase);
    println!(
        "asetiles seeds {} ({real} from PIKSLAR_ASE_FIXTURES)",
        ase.len()
    );
    for target in Target::ALL {
        if options
            .target
            .as_deref()
            .is_some_and(|t| t != target.name())
        {
            continue;
        }
        let cases = options
            .only
            .map_or(0..options.iterations, |only| only..only + 1);
        let tally = in_parallel(workers(options), target, &seeds, options.seed, cases)?;
        println!(
            "{:9} cases {:8} accepted {:8} used {:8} restored {:8} peak {:7.2} MB largest {:7.2} MB slowest {:7.2} ms",
            target.name(),
            tally.cases,
            tally.accepted,
            tally.exercised,
            tally.restored,
            tally.peak as f64 / MB,
            tally.largest as f64 / MB,
            tally.slowest.as_secs_f64() * 1000.0,
        );
    }
    if options.only.is_none() && options.target.is_none() {
        for (name, bomb) in fuzz::BOMB_NAMES.iter().zip(fuzz::decompression_bombs()?) {
            println!(
                "worst-case {name} image: {} byte file, peak {:.1} MB, {:.0} ms",
                bomb.file_bytes,
                bomb.usage.peak as f64 / MB,
                bomb.took.as_secs_f64() * 1000.0
            );
        }
        for claim in pikslar_safety::fuzz_limits::claims()? {
            println!(
                "claimed {}: {} byte file, peak {:.1} MB, {:.0} ms",
                claim.name,
                claim.file_bytes,
                claim.usage.peak as f64 / MB,
                claim.took.as_secs_f64() * 1000.0
            );
        }
    }
    Ok(())
}

const UNDO_STEPS: usize = 150;

/// One kind of random run: the mixed runs or the terrain runs.
type RunFn = fn(Plan, u64) -> Result<Counts, String>;

fn undo_runs(plan: Plan, options: &Options, one: RunFn) -> Result<Counts, String> {
    let workers = workers(options);
    let first = options.only.unwrap_or(options.seed);
    let runs = options.only.map_or(options.iterations, |_| 1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                scope.spawn(move || {
                    let mut total = Counts::default();
                    for run in (worker..runs).step_by(workers as usize) {
                        total.merge(&one(plan, first + run)?);
                    }
                    Ok::<Counts, String>(total)
                })
            })
            .collect();
        let mut total = Counts::default();
        for handle in handles {
            total.merge(&handle.join().map_err(|_| "a worker crashed".to_owned())??);
        }
        Ok(total)
    })
}

/// Random edits, undos and redos, each state compared with a remembered one.
fn undo_all(options: &Options) -> Result<(), String> {
    let budgets = [("normal", None), ("tiny", Some(16 * 1024))];
    for mode in [ColorMode::Palette, ColorMode::Rgba] {
        for (name, budget) in budgets {
            let steps = UNDO_STEPS;
            let plan = Plan {
                mode,
                steps,
                budget,
            };
            let counts = undo_runs(plan, options, undo_model::run)?;
            println!(
                "{mode:?} {name:6} sequences {} steps {} undos {} redos {} floating {} dropped {} refused {} slices {} slice-refused {} tweens {} links {} brushes {} brush-turns {} brush-strokes {} palette {} remaps {} loads {} saves {} map-strokes {} tiles {} tile-pixels {} terrains {} terrain-strokes {}",
                options.iterations,
                counts.steps,
                counts.undos,
                counts.redos,
                counts.floating,
                counts.dropped,
                counts.refused,
                counts.slices,
                counts.slices_refused,
                counts.tweens,
                counts.links,
                counts.brushes,
                counts.brush_turns,
                counts.brush_strokes,
                counts.palette,
                counts.remaps,
                counts.loads,
                counts.saves,
                counts.map_strokes,
                counts.tiles,
                counts.tile_pixels,
                counts.terrains,
                counts.terrain_strokes
            );
        }
    }
    terrain_all(options)
}

/// The terrain runs (M11): mostly terrain work, each stroke held to the model's own rules.
fn terrain_all(options: &Options) -> Result<(), String> {
    let budgets = [("normal", None), ("tiny", Some(16 * 1024))];
    for mode in [ColorMode::Palette, ColorMode::Rgba] {
        for (name, budget) in budgets {
            let steps = UNDO_STEPS;
            let plan = Plan {
                mode,
                steps,
                budget,
            };
            let c = undo_runs(plan, options, undo_model::autotile::run)?;
            println!(
                "terrain {mode:?} {name:6} sequences {} steps {} undos {} redos {} saves {} terrain-sets {} terrain-strokes {} cells-checked {} previews {} pins {} removed-tiles {} reloaded-strokes {}",
                options.iterations,
                c.steps,
                c.undos,
                c.redos,
                c.saves,
                c.terrains,
                c.terrain_strokes,
                c.terrain_cells,
                c.previews,
                c.pins,
                c.removed_tiles,
                c.reloaded_strokes
            );
        }
    }
    Ok(())
}

/// Prints what a project looks like, one number per frame, with floating pixels in place.
fn hash_file(path: Option<String>) -> Result<(), String> {
    let path = path.ok_or("hash needs a file")?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let found = piks::load_recovery(&bytes).map_err(|e| format!("{path}: {e}"))?;
    let mut editor = Editor::new(found.doc);
    if let Some(snapshot) = found.floating {
        editor
            .restore_floating(snapshot)
            .map_err(|e| e.to_string())?;
    }
    let hashes: Vec<String> = frame_hashes(&mut editor)
        .iter()
        .map(|h| format!("{h:08x}"))
        .collect();
    println!("{}", hashes.join(","));
    Ok(())
}

/// Prints the state of a project with tilemaps that `spikes/e2e-kill-tiles.ps1` compares with
/// its model, or what rule it breaks. A file that will not open is an error.
fn tiles_file(path: Option<String>) -> Result<(), String> {
    let path = path.ok_or("tiles needs a file")?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let doc = piks::load(&bytes).map_err(|e| format!("{path}: {e}"))?;
    println!("{}", pikslar_safety::fuzz_tiles::tile_state(&doc));
    Ok(())
}

fn main() -> ExitCode {
    fuzz::report_panics_with_case();
    let mut args = std::env::args().skip(1);
    let result = match args.next().as_deref() {
        Some("mutate") => parse_options(args).and_then(|o| mutate_all(&o)),
        Some("hash") => hash_file(args.next()),
        Some("tiles") => tiles_file(args.next()),
        Some("undo") => parse_options(args).and_then(|o| undo_all(&o)),
        Some("terrain") => parse_options(args).and_then(|o| terrain_all(&o)),
        _ => Err(
            "usage: safety mutate|undo|terrain|hash FILE|tiles FILE [--iterations N] [--seed S] [--only I] [--target T] [--workers W]"
                .to_owned(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("FAILED: {message}");
            ExitCode::FAILURE
        }
    }
}
