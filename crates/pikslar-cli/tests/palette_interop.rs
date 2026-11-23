//! The projects `spikes/check-palette-files.py` exports with `palette export` and reads back with
//! readers that are not Pikslar's. `write_palette_fixtures` is ignored: it only writes files.
//!
//! Swatch `i` of a counted case is `colour(i)`; the script computes the same colours itself.

use pikslar_core::{rgba, ColorMode, Document};
use pikslar_format::piks;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

/// Swatch `i` of the counted cases: distinct for every `i` below 65,536, and opaque.
fn colour(i: usize) -> u32 {
    rgba(
        (i & 0xff) as u8,
        ((i >> 8) & 0xff) as u8,
        ((i * 7 + 3) & 0xff) as u8,
        255,
    )
}

fn project(swatches: &[u32]) -> Document {
    let mut doc = Document::new(4, 4, ColorMode::Palette).expect("size");
    let table = Arc::make_mut(&mut doc.colors);
    for &c in swatches {
        table.add_swatch(c).expect("room");
    }
    doc
}

fn cases() -> Vec<(String, Document)> {
    let mut out: Vec<(String, Document)> = [1usize, 16, 256, 257, 4096]
        .iter()
        .map(|&n| {
            let colours: Vec<u32> = (0..n).map(colour).collect();
            (format!("count-{n}"), project(&colours))
        })
        .collect();
    // Partly clear swatches: the formats have no alpha, so each is written as its colour alone.
    let alpha = [
        rgba(255, 0, 0, 255),
        rgba(10, 20, 30, 128),
        rgba(40, 50, 60, 1),
    ];
    out.push(("alpha".into(), project(&alpha)));
    out.push(("none".into(), project(&[])));
    out
}

#[test]
fn the_counted_cases_have_the_swatches_the_script_expects() {
    for (name, doc) in cases() {
        let table = &doc.colors;
        let got: Vec<u32> = table.swatches().iter().map(|&id| table.color(id)).collect();
        if let Some(n) = name.strip_prefix("count-") {
            let n: usize = n.parse().expect("a count");
            assert_eq!(got, (0..n).map(colour).collect::<Vec<_>>(), "{name}");
        }
    }
}

#[test]
#[ignore = "writes the projects for spikes/check-palette-files.py"]
fn write_palette_fixtures() {
    let Some(dir) = std::env::var_os("PIKSLAR_PALETTE_FIXTURES") else {
        panic!("set PIKSLAR_PALETTE_FIXTURES to a folder");
    };
    let dir = PathBuf::from(dir);
    fs::create_dir_all(&dir).expect("folder");
    for (name, doc) in cases() {
        fs::write(dir.join(format!("{name}.piks")), piks::save(&doc)).expect("write");
    }
}
