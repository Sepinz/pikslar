//! `pikslar-bench tile-pool`: short-lived scoped threads against a reusable worker pool for the
//! window frame step. Bench only: the program composites on scoped threads
//! (`pikslar-render/src/compose_threads.rs`), and this prototype pool is never shipped.
//!
//! Both ways run the same jobs. Each thread owns an editor and a compositor of the map. Every
//! step, the damaged tiles the window shows are found as the program finds them. While the work
//! (tiles times layers drawn) is under the program's threshold, the calling thread composites
//! them alone. From there on they are split in four. The calling thread takes one part, and the
//! other three go either to threads started for that step inside `std::thread::scope`, or to
//! three pool threads that wait on a channel for the whole run. Blitting is left out: it is
//! the same for both.
//!
//! It also prints what starting or waking three threads costs with nothing to do, back to back
//! and after 16 ms idle, the gap between two steps at 60 frames a second.

use crate::stress_map::{self, SIDE};
use crate::tile_steps::{self, view, Zoom, WINDOW_CANVAS};
use pikslar_core::{Damage, Document, Editor, TILE};
use pikslar_render::{shown_region, Compositor, Rect, MAX_THREADS, PARALLEL_WORK};
use std::hint::black_box;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const RUNS: usize = 5;
const WAKES: usize = 60;
const IDLE: Duration = Duration::from_millis(16);

/// One thread's share of the work: its own editor and compositor of the document.
struct Part {
    editor: Editor,
    comp: Compositor,
    changed: Vec<Rect>,
}

impl Part {
    fn new(doc: &Document) -> Self {
        let editor = Editor::new(doc.clone());
        let mut comp = Compositor::new(doc.width, doc.height);
        comp.set_threads(1, usize::MAX);
        let mut changed = Vec::new();
        comp.update(&editor, Damage::All, &mut changed);
        Part {
            editor,
            comp,
            changed,
        }
    }

    fn compose(&mut self, frame: usize, tiles: &[u32]) {
        self.editor.frame = frame;
        self.changed.clear();
        let damage = Damage::Tiles(tiles.to_vec());
        self.comp.update(&self.editor, damage, &mut self.changed);
        black_box(self.comp.pixels());
    }
}

type Job = (usize, Arc<Vec<u32>>);

/// Threads that live as long as the pool, each owning a part and waiting for jobs.
struct Pool {
    senders: Vec<Sender<Job>>,
    done: Receiver<()>,
    handles: Vec<JoinHandle<()>>,
}

impl Pool {
    fn new(parts: Vec<Part>) -> Self {
        let (done_tx, done) = channel();
        let mut senders = Vec::new();
        let mut handles = Vec::new();
        for mut part in parts {
            let (tx, rx) = channel::<Job>();
            let done_tx = done_tx.clone();
            handles.push(std::thread::spawn(move || {
                for (frame, tiles) in rx {
                    if !tiles.is_empty() {
                        part.compose(frame, &tiles);
                    }
                    if done_tx.send(()).is_err() {
                        break;
                    }
                }
            }));
            senders.push(tx);
        }
        Pool {
            senders,
            done,
            handles,
        }
    }

    /// Hands one job to each of the first threads and waits until they are all done.
    fn run(&self, jobs: impl Iterator<Item = Job>) {
        let sent = self
            .senders
            .iter()
            .zip(jobs)
            .filter(|(tx, job)| tx.send(job.clone()).is_ok())
            .count();
        for _ in 0..sent {
            black_box(self.done.recv().is_ok());
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.senders.clear();
        for handle in self.handles.drain(..) {
            black_box(handle.join().is_ok());
        }
    }
}

/// Steps the main editor through the frames, as the window does, and says what to composite.
struct Stepper {
    editor: Editor,
    shown: Vec<bool>,
    layers: usize,
}

impl Stepper {
    fn new(doc: &Document, zoom: Zoom) -> Self {
        let r = shown_region(view(zoom, SIDE), WINDOW_CANVAS, (SIDE, SIDE));
        let wide = SIDE / TILE;
        let side = TILE as i32;
        let mut shown = vec![false; wide * wide];
        for ty in r.y / side..=(r.bottom() - 1) / side {
            for tx in r.x / side..=(r.right() - 1) / side {
                shown[ty as usize * wide + tx as usize] = true;
            }
        }
        Stepper {
            editor: Editor::new(doc.clone()),
            shown,
            layers: doc.layers().iter().filter(|l| l.visible).count().max(1),
        }
    }

    /// The next frame and the shown tiles its step damaged.
    fn next(&mut self) -> (usize, Vec<u32>) {
        let from = self.editor.frame;
        self.editor.frame = (from + 1) % self.editor.doc().frames.len();
        self.editor.damage_frame_change(from);
        let tiles: Vec<u32> = match self.editor.take_damage() {
            Damage::None => Vec::new(),
            Damage::Tiles(tiles) => tiles,
            _ => (0..self.shown.len() as u32).collect(),
        };
        let mut tiles: Vec<u32> = tiles
            .into_iter()
            .filter(|&t| self.shown[t as usize])
            .collect();
        tiles.sort_unstable();
        (self.editor.frame, tiles)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Way {
    One,
    Scoped,
    Pooled,
}

/// `tiles` cut into `MAX_THREADS` runs of about as many tiles.
fn split(tiles: &[u32]) -> Vec<Arc<Vec<u32>>> {
    let size = tiles.len().div_ceil(MAX_THREADS).max(1);
    tiles.chunks(size).map(|c| Arc::new(c.to_vec())).collect()
}

fn scoped_step(own: &mut Part, helpers: &mut [Part], frame: usize, parts: &[Arc<Vec<u32>>]) {
    std::thread::scope(|scope| {
        for (part, tiles) in helpers.iter_mut().zip(parts.iter().skip(1)) {
            scope.spawn(move || part.compose(frame, tiles));
        }
        if let Some(tiles) = parts.first() {
            own.compose(frame, tiles);
        }
    });
}

fn pooled_step(own: &mut Part, pool: &Pool, frame: usize, parts: &[Arc<Vec<u32>>]) {
    let jobs = parts.iter().skip(1).map(|tiles| (frame, Arc::clone(tiles)));
    // The pool's threads start on their parts before the calling thread starts its own.
    let sent = pool
        .senders
        .iter()
        .zip(jobs)
        .filter(|(tx, job)| tx.send(job.clone()).is_ok())
        .count();
    if let Some(tiles) = parts.first() {
        own.compose(frame, tiles);
    }
    for _ in 0..sent {
        black_box(pool.done.recv().is_ok());
    }
}

/// Microseconds per frame step, best of runs, and the share of steps that used threads.
fn step_micros(doc: &Document, zoom: Zoom, way: Way) -> (f64, f64) {
    let mut stepper = Stepper::new(doc, zoom);
    let mut own = Part::new(doc);
    let mut helpers: Vec<Part> = (1..MAX_THREADS).map(|_| Part::new(doc)).collect();
    let pool = (way == Way::Pooled).then(|| Pool::new(std::mem::take(&mut helpers)));
    let frames = doc.frames.len();
    let (mut steps, mut threaded) = (0usize, 0usize);
    let mut best = f64::MAX;
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..frames {
            let (frame, tiles) = stepper.next();
            steps += 1;
            if way == Way::One || tiles.len() * stepper.layers < PARALLEL_WORK {
                own.compose(frame, &tiles);
                continue;
            }
            threaded += 1;
            let parts = split(&tiles);
            match &pool {
                Some(pool) => pooled_step(&mut own, pool, frame, &parts),
                None => scoped_step(&mut own, &mut helpers, frame, &parts),
            }
        }
        best = best.min(start.elapsed().as_secs_f64() * 1e6 / frames as f64);
    }
    (best, threaded as f64 / steps as f64)
}

fn median(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

/// Median microseconds to start three idle scoped threads, and to wake three idle pool
/// threads and hear back, each after `idle` with nothing running.
fn wake_micros(idle: Duration) -> (f64, f64) {
    let tiny = Document::new(16, 16, pikslar_core::ColorMode::Rgba).expect("a canvas");
    let pool = Pool::new((1..MAX_THREADS).map(|_| Part::new(&tiny)).collect());
    let empty = Arc::new(Vec::new());
    let time = |f: &mut dyn FnMut()| {
        let samples = (0..WAKES).map(|_| {
            std::thread::sleep(idle);
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1e6
        });
        median(samples.collect())
    };
    let scoped = time(&mut || {
        std::thread::scope(|scope| {
            for _ in 1..MAX_THREADS {
                scope.spawn(|| black_box(0));
            }
        })
    });
    let pooled = time(&mut || pool.run((1..MAX_THREADS).map(|_| (0, Arc::clone(&empty)))));
    (scoped, pooled)
}

fn steps() {
    let maps = [
        ("stress", stress_map::stress_map_document()),
        ("realistic", tile_steps::realistic_map()),
        ("worst", tile_steps::worst_map()),
    ];
    println!(
        "frame step without the blit, us: one thread, scoped, pool; share of steps on threads"
    );
    for (name, doc) in &maps {
        for (zoom, label) in [(Zoom::One, "zoom 1"), (Zoom::Fit, "fit")] {
            let (one, _) = step_micros(doc, zoom, Way::One);
            let (scoped, share) = step_micros(doc, zoom, Way::Scoped);
            let (pooled, _) = step_micros(doc, zoom, Way::Pooled);
            println!("  {name:9} {label:6} {one:10.1} {scoped:10.1} {pooled:10.1}   {share:4.2}");
        }
    }
}

pub fn run() {
    println!(
        "{} threads at most, from {PARALLEL_WORK} layer tiles",
        MAX_THREADS
    );
    for (idle, label) in [(Duration::ZERO, "back to back"), (IDLE, "after 16 ms idle")] {
        let (scoped, pooled) = wake_micros(idle);
        println!(
            "three idle threads, {label}: scoped start {scoped:.1} us, pool wake {pooled:.1} us"
        );
    }
    steps();
}
