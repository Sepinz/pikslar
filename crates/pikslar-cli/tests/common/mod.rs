//! Running the built program the way a script would.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Run {
    pub code: i32,
    pub out: String,
    pub err: String,
}

pub fn cli(args: &[&dyn AsRef<std::ffi::OsStr>]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_pikslar-cli"))
        .args(args.iter().map(|a| a.as_ref()))
        .output()
        .expect("the program runs");
    Run {
        code: output.status.code().expect("an exit code"),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

pub fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pikslar-safety/corpus")
        .join(name)
}

/// A fresh, empty folder for one test.
pub fn folder(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pikslar-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

/// Every file in a folder, sorted, so a test can say exactly what is there.
pub fn listing(d: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(d)
        .expect("list")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A project of `frames` frames whose pixels differ in every frame, with `seed` changing all.
pub fn project(at: &Path, side: usize, frames: usize, seed: u32) -> PathBuf {
    use pikslar_core::{ColorMode, Document, Grid};
    let mut doc = Document::new(side, side, ColorMode::Rgba).expect("size");
    for f in 1..frames {
        doc.add_frame(f, false).expect("room");
    }
    for f in 0..frames {
        let px: Vec<u32> = (0..side * side)
            .map(|i| {
                let v = (i as u32 ^ (f as u32).wrapping_mul(2_654_435_761) ^ seed) & 0xff;
                (v << 24) | (v << 16) | ((255 - v) << 8) | (f as u32 & 0xff)
            })
            .collect();
        doc.set_cel(0, f, Grid::from_cells(side, side, &px))
            .expect("cel");
        doc.frames[f].duration_ms = 20 + ((f as u32 + seed) % 7) * 10;
    }
    std::fs::write(at, pikslar_format::piks::save(&doc)).expect("write project");
    at.to_path_buf()
}

/// [`cli`] with `input` on standard input, closed after it.
pub fn cli_input(args: &[&dyn AsRef<std::ffi::OsStr>], input: &[u8]) -> Run {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_pikslar-cli"))
        .args(args.iter().map(|a| a.as_ref()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the program runs");
    let mut stdin = child.stdin.take().expect("stdin");
    let input = input.to_vec();
    // Written from a thread of its own, so a full output pipe cannot hold both sides.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let output = child.wait_with_output().expect("the program ends");
    writer.join().expect("input written");
    Run {
        code: output.status.code().expect("an exit code"),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// The persisted id of a project's bottom layer.
pub fn bottom_layer(path: &Path) -> i64 {
    let doc = pikslar_format::piks::load(&std::fs::read(path).expect("read")).expect("opens");
    i64::from(doc.layers()[0].persisted_id().get())
}
