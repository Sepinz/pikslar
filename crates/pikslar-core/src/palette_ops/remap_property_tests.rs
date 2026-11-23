//! Property tests for the lossless remaps and Replace colour over random documents: every
//! layer shows the same colours afterwards, holds, links and shared drawings stay shared, and
//! undo gives the identical document. Replace colour changes exactly the expected pixels.
//!
//! A quick run goes with the suite; `many_random_documents` runs 2,000 with `--ignored`.

use super::remap::RemapKind;
use super::remap_build::{build, Dice, Float};
use super::rewrite::used_ids;
use crate::color::ColorTable;
use crate::command::PaletteCmd;
use crate::document::{ColorMode, Document};
use crate::editor::Editor;
use std::collections::HashMap;

const QUICK: u64 = 300;
const LONG: u64 = 2_000;
const KINDS: [RemapKind; 3] = [
    RemapKind::ToSwatchOrder,
    RemapKind::MergeDuplicates,
    RemapKind::RemoveUnused,
];

/// One cel: its link, its place and every pixel around the canvas.
type SeenCel = (u32, (i16, i16), Vec<u32>);
/// What each frame of each layer, or each tile of each tileset, shows.
type Looks = Vec<Vec<Vec<u32>>>;

/// Everything a document keeps in its images, as stored ids, and which images are one.
#[derive(Clone, Debug, PartialEq)]
struct Exact {
    colors: ColorTable,
    /// Per layer and frame: link, place and every pixel around the canvas.
    cels: Vec<Vec<SeenCel>>,
    /// Per tileset and tile: id and pixels.
    tiles: Vec<Vec<(u32, Vec<u32>)>>,
    /// Which drawing each cel and tile shows, numbered by first appearance.
    sharing: Vec<usize>,
}

fn exact(doc: &Document) -> Exact {
    let mut numbers: HashMap<usize, usize> = HashMap::new();
    let mut sharing = Vec::new();
    let mut number = |key: usize| {
        let next = numbers.len();
        sharing.push(*numbers.entry(key).or_insert(next));
    };
    let mut cels = Vec::new();
    for layer in doc.layers() {
        let mut row = Vec::new();
        for cel in layer.cels() {
            number(cel.image_key() as usize);
            let around = (-8..40).flat_map(|y| (-8..40).map(move |x| (x, y)));
            let pixels = around.map(|(x, y)| cel.get(x, y)).collect();
            row.push((cel.link(), cel.position(), pixels));
        }
        cels.push(row);
    }
    let mut tiles = Vec::new();
    for set in doc.tilesets.iter() {
        let mut row = Vec::new();
        for entry in set.tiles() {
            number(std::sync::Arc::as_ptr(&entry.image) as usize);
            let pixels = (0..64).map(|i| entry.image.get(i % 8, i / 8)).collect();
            row.push((entry.id.get(), pixels));
        }
        tiles.push(row);
    }
    Exact {
        colors: (*doc.colors).clone(),
        cels,
        tiles,
        sharing,
    }
}

/// The colours every cel and tile shows: ids looked up where the layer or tileset is palette.
fn shown(doc: &Document) -> (Looks, Looks) {
    let look = |mode: ColorMode, cell: u32| match mode {
        ColorMode::Palette => doc.colors.color(cell),
        ColorMode::Rgba => cell,
    };
    let cels = doc.layers().iter().map(|layer| {
        let frames = layer.cels().iter();
        let around = |cel: &crate::Cel| -> Vec<u32> {
            let at = (-8..40).flat_map(|y| (-8..40).map(move |x| (x, y)));
            at.map(|(x, y)| look(layer.mode, cel.get(x, y))).collect()
        };
        frames.map(around).collect()
    });
    let tiles = doc.tilesets.iter().map(|set| {
        let one = |e: &crate::tileset::TileEntry| -> Vec<u32> {
            (0..64)
                .map(|i| look(set.mode(), e.image.get(i % 8, i / 8)))
                .collect()
        };
        set.tiles().iter().map(one).collect()
    });
    (cels.collect(), tiles.collect())
}

fn assert_kind_result(kind: RemapKind, before: &Exact, e: &Editor, seed: u64) {
    let table = &e.doc().colors;
    let used = used_ids(e.doc());
    match kind {
        RemapKind::ToSwatchOrder => {
            let n = table.swatches().len() as u32;
            let first: Vec<u32> = (1..=n).collect();
            assert_eq!(table.swatches(), first, "seed {seed}");
            assert_eq!(table.len(), before.colors.len(), "seed {seed}");
        }
        RemapKind::MergeDuplicates => {
            for id in 1..table.len() as u32 {
                let lowest = table.id_for_existing(table.color(id)).expect("known");
                assert!(!used[id as usize] || lowest == id, "seed {seed}: id {id}");
            }
        }
        RemapKind::RemoveUnused => {
            let wanted = |id: u32| {
                let group = table.groups().iter().any(|g| g.ids.contains(&id));
                used[id as usize] || table.swatches().contains(&id) || group
            };
            assert!((1..table.len() as u32).all(wanted), "seed {seed}");
            assert!(table.len() <= before.colors.len(), "seed {seed}");
        }
    }
}

fn check_remap(seed: u64, kind: RemapKind) {
    let mut e = build(seed, Float::No);
    let (before, looks) = (exact(e.doc()), shown(e.doc()));
    let depth = e.history.undo_depth();
    e.apply(PaletteCmd::Remap(kind)).expect("remapped");
    let after = exact(e.doc());
    assert_eq!(shown(e.doc()), looks, "seed {seed} {kind:?}: looks");
    assert_eq!(after.sharing, before.sharing, "seed {seed} {kind:?}");
    assert_eq!(
        (after
            .cels
            .iter()
            .flatten()
            .map(|c| (c.0, c.1))
            .collect::<Vec<_>>()),
        (before
            .cels
            .iter()
            .flatten()
            .map(|c| (c.0, c.1))
            .collect::<Vec<_>>()),
        "seed {seed} {kind:?}: holds and places"
    );
    assert_kind_result(kind, &before, &e, seed);
    let steps = e.history.undo_depth() - depth;
    assert!(steps <= 1, "seed {seed} {kind:?}: {steps} steps");
    if steps == 1 {
        assert!(e.undo());
        assert_eq!(exact(e.doc()), before, "seed {seed} {kind:?}: undo");
        assert!(e.redo());
        assert_eq!(exact(e.doc()), after, "seed {seed} {kind:?}: redo");
    }
}

/// The remap on a document with floating pixels shows what the same document shows with them
/// put down first.
fn check_floating(seed: u64, kind: RemapKind) {
    let anchored = build(seed, Float::Anchored);
    let mut pending = build(seed, Float::Pending);
    assert!(pending.floating.is_some(), "seed {seed}");
    pending.apply(PaletteCmd::Remap(kind)).expect("remapped");
    assert!(pending.floating.is_none(), "seed {seed}: put down");
    assert_eq!(
        shown(pending.doc()),
        shown(anchored.doc()),
        "seed {seed} {kind:?}"
    );
}

/// Ids named by the layers a replace reaches, worked out apart from the code under test.
fn expected(before: &Exact, doc: &Document, from: u32, to: u32, layer: Option<usize>) -> Exact {
    let mut want = before.clone();
    let links: Vec<u32> = layer.map_or(Vec::new(), |l| {
        doc.layers()[l].links().filter(|&id| id != 0).collect()
    });
    for (index, row) in want.cels.iter_mut().enumerate() {
        let palette = doc.layers()[index].mode == ColorMode::Palette;
        for cel in row.iter_mut() {
            let reached =
                layer.is_none_or(|l| l == index || (cel.0 != 0 && links.contains(&cel.0)));
            if palette && reached {
                cel.2
                    .iter_mut()
                    .filter(|p| **p == from)
                    .for_each(|p| *p = to);
            }
        }
    }
    if layer.is_none() {
        for (set, row) in doc.tilesets.iter().zip(want.tiles.iter_mut()) {
            if set.mode() == ColorMode::Palette {
                for tile in row.iter_mut() {
                    tile.1
                        .iter_mut()
                        .filter(|p| **p == from)
                        .for_each(|p| *p = to);
                }
            }
        }
    }
    want
}

fn check_replace(seed: u64) {
    let mut e = build(seed, Float::No);
    let mut dice = Dice::new(seed ^ 0xABCD);
    let len = e.doc().colors.len();
    let from = 1 + dice.below(len - 1) as u32;
    let to = dice.below(len) as u32;
    let layer = (dice.below(2) == 0).then(|| dice.below(e.doc().layers().len()));
    let before = exact(e.doc());
    let id = layer.map(|l| e.layer_id(l).expect("layer"));
    let depth = e.history.undo_depth();
    let result = e.apply(PaletteCmd::ReplaceColor {
        from,
        to,
        layer: id,
    });
    if let Some(l) = layer.filter(|&l| e.doc().layers()[l].mode != ColorMode::Palette) {
        assert!(result.is_err(), "seed {seed}: layer {l} has no ids");
        return;
    }
    result.expect("replaced");
    let after = exact(e.doc());
    let want = expected(&before, e.doc(), from, to, layer);
    assert_eq!(after.cels, want.cels, "seed {seed}: cels");
    assert_eq!(after.tiles, want.tiles, "seed {seed}: tiles");
    assert_eq!(
        after.colors, before.colors,
        "seed {seed}: no colour changes"
    );
    if layer.is_none() {
        assert_eq!(after.sharing, before.sharing, "seed {seed}: sharing");
    }
    assert!(e.history.undo_depth() <= depth + 1, "seed {seed}: one step");
    if e.history.undo_depth() == depth + 1 {
        assert!(e.undo());
        assert_eq!(exact(e.doc()), before, "seed {seed}: undo");
    }
}

fn check_brushes(seed: u64) {
    let mut e = build(seed, Float::No);
    let kind = KINDS[(seed % 3) as usize];
    e.apply(PaletteCmd::Remap(kind)).expect("remapped");
    let table = e.doc().colors.clone();
    let brushes = e.brushes();
    let all = brushes.active_custom().into_iter().chain(brushes.recent());
    for brush in all {
        let ids = brush.ids().expect("captured with ids");
        for (&id, &color) in ids.iter().zip(brush.colors()) {
            assert!(
                id == 0 || table.color(id) == color,
                "seed {seed}: brush id {id}"
            );
        }
    }
    if let (Some(active), Some(first)) = (brushes.active_custom(), brushes.recent().first()) {
        assert!(
            std::sync::Arc::ptr_eq(active, first),
            "seed {seed}: one brush"
        );
    }
}

fn run(seeds: std::ops::RangeInclusive<u64>) {
    for seed in seeds {
        for kind in KINDS {
            check_remap(seed, kind);
        }
        check_floating(seed, KINDS[(seed % 3) as usize]);
        check_replace(seed);
        check_brushes(seed);
    }
}

#[test]
fn remaps_and_replace_keep_every_frame_shared_and_undoable() {
    run(1..=QUICK);
}

#[test]
#[ignore = "2,000 documents: run with --ignored"]
fn many_random_documents() {
    run(1..=LONG);
}

/// The random documents are not trivial: across the quick run each feature the remaps must
/// carry shows up in many documents, and each remap really moves ids in many of them.
#[test]
fn the_random_documents_cover_what_the_remaps_carry() {
    let mut seen = [0usize; 8];
    for seed in 1..=QUICK {
        let mut e = build(seed, Float::No);
        let doc = e.doc();
        let table = doc.link_table();
        let holds = doc.layers().iter().any(|l| l.links().any(|id| id != 0));
        let across = table.links().any(|m| table.reaches_across(m[0].id));
        let tile_set = |mode| {
            doc.tilesets
                .iter()
                .any(|s| s.mode() == mode && !s.is_empty())
        };
        let duplicates = (1..doc.colors.len() as u32)
            .any(|id| doc.colors.id_for_existing(doc.colors.color(id)) != Some(id));
        let flags = [
            holds,
            across,
            tile_set(ColorMode::Palette),
            tile_set(ColorMode::Rgba),
            duplicates,
            doc.colors.swatches().len() < doc.colors.len() - 1,
        ];
        flags
            .iter()
            .enumerate()
            .for_each(|(i, &f)| seen[i] += usize::from(f));
        for kind in KINDS {
            let depth = e.history.undo_depth();
            e.apply(PaletteCmd::Remap(kind)).expect("remapped");
            seen[6] += usize::from(e.history.undo_depth() > depth);
            e.undo();
        }
        seen[7] += usize::from(build(seed, Float::Pending).floating.is_some());
    }
    for (what, count) in seen.iter().enumerate() {
        assert!(
            *count >= QUICK as usize / 10,
            "feature {what} in only {count}"
        );
    }
}
