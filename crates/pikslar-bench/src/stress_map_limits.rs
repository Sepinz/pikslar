//! `pikslar-bench stress-map-limits <file.piks> [hold-seconds]`: the stress-map limits measured
//! without the window, for when the real program cannot be driven. It reads and opens the file,
//! draws its first frame on a canvas the size of the app's, steps through the frames, and prints
//! the numbers as `limits.ps1` writes them, with the heap in use. A frame step is taken as the
//! program takes it in a 1400 by 900 window at zoom 1; the full composite of every canvas tile
//! that a step used to be is printed after it. The heap is read with one compositor and one
//! window buffer kept, as the program keeps them. With a hold it then waits, so
//! that a caller can read the process's private memory. These are not the program's numbers: the
//! window and its interface are left out.

use crate::live_bytes;
use crate::tile_steps::{WindowStep, Zoom};
use crate::workloads::Canvas;
use pikslar_core::{Damage, Editor};
use pikslar_format::piks;
use pikslar_render::{Compositor, PARALLEL_WORK};
use std::time::{Duration, Instant};

/// Frame steps timed: every frame of the stress map several times over.
const STEPS: usize = 64;

fn millis(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1e3
}

/// A frame step as the program took it before: every canvas tile composited on one thread.
fn full_composite_ms(editor: &mut Editor) -> f64 {
    let frames = editor.doc().frames.len();
    let (w, h) = (editor.doc().width, editor.doc().height);
    let mut comp = Compositor::new(w, h);
    comp.set_threads(1, PARALLEL_WORK);
    let mut changed = Vec::new();
    let start = Instant::now();
    for step in 0..STEPS {
        editor.frame = (step + 1) % frames;
        changed.clear();
        comp.update(editor, Damage::All, &mut changed);
    }
    millis(start) / STEPS as f64
}

pub fn run(path: &str, hold: u64) -> Result<(), String> {
    let start = Instant::now();
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let doc = piks::load(&bytes).map_err(|e| format!("{path}: {e}"))?;
    drop(bytes);
    if doc.width != doc.height {
        return Err(format!(
            "{path}: the canvas helper draws square documents only"
        ));
    }
    let side = doc.width;
    let mut editor = Editor::new(doc);
    let mut canvas = Canvas::new(side);
    canvas.present(&mut editor);
    println!("stress_map_open_ms {:.2}", millis(start));
    // The window's compositor below takes the place of the one that drew the first frame.
    drop(canvas);
    let full = full_composite_ms(&mut editor);
    // As `limits.ps1` runs the program: a 1400 by 900 window at zoom 1, stepping frames.
    let mut window = WindowStep::new(editor.doc(), Zoom::One);
    let start = Instant::now();
    (0..STEPS).for_each(|_| window.step());
    println!(
        "stress_map_frame_step_ms {:.2}",
        millis(start) / STEPS as f64
    );
    println!("stress_map_full_composite_ms {full:.2}");
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!(
        "stress_map_threads {}",
        threads.min(pikslar_render::MAX_THREADS)
    );
    let cache = window.compositor().tile_cache_bytes();
    println!("stress_map_tile_cache_kb {:.1}", cache as f64 / 1024.0);
    // The window, its compositor and their caches stay, as in the program.
    println!(
        "stress_map_heap_mb {:.2}",
        live_bytes() as f64 / 1_048_576.0
    );
    if hold > 0 {
        println!("holding");
        std::thread::sleep(Duration::from_secs(hold));
    }
    Ok(())
}
