//! `pikslar-bench save-study [folder]`: how long Ctrl+S holds the UI thread on large documents,
//! split into the steps the program takes (B1 plan 3.3 item 11). Measures nothing for the gate.
//!
//! The program's `files::save` lives in the `pikslar` binary, which nothing can link to, so its
//! `checked_bytes` is repeated here call for call: encode, reopen, re-encode and compare. The
//! write is the real `write_atomic`, through `write_atomic_checked`, whose step calls mark the
//! split points. After a save the program also hashes the bytes for Rewind; that is timed too.
//!
//! Since B1 Batch 2R the save runs on a thread of its own, and the UI thread only copies the
//! document and starts that thread: "copy" and "spawn" time those two, and "ui now" is their
//! sum, what Ctrl+S holds the window for. "ui before" is what it held before. The program now
//! puts the file in place with `ReplaceFileW` and reads it back; this study still renames,
//! which changes only the saving thread's time.

use crate::stress::{full_document, stress_document};
use crate::stress_map::stress_map_document;
use crate::workloads::{indexed_or_plain, percentile, sprite_layer, Rng};
use pikslar_core::{ColorMode, Document, Grid};
use pikslar_format::atomic::{write_atomic_checked, SaveStep};
use pikslar_format::{piks, MAX_FILE_BYTES};
use std::path::{Path, PathBuf};
use std::time::Instant;

const SAVES: usize = 15;
const ANIM_SIZE: (usize, usize) = (256, 144);
const ANIM_LAYERS: usize = 12;
const ANIM_FRAMES: usize = 24;

/// The parts of one save, in milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct Phases {
    pub encode: f64,
    pub verify: f64,
    /// Removing any old temporary file and creating the new one, and the read-only check.
    pub create: f64,
    pub write: f64,
    /// `sync_all` and closing the file.
    pub flush: f64,
    pub read_back: f64,
    pub rename: f64,
    /// Rewind's hash of the saved bytes, which the program took on the UI thread after a save.
    pub print: f64,
    /// Copying the document, which the UI thread does before handing it to the saving thread.
    pub copy: f64,
    /// Starting the saving thread.
    pub spawn: f64,
}

impl Phases {
    /// What `files::save` takes: everything but the Rewind hash.
    pub fn save_total(&self) -> f64 {
        self.encode
            + self.verify
            + self.create
            + self.write
            + self.flush
            + self.read_back
            + self.rename
    }
}

fn ms(from: Instant, to: Instant) -> f64 {
    to.duration_since(from).as_secs_f64() * 1000.0
}

/// `files::checked_bytes`, as the program has it.
fn checked_bytes(doc: &Document) -> Result<Vec<u8>, String> {
    let bytes = piks::try_save(doc).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("too large".into());
    }
    Ok(bytes)
}

fn verified(bytes: &[u8]) -> Result<(), String> {
    let (reopened, notes) = piks::load_noted(bytes).map_err(|e| e.to_string())?;
    if !notes.can_save_over() || piks::save(&reopened) != bytes {
        return Err("would not reopen identically".into());
    }
    Ok(())
}

/// The copy the UI thread takes and the thread it starts, in milliseconds.
fn timed_handoff(doc: &Document) -> Result<(f64, f64), String> {
    let start = Instant::now();
    let copy = doc.clone();
    let copied = Instant::now();
    let worker = std::thread::Builder::new()
        .name("save".into())
        .spawn(move || drop(std::hint::black_box(copy)))
        .map_err(|e| e.to_string())?;
    let spawned = Instant::now();
    worker
        .join()
        .map_err(|_| "the thread panicked".to_string())?;
    Ok((ms(start, copied), ms(copied, spawned)))
}

/// One save of `doc` to `path`, as Ctrl+S does it, timed in parts.
pub fn timed_save(doc: &Document, path: &Path) -> Result<Phases, String> {
    let (copy, spawn) = timed_handoff(doc)?;
    let start = Instant::now();
    let bytes = checked_bytes(doc)?;
    let encoded = Instant::now();
    verified(&bytes)?;
    let checked = Instant::now();
    let mut marks = [checked; 4];
    let mut mark = |step: SaveStep, _: &Path| {
        let slot = match step {
            SaveStep::Created => Some(0),
            SaveStep::Written => Some(1),
            SaveStep::Synced => Some(2),
            SaveStep::Verified => Some(3),
            SaveStep::HalfWritten => None,
        };
        if let Some(slot) = slot {
            marks[slot] = Instant::now();
        }
        Ok(())
    };
    write_atomic_checked(path, &bytes, &mut mark).map_err(|e| e.to_string())?;
    let renamed = Instant::now();
    let print = pikslar_rewind::hash128(&bytes);
    std::hint::black_box(print);
    let printed = Instant::now();
    Ok(Phases {
        encode: ms(start, encoded),
        verify: ms(encoded, checked),
        create: ms(checked, marks[0]),
        write: ms(marks[0], marks[1]),
        flush: ms(marks[1], marks[2]),
        read_back: ms(marks[2], marks[3]),
        rename: ms(marks[3], renamed),
        print: ms(renamed, printed),
        copy,
        spawn,
    })
}

/// 256 by 144, 12 layers by 24 frames, every drawing different, half palette and half RGBA.
pub fn animation_document() -> Document {
    let (w, h) = ANIM_SIZE;
    let mut doc = Document::new(w, h, ColorMode::Palette).expect("valid size");
    for layer in 1..ANIM_LAYERS {
        let mode = if layer % 2 == 0 {
            ColorMode::Palette
        } else {
            ColorMode::Rgba
        };
        doc.add_layer(layer, mode).expect("room");
    }
    for frame in 1..ANIM_FRAMES {
        doc.add_frame(frame, false).expect("room");
    }
    let mut rng = Rng(23);
    for layer in 0..ANIM_LAYERS {
        for frame in 0..ANIM_FRAMES {
            let cells = sprite_layer(w, h, &mut rng);
            let mode = doc.layers()[layer].mode;
            let stored = indexed_or_plain(&mut doc, mode, cells);
            doc.set_cel(layer, frame, Grid::from_cells(w, h, &stored))
                .expect("canvas size");
        }
    }
    doc
}

fn square_2048() -> Document {
    full_document(2048, 2048)
}

fn sheet_2048x3040() -> Document {
    full_document(2048, 3040)
}

type Case = (&'static str, fn() -> Document);
type Column = (&'static str, fn(&Phases) -> f64);

const CASES: [Case; 5] = [
    ("2048x2048 one full RGBA layer", square_2048),
    ("2048x3040 one full RGBA layer", sheet_2048x3040),
    ("stress 128x128 12 layers 400 frames", stress_document),
    ("tilemap stress map 2048x2048", stress_map_document),
    ("256x144 12 layers 24 frames", animation_document),
];

fn column(runs: &[Phases], pick: fn(&Phases) -> f64) -> (f64, f64) {
    let mut values: Vec<f64> = runs.iter().map(pick).collect();
    values.sort_by(f64::total_cmp);
    (percentile(&values, 0.5), values[values.len() - 1])
}

fn report(name: &str, size: u64, runs: &[Phases]) {
    let columns: [Column; 13] = [
        ("encode", |p| p.encode),
        ("verify", |p| p.verify),
        ("create", |p| p.create),
        ("write", |p| p.write),
        ("flush", |p| p.flush),
        ("read-back", |p| p.read_back),
        ("rename", |p| p.rename),
        ("save total", Phases::save_total),
        ("rewind hash", |p| p.print),
        ("ui before", |p| p.save_total() + p.print),
        ("copy", |p| p.copy),
        ("spawn", |p| p.spawn),
        ("ui now", |p| p.copy + p.spawn),
    ];
    println!(
        "{name}: {size} bytes, {} saves, ms median / max",
        runs.len()
    );
    for (label, pick) in columns {
        let (median, max) = column(runs, pick);
        println!("  {label:<12} {median:>9.2} {max:>9.2}");
    }
}

fn run_case(name: &str, make: fn() -> Document, folder: &Path) -> Result<(), String> {
    let doc = make();
    let path = folder.join(format!("{}.piks", name.replace(' ', "-")));
    // The first save creates the file; Ctrl+S normally replaces one.
    timed_save(&doc, &path)?;
    let runs = (0..SAVES)
        .map(|_| timed_save(&doc, &path))
        .collect::<Result<Vec<_>, _>>()?;
    let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    report(name, size, &runs);
    std::fs::remove_file(&path).map_err(|e| e.to_string())
}

pub fn run(folder: Option<PathBuf>) -> Result<(), String> {
    let folder = folder.unwrap_or_else(|| std::env::temp_dir().join("pikslar-save-study"));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    println!("Saving to {}", folder.display());
    for (name, make) in CASES {
        run_case(name, make, &folder)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timed_save_writes_what_the_program_would_and_accounts_for_its_parts() {
        let folder =
            std::env::temp_dir().join(format!("pikslar-save-study-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a folder");
        let path = folder.join("small.piks");
        let doc = full_document(64, 48);
        let phases = timed_save(&doc, &path).expect("a save");
        let again = timed_save(&doc, &path).expect("a save over it");
        assert_eq!(std::fs::read(&path).expect("the file"), piks::save(&doc));
        for p in [phases, again] {
            let parts = [
                p.encode,
                p.verify,
                p.create,
                p.write,
                p.flush,
                p.read_back,
                p.rename,
            ];
            assert!(parts.iter().all(|t| *t >= 0.0));
            assert!(p.save_total() > 0.0);
        }
        std::fs::remove_dir_all(&folder).expect("cleaned up");
    }

    #[test]
    fn the_animation_document_has_the_asked_for_shape_and_saves_checked() {
        let doc = animation_document();
        assert_eq!((doc.width, doc.height), ANIM_SIZE);
        assert_eq!(doc.layers().len(), ANIM_LAYERS);
        assert_eq!(doc.frames.len(), ANIM_FRAMES);
        let bytes = checked_bytes(&doc).expect("encodes");
        verified(&bytes).expect("reopens identically");
    }
}
