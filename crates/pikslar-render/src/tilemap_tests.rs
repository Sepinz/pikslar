//! Tilemap layers on the screen and in every export: a document with maps must come out exactly
//! as the same document does with each map replaced by the pixels its cells draw, worked out by
//! the brute-force oracle. Also the redraw after a tile edit, its undo and its redo.

use crate::tilemap_oracle::{placed_tiles, random_scene, Rng, Scene};
use crate::view::Rect;
use crate::{flatten_document, BandSource, Compositor};
use pikslar_core::{rgba, Blending, Damage, Document, Editor, Grid, StepCmd, TileCmd, TILE};
use std::sync::Arc;

const SEEDS: std::ops::RangeInclusive<u64> = 1..=150;

fn screen(doc: &Document, frame: usize, onion: bool) -> Vec<u32> {
    let mut e = Editor::new(doc.clone());
    e.frame = frame;
    let mut comp = Compositor::new(doc.width, doc.height);
    comp.onion = onion;
    comp.onion_depth = 2;
    comp.update(&e, Damage::All, &mut Vec::new());
    comp.pixels().to_vec()
}

/// The scene of `seed`, both documents in `blending`.
fn scene_in(seed: u64, blending: Blending) -> Scene {
    let mut scene = random_scene(seed);
    (scene.doc.blending, scene.truth.blending) = (blending, blending);
    scene
}

const BLENDINGS: [Blending; 2] = [Blending::Pikslar, Blending::Aseprite];

fn bands(source: &BandSource<'_>, frame: usize) -> Vec<u32> {
    let mut all = Vec::new();
    let mut band = Vec::new();
    for index in 0..source.bands() {
        source.band(frame, index, &mut band);
        all.extend_from_slice(&band);
    }
    all
}

#[test]
fn the_compositor_with_and_without_onion_skin_draws_maps_as_the_oracle_says() {
    for (seed, blending) in SEEDS.flat_map(|s| BLENDINGS.map(|b| (s, b))) {
        let Scene { doc, truth, .. } = scene_in(seed, blending);
        assert_eq!(doc.check_tilemaps(), Ok(()), "seed {seed}");
        for frame in 0..doc.frames.len() {
            for onion in [false, true] {
                let what = format!("seed {seed} frame {frame} onion {onion} {blending:?}");
                assert!(
                    screen(&doc, frame, onion) == screen(&truth, frame, onion),
                    "{what}"
                );
            }
        }
    }
}

#[test]
fn every_export_path_draws_maps_as_the_oracle_says() {
    for (seed, blending) in SEEDS.flat_map(|s| BLENDINGS.map(|b| (s, b))) {
        let Scene { doc, truth, .. } = scene_in(seed, blending);
        let mut rng = Rng(seed ^ 0x5151);
        let (w, h) = (doc.width, doc.height);
        let mut comp = Compositor::new(1, 1);
        for frame in 0..doc.frames.len() {
            let want = flatten_document(&truth, frame, &mut comp);
            let what = format!("seed {seed} frame {frame} {blending:?}");
            assert!(
                flatten_document(&doc, frame, &mut comp) == want,
                "flatten {what}"
            );
            assert!(bands(&BandSource::new(&doc), frame) == want, "bands {what}");
            let chosen: Vec<usize> = (0..doc.layers().len()).filter(|_| rng.chance(2)).collect();
            let (a, b) = (
                BandSource::with_layers(&doc, &chosen),
                BandSource::with_layers(&truth, &chosen),
            );
            assert!(
                bands(&a, frame) == bands(&b, frame),
                "chosen layers {chosen:?} {what}"
            );
            // A region of the canvas as the screen's true-size view asks for it.
            let (x, y) = (
                rng.below(w as u32 - 1) as usize,
                rng.below(h as u32 - 1) as usize,
            );
            let region = (
                x,
                y,
                1 + rng.below((w - x) as u32) as usize,
                1 + rng.below((h - y) as u32) as usize,
            );
            let (mut plain, mut plain_truth) = (Vec::new(), Vec::new());
            let (mut e, mut t) = (Editor::new(doc.clone()), Editor::new(truth.clone()));
            (e.frame, t.frame) = (frame, frame);
            Compositor::new(w, h).compose_plain(&e, region, &mut plain);
            Compositor::new(w, h).compose_plain(&t, region, &mut plain_truth);
            assert!(plain == plain_truth, "region {region:?} {what}");
            // The true-size view shows what the canvas shows.
            let shown = screen(&doc, frame, false);
            let (x0, y0, rw, rh) = region;
            let cut: Vec<u32> = (y0..y0 + rh)
                .flat_map(|y| shown[y * w + x0..][..rw].to_vec())
                .collect();
            assert!(plain == cut, "true size of {region:?} {what}");
        }
    }
}

/// A changed copy of the tile, which the edit gives every placement.
fn edited(scene: &Scene, set: usize, n: usize) -> Arc<pikslar_core::Image> {
    let spec = &scene.tilesets[set];
    let (w, h) = spec.size;
    let mut pixels = spec.tiles[n].clone();
    for p in pixels.iter_mut() {
        *p = match spec.mode {
            // The next of the five colours: never the one it was.
            pikslar_core::ColorMode::Palette => *p % 5 + 1,
            pikslar_core::ColorMode::Rgba => p.wrapping_add(rgba(1, 1, 1, 1)),
        };
    }
    Arc::new(Grid::from_cells(w, h, &pixels).into_image())
}

fn composed(e: &Editor, onion: bool) -> Compositor {
    let mut comp = Compositor::new(e.doc().width, e.doc().height);
    comp.onion = onion;
    comp.onion_depth = 2;
    comp.update(e, Damage::All, &mut Vec::new());
    comp
}

/// Edits one tile, then undoes and redoes that, each time bringing the cached picture up to date
/// from the damage alone and checking it against a picture made from scratch.
fn edit_undo_redo(scene: &Scene, onion: bool, frame: usize) -> Vec<Vec<Rect>> {
    let mut e = Editor::new(scene.doc.clone());
    e.frame = frame;
    let mut comp = composed(&e, onion);
    e.take_damage();
    let (set, n) = (0, 0);
    let command = TileCmd::SetTileImage {
        tileset: scene.tilesets[set].id,
        tile: pikslar_core::tileset::TileId::new(n as u32 + 1).expect("an id"),
        image: edited(scene, set, n),
    };
    let mut rects = Vec::new();
    let mut step = |e: &mut Editor, comp: &mut Compositor, what: &str| {
        let mut changed = Vec::new();
        let damage = e.take_damage();
        comp.update(e, damage, &mut changed);
        assert!(comp.pixels() == composed(e, onion).pixels(), "{what}");
        rects.push(changed);
    };
    e.apply(command).expect("fits");
    step(&mut e, &mut comp, "after the edit");
    e.apply(StepCmd::Undo).expect("undo");
    step(&mut e, &mut comp, "after undo");
    e.apply(StepCmd::Redo).expect("redo");
    step(&mut e, &mut comp, "after redo");
    rects
}

#[test]
fn a_tile_edit_its_undo_and_its_redo_redraw_exactly_what_changed() {
    for seed in SEEDS {
        let scene = random_scene(seed);
        if scene.tilesets.is_empty() {
            continue;
        }
        for frame in 0..scene.doc.frames.len() {
            for onion in [false, true] {
                edit_undo_redo(&scene, onion, frame);
            }
        }
    }
}

#[test]
fn a_tile_edit_redraws_only_the_canvas_tiles_that_place_the_tile() {
    for seed in SEEDS {
        let scene = random_scene(seed);
        let Some(set) = scene.tilesets.first() else {
            continue;
        };
        let doc = &scene.doc;
        let shown = doc.shown_layers();
        for frame in 0..doc.frames.len() {
            let mut want = std::collections::BTreeSet::new();
            for map in scene.maps.iter().filter(|m| m.tileset == 0 && shown[m.at]) {
                let tiles = placed_tiles(&map.frames[frame], set, &[1], (doc.width, doc.height));
                want.extend(tiles);
            }
            let rects = edit_undo_redo(&scene, false, frame);
            let across = doc.width.div_ceil(TILE);
            for (step, changed) in rects.iter().enumerate() {
                let got: std::collections::BTreeSet<usize> = changed
                    .iter()
                    .map(|r| r.y as usize / TILE * across + r.x as usize / TILE)
                    .collect();
                assert_eq!(got, want, "seed {seed} frame {frame} step {step}");
                assert_eq!(got.len(), changed.len(), "each canvas tile once");
            }
        }
    }
}

/// The scenes are worth comparing: maps are seen on screen, in both colour modes, under groups,
/// with holds and with tile edits that change what is shown.
#[test]
fn the_random_scenes_exercise_what_they_claim_to() {
    let (mut seen, mut palette, mut grouped, mut held, mut edited_on_screen) = (0, 0, 0, 0, 0);
    let mut total = 0;
    for seed in SEEDS {
        let scene = random_scene(seed);
        total += 1;
        let hidden = {
            let mut doc = scene.doc.clone();
            for map in &scene.maps {
                doc.layers_mut()[map.at].visible = false;
            }
            doc
        };
        seen += usize::from(screen(&scene.doc, 0, false) != screen(&hidden, 0, false));
        palette += usize::from(
            scene
                .tilesets
                .iter()
                .any(|s| s.mode == pikslar_core::ColorMode::Palette),
        );
        grouped += usize::from(scene.doc.layers().iter().any(|l| l.depth > 0));
        held += usize::from(
            scene
                .maps
                .iter()
                .any(|m| m.frames.windows(2).any(|w| w[0].key == w[1].key)),
        );
        if !scene.tilesets.is_empty() {
            let rects = edit_undo_redo(&scene, false, 0);
            edited_on_screen += usize::from(rects.iter().any(|r| !r.is_empty()));
        }
    }
    assert!(seen > total / 2, "maps seen on screen in {seen} of {total}");
    assert!(
        palette > total / 4 && grouped > total / 4 && held > total / 10,
        "{palette} {grouped} {held}"
    );
    assert!(
        edited_on_screen > total / 4,
        "tile edits drew something in {edited_on_screen}"
    );
}

#[test]
fn merging_a_tilemap_layer_down_draws_what_merging_its_pixels_draws() {
    let (mut merged_any, mut refused_alike) = (0, 0);
    for seed in SEEDS {
        let Scene {
            doc, truth, maps, ..
        } = random_scene(seed);
        let pair = maps.iter().map(|m| m.at).find(|&at| {
            let (below, above) = (&doc.layers()[at - 1], &doc.layers()[at]);
            below.kind == pikslar_core::LayerKind::Paint && below.depth == above.depth
        });
        let Some(at) = pair else { continue };
        let (mut e, mut t) = (Editor::new(doc.clone()), Editor::new(truth.clone()));
        let merge = |e: &mut Editor| {
            let layer = e.layer_id(at).expect("layer");
            e.apply(pikslar_core::LayerCmd::MergeDown { layer })
        };
        let (got, want) = (merge(&mut e), merge(&mut t));
        assert_eq!(got.is_ok(), want.is_ok(), "seed {seed}: {got:?} {want:?}");
        let Ok(_) = got else {
            refused_alike += usize::from(got == want);
            continue;
        };
        merged_any += 1;
        let mut comp = Compositor::new(1, 1);
        for frame in 0..doc.frames.len() {
            let a = flatten_document(e.doc(), frame, &mut comp);
            assert!(
                a == flatten_document(t.doc(), frame, &mut comp),
                "seed {seed} frame {frame}"
            );
        }
        assert_eq!(e.history().undo_depth(), 1, "one step");
        e.apply(StepCmd::Undo).expect("undo");
        assert_eq!(e.doc(), &doc, "seed {seed}: undo restores the layers");
    }
    assert!(
        merged_any > 20,
        "{merged_any} merges, {refused_alike} refused alike"
    );
}
