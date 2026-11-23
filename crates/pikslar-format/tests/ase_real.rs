//! Files made by Aseprite itself, which are not in this repository: run with
//! `PIKSLAR_ASE_FIXTURES` set to a folder holding Aseprite's `tests/sprites` and asefile's
//! `tests/data` (see `docs/ase-compatibility.md`), and `--ignored`. Pixels are compared by
//! `spikes/ase-real.py`; this checks what a picture cannot show: holds and the palette.

use pikslar_core::{rgba, Document};
use pikslar_format::import_ase;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(std::env::var("PIKSLAR_ASE_FIXTURES").expect("PIKSLAR_ASE_FIXTURES is set"))
}

fn find(root: &Path, name: &str) -> PathBuf {
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        for entry in std::fs::read_dir(&dir)
            .expect("a readable folder")
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.file_name().is_some_and(|n| n == name) {
                return path;
            }
        }
    }
    panic!("{name} is not under {}", root.display());
}

fn open(name: &str) -> Document {
    let bytes = std::fs::read(find(&fixtures(), name)).expect("a readable file");
    import_ase(&bytes).expect("a file that imports").0
}

#[test]
#[ignore = "needs PIKSLAR_ASE_FIXTURES"]
fn linked_cels_from_aseprite_become_holds() {
    // tags3: layer "b" links frames 6, 7, 9, 10, 11 and 12 (from 1) to 1, 2, 8, 3, 4 and 5.
    let doc = open("tags3.aseprite");
    let b = &doc.layers()[1];
    let ids: Vec<u32> = b.links().collect();
    for (from, to) in [(5, 0), (6, 1), (8, 7), (9, 2), (10, 3), (11, 4)] {
        assert_ne!(ids[to], 0, "frame {to}");
        assert_eq!(ids[from], ids[to], "frame {from} holds frame {to}");
    }
    let mut held: Vec<u32> = [0, 1, 7, 2, 3, 4].iter().map(|&f| ids[f]).collect();
    held.sort_unstable();
    held.dedup();
    assert_eq!(held.len(), 6, "each hold is its own: {ids:?}");
    // Layer "a" is one drawing linked across all twelve frames.
    let a: Vec<u32> = doc.layers()[0].links().collect();
    assert!(a.iter().all(|&id| id != 0 && id == a[0]), "{a:?}");
}

#[test]
#[ignore = "needs PIKSLAR_ASE_FIXTURES"]
fn the_palette_keeps_its_colors_and_their_alpha() {
    // asefile's test: 85 colors, the first (46, 34, 47) and the 72nd black at alpha 83.
    let doc = open("palette.aseprite");
    // Place 0 of the table is clear, so a palette index is its place less one.
    assert_eq!(doc.colors.color(1), rgba(46, 34, 47, 255));
    assert_eq!(doc.colors.color(72), rgba(0, 0, 0, 83));
}

fn open_with_warnings(name: &str) -> (Document, Vec<pikslar_format::AseWarning>) {
    let bytes = std::fs::read(find(&fixtures(), name)).expect("a readable file");
    import_ase(&bytes).expect("a file that imports")
}

#[test]
#[ignore = "needs PIKSLAR_ASE_FIXTURES"]
fn slices_from_aseprite_keep_their_names_pivots_and_centres() {
    // asefile's test: "Slice 1" has a pivot at x 4 and four keys, "Slice 2" a centre of
    // (3, 3) and 2 by 2 and one key.
    let (doc, _) = open_with_warnings("slice_advanced.aseprite");
    let names: Vec<&str> = doc.slices.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Slice 1", "Slice 2"]);
    assert_eq!(doc.slices[0].pivot.map(|p| p.0), Some(4));
    let c = doc.slices[1].center.expect("a centre");
    assert_eq!((c.x, c.y, c.width, c.height), (3, 3, 2, 2));
    let (doc, warnings) = open_with_warnings("slices-moving.aseprite");
    assert_eq!(doc.slices.len(), 1);
    assert!(warnings
        .iter()
        .any(|w| w.to_string().contains("changes from frame to frame")));
}

#[test]
#[ignore = "needs PIKSLAR_ASE_FIXTURES"]
fn a_grayscale_file_opens_with_its_grey_values_and_no_warning_about_them() {
    let (doc, warnings) = open_with_warnings("grayscale.aseprite");
    assert!(
        warnings
            .iter()
            .all(|w| !w.to_string().contains("grayscale")),
        "{warnings:?}"
    );
    let cel = doc.layers()[0].cel(0);
    let seen = (0..doc.height as i32)
        .flat_map(|y| (0..doc.width as i32).map(move |x| (x, y)))
        .map(|(x, y)| cel.get(x, y))
        .filter(|&c| c != 0)
        .count();
    assert!(seen > 0, "the grey layer is not empty");
}
