//! Tilemaps of files made by Aseprite itself, which are not in this repository: run with
//! `PIKSLAR_ASE_FIXTURES` set to a folder holding Aseprite's `tests/sprites` and asefile's
//! `tests/data` (see `docs/ase-compatibility.md`), and `--ignored`.
//!
//! Two kinds of picture are compared with what the importer made, by expanding every map
//! cell by cell (`src/ase_expand.rs`, since the renderer for tilemaps lands in another phase):
//! asefile's reference PNGs, which its own tests take for what Aseprite shows, and, for the files
//! that come with none, pictures drawn from the file by `spikes/ase-tilemap-expect.py`, whose
//! folder is `PIKSLAR_ASE_EXPECTED`. Pixels clear in both are the same.

#[path = "../src/ase_expand.rs"]
mod ase_expand;

use ase_expand::expand;
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{ColorMode, Document};
use pikslar_format::{import_ase, png};
use std::path::{Path, PathBuf};

fn find(name: &str) -> PathBuf {
    let root = PathBuf::from(std::env::var("PIKSLAR_ASE_FIXTURES").expect("PIKSLAR_ASE_FIXTURES"));
    let mut todo = vec![root.clone()];
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
    let bytes = std::fs::read(find(name)).expect("a readable file");
    import_ase(&bytes).expect("a file that imports").0
}

fn picture(path: &Path) -> png::Image {
    png::decode(&std::fs::read(path).expect("a readable picture")).expect("a PNG")
}

/// A value of the expansion as a color: a palette document's values are table places.
fn shown(doc: &Document, layer: usize, value: u32) -> u32 {
    match doc.layers()[layer].mode {
        ColorMode::Palette => doc.colors.color(value),
        _ => value,
    }
}

fn canvas(doc: &Document, layer: usize, frame: usize) -> Vec<u32> {
    let cells = expand(doc, layer, frame);
    cells.into_iter().map(|v| shown(doc, layer, v)).collect()
}

/// The first place where the pictures differ, as a message, clear in both being the same.
fn differ(got: &[u32], want: &[u32]) -> Option<String> {
    if got.len() != want.len() {
        return Some(format!("{} pixels, not {}", got.len(), want.len()));
    }
    let bad = got
        .iter()
        .zip(want)
        .enumerate()
        .find(|&(_, (g, w))| g != w && (g >> 24 != 0 || w >> 24 != 0));
    bad.map(|(at, (g, w))| format!("pixel {at}: {g:08x}, expected {w:08x}"))
}

fn tile_colors(doc: &Document, set: u32, tile: u32) -> Vec<u32> {
    let set = doc.tileset(TilesetId::new(set)).expect("a tileset");
    let (w, h) = set.tile_size();
    let image = &set
        .tile(TileId::new(tile).expect("an id"))
        .expect("a tile")
        .image;
    let palette = set.mode() == ColorMode::Palette;
    let value = |x, y| image.get(x as i32, y as i32);
    let at = |v: u32| if palette { doc.colors.color(v) } else { v };
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| at(value(x, y)))
        .collect()
}

fn assert_picture(name: &str, got: &[u32], reference: &str) {
    let want = picture(&find(&format!("{reference}.png")));
    assert_eq!(got.len(), want.width * want.height, "{name}: size");
    assert!(
        want.px.iter().any(|&p| p >> 24 != 0),
        "{name}: the reference is blank"
    );
    assert_eq!(differ(got, &want.px), None, "{name}");
}

#[test]
#[ignore = "needs PIKSLAR_ASE_FIXTURES"]
fn maps_equal_asefiles_reference_pictures() {
    for name in ["tilemap", "tilemap_indexed", "tilemap_grayscale"] {
        let doc = open(&format!("{name}.aseprite"));
        assert_eq!(doc.layers().len(), 1);
        assert_picture(name, &canvas(&doc, 0, 0), name);
    }
    // tilemap_multi: a paint layer under two maps of two tilesets (of 20 by 16 and 16 by 16).
    let doc = open("tilemap_multi.aseprite");
    assert_eq!(doc.tilesets.len(), 2);
    let names: Vec<&str> = doc.layers().iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Layer 1", "Tilemap 1", "Tilemap 2"]);
    for (layer, reference) in [(1, "tilemap_multi_map1"), (2, "tilemap_multi_map2")] {
        assert_picture(reference, &canvas(&doc, layer, 0), reference);
    }
    let (maps, paint) = (
        &[canvas(&doc, 1, 0), canvas(&doc, 2, 0)],
        doc.layers()[0].cel(0),
    );
    let whole: Vec<u32> = (0..doc.width * doc.height)
        .map(|i| {
            let under = paint.get((i % doc.width) as i32, (i / doc.width) as i32);
            maps.iter().fold(
                under,
                |below, m| if m[i] >> 24 == 255 { m[i] } else { below },
            )
        })
        .collect();
    assert_picture("tilemap_multi", &whole, "tilemap_multi");
}

#[test]
#[ignore = "needs PIKSLAR_ASE_FIXTURES"]
fn tiles_equal_asefiles_reference_pictures() {
    let doc = open("tileset.aseprite");
    // Two tilesets of five tiles each, the first the layer's; tile 0 is the empty tile.
    assert_eq!(doc.tilesets.len(), 2);
    let set = doc.tileset(TilesetId::new(1)).expect("tileset");
    assert_eq!((set.len(), set.tile_size()), (4, (16, 16)));
    let stacked = picture(&find("tileset.png"));
    assert_eq!((stacked.width, stacked.height), (16, 80));
    for tile in 1..=4u32 {
        let want = &stacked.px[tile as usize * 256..(tile as usize + 1) * 256];
        assert_eq!(
            differ(&tile_colors(&doc, 1, tile), want),
            None,
            "tile {tile}"
        );
    }
    assert_picture("tileset_1", &tile_colors(&doc, 1, 1), "tileset_1");
    let multi = open("tilemap_multi.aseprite");
    assert_picture(
        "single tile",
        &tile_colors(&multi, 1, 3),
        "tilemap_single_tile_1",
    );
    let edges = open("tilemap_empty_edges.aseprite");
    let layer = &edges.layers()[0];
    let (first, below) = (
        layer.cel(0).image().get(0, 0),
        layer.cel(0).image().get(0, 1),
    );
    assert_eq!((first, below), (1, 4));
    assert_picture(
        "edges 0 0",
        &tile_colors(&edges, 1, 1),
        "tilemap_empty_edges_0_0",
    );
    assert_picture(
        "edges 0 1",
        &tile_colors(&edges, 1, 4),
        "tilemap_empty_edges_0_1",
    );
}

/// The files with tilemap layers and what each should hold: (file, tilesets, layers).
const FILES: [(&str, usize, usize); 11] = [
    ("tilemap.aseprite", 2, 1),
    ("tilemap_empty_edges.aseprite", 1, 1),
    ("tilemap_grayscale.aseprite", 1, 1),
    ("tilemap_indexed.aseprite", 2, 1),
    ("tilemap_multi.aseprite", 2, 3),
    ("tileset.aseprite", 2, 1),
    ("cel_overflow.aseprite", 1, 2),
    ("2x2tilemap2x2tile.aseprite", 1, 1),
    ("2x3tilemap-indexed.aseprite", 1, 1),
    ("3x2tilemap-grayscale.aseprite", 1, 2),
    ("file-tests-props.aseprite", 1, 4),
];

#[test]
#[ignore = "needs PIKSLAR_ASE_FIXTURES and PIKSLAR_ASE_EXPECTED"]
fn every_tilemap_layer_equals_the_picture_drawn_from_the_file() {
    let expected = PathBuf::from(std::env::var("PIKSLAR_ASE_EXPECTED").expect("a folder"));
    let (mut exact, mut checked) = (0, 0);
    for (file, tilesets, layers) in FILES {
        let doc = open(file);
        assert_eq!(
            (doc.tilesets.len(), doc.layers().len()),
            (tilesets, layers),
            "{file}"
        );
        let stem = file.trim_end_matches(".aseprite");
        let mut bad = Vec::new();
        for (index, layer) in doc.layers().iter().enumerate() {
            if layer.tileset().is_none() {
                continue;
            }
            for frame in 0..doc.frames.len() {
                let path = expected.join(format!("{stem}.{index}.{frame}.png"));
                let got = canvas(&doc, index, frame);
                let want = if path.is_file() {
                    let px = picture(&path).px;
                    assert!(px.iter().any(|&p| p >> 24 != 0), "{file}: a blank picture");
                    px
                } else {
                    vec![0; got.len()]
                };
                checked += 1;
                if let Some(why) = differ(&got, &want) {
                    bad.push(format!("layer {index} frame {frame}: {why}"));
                }
            }
        }
        eprintln!(
            "{file}: {}",
            if bad.is_empty() { "exact" } else { "DIFFERS" }
        );
        assert!(bad.is_empty(), "{file}: {bad:?}");
        exact += 1;
    }
    eprintln!(
        "{exact} of {} files, {checked} layer frames, exact",
        FILES.len()
    );
}
