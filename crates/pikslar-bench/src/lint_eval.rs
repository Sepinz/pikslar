//! Reports what Pixel Lint finds on real art, and draws samples of it to judge by eye.
//!
//! Usage: `pikslar-bench lint-eval <png or folder> <output prefix>`

use pikslar_core::{lint_all, Finding, LintKind};
use pikslar_format::png::{self, Image};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const KINDS: [(LintKind, &str); 4] = [
    (LintKind::Stray, "stray"),
    (LintKind::Double, "double"),
    (LintKind::Jag, "jag"),
    (LintKind::NearColors, "near-colors"),
];
const CROP: usize = 23;
const ZOOM: usize = 7;
const GAP: usize = 4;
const COLUMNS: usize = 9;
const ROWS: usize = 5;
const MARK: u32 = u32::from_le_bytes([255, 0, 255, 255]);
const SHEET_BACK: u32 = u32::from_le_bytes([20, 20, 24, 255]);

#[derive(Default)]
struct Tally {
    files: usize,
    opaque: usize,
    time: Duration,
    worst: Duration,
    /// Per kind: which file and what was found.
    found: [Vec<(usize, Finding)>; 4],
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    let entries = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?.path();
        if entry.is_dir() {
            collect(&entry, out)?;
        } else if entry.extension().is_some_and(|e| e == "png") {
            out.push(entry);
        }
    }
    Ok(())
}

fn load(path: &Path) -> Option<Image> {
    png::decode(&std::fs::read(path).ok()?).ok()
}

fn add_file(tally: &mut Tally, index: usize, image: &Image) {
    let start = Instant::now();
    let findings = lint_all(&image.px, image.width, image.height, |c| c);
    let spent = start.elapsed();
    tally.files += 1;
    tally.opaque += image.px.iter().filter(|&&p| p != 0).count();
    tally.time += spent;
    tally.worst = tally.worst.max(spent);
    for finding in findings {
        let slot = KINDS.iter().position(|(k, _)| *k == finding.kind);
        if let Some(slot) = slot {
            tally.found[slot].push((index, finding));
        }
    }
}

fn print_report(tally: &Tally) {
    println!("files {}, opaque pixels {}", tally.files, tally.opaque);
    println!(
        "lint time: total {:.1} ms, worst file {:.2} ms",
        tally.time.as_secs_f64() * 1000.0,
        tally.worst.as_secs_f64() * 1000.0
    );
    for (slot, (_, name)) in KINDS.iter().enumerate() {
        let list = &tally.found[slot];
        let mut files: Vec<usize> = list.iter().map(|f| f.0).collect();
        files.dedup();
        println!(
            "{name:<12} {:>8}  per file {:>7.2}  per 1000 opaque {:>7.2}  in {} files",
            list.len(),
            list.len() as f64 / tally.files.max(1) as f64,
            list.len() as f64 * 1000.0 / tally.opaque.max(1) as f64,
            files.len()
        );
    }
}

fn checker(x: usize, y: usize) -> u32 {
    let shade = if (x / 2 + y / 2) % 2 == 0 { 70 } else { 90 };
    u32::from_le_bytes([shade, shade, shade, 255])
}

/// Straight alpha over the checkerboard, so soft edges look as they do in the editor.
fn over_checker(px: u32, x: usize, y: usize) -> u32 {
    let [r, g, b, a] = px.to_le_bytes();
    let [br, bg, bb, _] = checker(x, y).to_le_bytes();
    let mix = |f: u8, b: u8| ((f as u32 * a as u32 + b as u32 * (255 - a as u32)) / 255) as u8;
    u32::from_le_bytes([mix(r, br), mix(g, bg), mix(b, bb), 255])
}

struct Sheet {
    width: usize,
    px: Vec<u32>,
}

impl Sheet {
    fn set(&mut self, x: usize, y: usize, color: u32) {
        let index = y * self.width + x;
        if x < self.width && index < self.px.len() {
            self.px[index] = color;
        }
    }

    fn frame(&mut self, left: usize, top: usize, w: usize, h: usize) {
        for x in left..left + w {
            self.set(x, top, MARK);
            self.set(x, top + h - 1, MARK);
        }
        for y in top..top + h {
            self.set(left, y, MARK);
            self.set(left + w - 1, y, MARK);
        }
    }

    /// Draws the area around a finding, with the finding outlined, into one grid slot.
    fn draw_crop(&mut self, slot: usize, image: &Image, finding: &Finding) {
        let cell = CROP * ZOOM + GAP;
        let (left, top) = (slot % COLUMNS * cell, slot / COLUMNS * cell);
        let (cx, cy) = (finding.x + finding.w / 2, finding.y + finding.h / 2);
        let (ox, oy) = (cx - CROP as i32 / 2, cy - CROP as i32 / 2);
        for row in 0..CROP {
            for column in 0..CROP {
                let (ix, iy) = (ox + column as i32, oy + row as i32);
                let inside = ix >= 0
                    && iy >= 0
                    && (ix as usize) < image.width
                    && (iy as usize) < image.height;
                let px = if inside {
                    image.px[iy as usize * image.width + ix as usize]
                } else {
                    0
                };
                let shown = over_checker(px, column, row);
                for dy in 0..ZOOM {
                    for dx in 0..ZOOM {
                        self.set(left + column * ZOOM + dx, top + row * ZOOM + dy, shown);
                    }
                }
            }
        }
        let scale = ZOOM as i32;
        let frame_x = (finding.x - ox) * scale - 1;
        let frame_y = (finding.y - oy) * scale - 1;
        self.frame(
            left + frame_x.max(0) as usize,
            top + frame_y.max(0) as usize,
            (finding.w * scale + 2) as usize,
            (finding.h * scale + 2) as usize,
        );
    }
}

/// Every Nth finding, so the sample covers the whole collection.
fn write_sheet(paths: &[PathBuf], list: &[(usize, Finding)], out: &Path) -> Result<(), String> {
    if list.is_empty() {
        return Ok(());
    }
    let capacity = COLUMNS * ROWS;
    let step = (list.len() / capacity).max(1);
    let cell = CROP * ZOOM + GAP;
    let (width, height) = (COLUMNS * cell, ROWS * cell);
    let mut sheet = Sheet {
        width,
        px: vec![SHEET_BACK; width * height],
    };
    let mut cached: Option<(usize, Image)> = None;
    for (slot, (file, finding)) in list.iter().step_by(step).take(capacity).enumerate() {
        if cached.as_ref().is_none_or(|(f, _)| f != file) {
            cached = load(&paths[*file]).map(|image| (*file, image));
        }
        if let Some((_, image)) = &cached {
            sheet.draw_crop(slot, image, finding);
        }
    }
    let image = Image {
        width,
        height,
        px: sheet.px,
    };
    let bytes = png::encode(&image).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::write(out, bytes).map_err(|e| format!("{}: {e}", out.display()))
}

pub fn run(source: &Path, prefix: &str) -> Result<(), String> {
    let mut paths = Vec::new();
    collect(source, &mut paths)?;
    paths.sort();
    let mut tally = Tally::default();
    for (index, path) in paths.iter().enumerate() {
        if let Some(image) = load(path) {
            add_file(&mut tally, index, &image);
        }
    }
    print_report(&tally);
    for (slot, (_, name)) in KINDS.iter().enumerate() {
        let out = PathBuf::from(format!("{prefix}-{name}.png"));
        write_sheet(&paths, &tally.found[slot], &out)?;
    }
    Ok(())
}
