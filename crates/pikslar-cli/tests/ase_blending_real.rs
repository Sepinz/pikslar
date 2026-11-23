//! Files made by Aseprite itself, drawn as Pikslar exports them and compared with the PNG
//! Aseprite made beside each. Not in this repository: run with `PIKSLAR_ASE_FIXTURES` set to a
//! folder holding asefile's `tests/data` (see `docs/ase-compatibility.md`) and `--ignored`.
//!
//! References are found as `spikes/ase-real.py` finds them: `name.png` for the first frame, or
//! `name_01.png` upward. Pixels clear in both count as the same. Prints a line per file and the
//! totals; fails if fewer files match than `docs/ase-compatibility.md` reports.

use pikslar_core::{Blending, Document};
use pikslar_format::{import_ase, png};
use pikslar_render::{flatten_document, Compositor};
use std::path::{Path, PathBuf};

/// Pictures beside a file that are not its frames.
const NOT_FRAMES: [&str; 9] = [
    "util_extrude",
    "tileset",
    "tileset_1",
    "single_layer",
    "tilemap_multi_map1",
    "tilemap_multi_map2",
    "tilemap_single_tile_1",
    "tilemap_empty_edges_0_0",
    "tilemap_empty_edges_0_1",
];
/// What the docs report: files whose every referenced frame matches.
const MATCHING_AT_LEAST: usize = 29;

fn aseprite_files(root: &Path) -> Vec<PathBuf> {
    let (mut todo, mut out) = (vec![root.to_path_buf()], Vec::new());
    while let Some(dir) = todo.pop() {
        for path in std::fs::read_dir(&dir)
            .expect("a folder")
            .flatten()
            .map(|e| e.path())
        {
            let ext = path.extension().and_then(|e| e.to_str());
            if path.is_dir() {
                todo.push(path);
            } else if matches!(ext, Some("ase" | "aseprite")) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// (frame from 0, reference PNG) for each frame a picture beside the file shows.
fn references(path: &Path) -> Vec<(usize, PathBuf)> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let mut out = Vec::new();
    let first = path.with_extension("png");
    if first.is_file() && !NOT_FRAMES.contains(&stem) {
        out.push((0, first));
    }
    for n in 1..100 {
        let png = path.with_file_name(format!("{stem}_{n:02}.png"));
        if png.is_file() {
            out.push((n - 1, png));
        }
    }
    out
}

/// Differing pixels between a frame and its reference, or None if the sizes differ.
fn differing(doc: &Document, frame: usize, reference: &Path) -> Option<usize> {
    let want = png::decode(&std::fs::read(reference).expect("readable")).expect("a PNG");
    if (want.width, want.height) != (doc.width, doc.height) || frame >= doc.frames.len() {
        return None;
    }
    let got = flatten_document(doc, frame, &mut Compositor::new(1, 1));
    let same = |a: u32, b: u32| a == b || (a >> 24 == 0 && b >> 24 == 0);
    Some(
        got.iter()
            .zip(&want.px)
            .filter(|(a, b)| !same(**a, **b))
            .count(),
    )
}

#[test]
#[ignore = "needs PIKSLAR_ASE_FIXTURES"]
fn aseprite_pictures_match_their_references() {
    let root = PathBuf::from(std::env::var("PIKSLAR_ASE_FIXTURES").expect("set"));
    let (mut compared, mut matching, mut flagged) = (0, 0, 0);
    for path in aseprite_files(&root) {
        let refs = references(&path);
        let Ok((doc, _)) = import_ase(&std::fs::read(&path).expect("readable")) else {
            continue;
        };
        if refs.is_empty() {
            continue;
        }
        compared += 1;
        let aseprite = doc.blending == Blending::Aseprite;
        flagged += usize::from(aseprite);
        let counts: Vec<Option<usize>> = refs
            .iter()
            .map(|(f, png)| differing(&doc, *f, png))
            .collect();
        let exact = counts.iter().all(|c| *c == Some(0));
        matching += usize::from(exact);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let blending = if aseprite { "aseprite" } else { "pikslar" };
        println!("FILE {name} blending={blending} differing={counts:?} exact={exact}");
    }
    println!("TOTAL compared={compared} matching={matching} aseprite_blending={flagged}");
    assert!(compared > 0, "no references under {}", root.display());
    assert!(matching >= MATCHING_AT_LEAST, "{matching} files match");
}
