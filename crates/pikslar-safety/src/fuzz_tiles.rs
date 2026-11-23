//! The tilemap targets of the fuzzer (M10): project files with tilesets, their `TSET` and terrain
//! sections, and Aseprite files with tilemaps. Valid files to damage, and what is done with a
//! file that was accepted: the rules of its maps hold, it saves and opens again to the same
//! bytes, and the small ones survive the whole exercise of commands.

#[path = "../../pikslar-format/src/ase_build.rs"]
pub(crate) mod ase_build;

use crate::deep_use::exercise;
use crate::mutate::Rng;
use pikslar_core::autotile::TerrainKind;
use pikslar_core::tileset::TilesetId;
use pikslar_core::{
    rgba, CelCmd, CelDrag, ColorMode, Document, DragKind, Editor, FrameCmd, Grid, Landing,
    PaletteCmd, TerrainCmd, TileCmd, TileSource, TileTool, Turn,
};
use pikslar_format::piks;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Names of the files under `PIKSLAR_ASE_FIXTURES` that are seeds: every one with tiles in its
/// name, and the one that carries user data on its tilesets.
const FIXTURE_HINTS: [&str; 2] = ["tile", "file-tests-props"];
/// Most fixture files taken, so a wrong folder cannot make every case slow.
const MOST_FIXTURES: usize = 32;

fn fail(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn new_set(
    e: &mut Editor,
    name: &str,
    mode: ColorMode,
    size: (u16, u16),
) -> Result<TilesetId, String> {
    let made = e
        .apply(TileCmd::NewTileset {
            name: name.into(),
            mode,
            tile_w: size.0,
            tile_h: size.1,
        })
        .map_err(fail)?;
    Ok(TilesetId::new(made.created.ok_or("no tileset id")?))
}

fn add_tiles(e: &mut Editor, set: TilesetId, count: usize, seed: u64) -> Result<(), String> {
    let mut noise = Rng::new(seed);
    let tileset = e.doc().tilesets.get(set).ok_or("no tileset")?;
    let ((w, h), mode) = (tileset.tile_size(), tileset.mode());
    let colors = e.doc().colors.len();
    for _ in 0..count {
        let cells: Vec<u32> = (0..w * h)
            .map(|i| match (mode, i % 3) {
                (_, 0) => 0,
                (ColorMode::Palette, _) => noise.below(colors) as u32,
                _ => rgba(noise.next_u64() as u8, 60, 200, 255),
            })
            .collect();
        let from = TileSource::Image(Arc::new(Grid::from_cells(w, h, &cells).into_image()));
        e.apply(TileCmd::AddTile { tileset: set, from })
            .map_err(fail)?;
    }
    Ok(())
}

/// Places tiles of the layer's set along a rectangle, turned so that some cells carry flips.
fn place(
    e: &mut Editor,
    layer: usize,
    turns: &[Turn],
    path: Vec<(i32, i32)>,
) -> Result<(), String> {
    (e.layer, e.frame) = (layer, 0);
    let set = e.doc().layers()[layer].tileset().ok_or("not a map")?;
    let tile = e.doc().tilesets.get(set).ok_or("no tileset")?.tiles()[1].id;
    e.set_current_tile(set, tile).map_err(fail)?;
    for &turn in turns {
        e.turn_tile_stamp(turn).map_err(fail)?;
    }
    let tool = TileTool::Rect;
    e.apply(TileCmd::Stroke { tool, path }).map_err(fail)?;
    Ok(())
}

/// Terrain sets of all three kinds on `set`: one from a template, one with a few slots filled,
/// one left empty.
fn terrains(e: &mut Editor, set: TilesetId) -> Result<(), String> {
    let kinds = [
        TerrainKind::Blob47,
        TerrainKind::Edge4,
        TerrainKind::Corner4,
    ];
    for (n, kind) in kinds.into_iter().enumerate() {
        let template = n == 0;
        let name = format!("terrain {n}");
        let made = e
            .apply(TerrainCmd::New {
                tileset: set,
                kind,
                name,
                template,
            })
            .map_err(fail)?;
        let id = u16::try_from(made.created.ok_or("no terrain id")?).map_err(fail)?;
        if n == 1 {
            let tiles: Vec<_> = e.doc().tilesets.get(set).ok_or("set")?.tiles().to_vec();
            for (slot, tile) in tiles.iter().take(4).enumerate() {
                let (slot, tile) = (slot as u8, tile.id);
                e.apply(TerrainCmd::Assign {
                    tileset: set,
                    set: id,
                    slot,
                    tile,
                })
                .map_err(fail)?;
            }
        }
    }
    Ok(())
}

/// Maps of a square RGBA set (two layers, linked at one frame and with a hold) and of an
/// oblong palette set, with flips, a diagonal flip on the square tiles, terrain sets on both
/// sets, a paint layer, and a third set no layer uses.
fn tiled_document() -> Result<Document, String> {
    let mut e = Editor::new(Document::new(24, 16, ColorMode::Rgba).map_err(fail)?);
    for n in 1..6u8 {
        let color = rgba(n * 40, 90, 255 - n * 30, 255);
        e.apply(PaletteCmd::AddSwatch { color }).map_err(fail)?;
    }
    let square = new_set(&mut e, "Walls", ColorMode::Rgba, (4, 4))?;
    let oblong = new_set(&mut e, "Floor", ColorMode::Palette, (4, 2))?;
    let spare = new_set(&mut e, "Spare", ColorMode::Rgba, (2, 3))?;
    add_tiles(&mut e, square, 5, 1)?;
    add_tiles(&mut e, oblong, 3, 2)?;
    add_tiles(&mut e, spare, 2, 3)?;
    for (at, set) in [(1, square), (2, square), (3, oblong)] {
        e.apply(TileCmd::NewLayer { at, tileset: set })
            .map_err(fail)?;
    }
    for at in 1..4 {
        e.apply(FrameCmd::Add { at, copy: at == 2 }).map_err(fail)?;
    }
    place(
        &mut e,
        1,
        &[Turn::Clockwise, Turn::FlipHorizontal],
        vec![(0, 0), (15, 11)],
    )?;
    place(&mut e, 2, &[Turn::FlipVertical], vec![(4, 4), (23, 15)])?;
    place(&mut e, 3, &[Turn::FlipHorizontal], vec![(0, 8), (11, 15)])?;
    let drag = CelDrag {
        layers: (1, 1),
        frames: (0, 0),
        to_layer: 2,
        to_frame: 2,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    };
    e.apply(CelCmd::Drag(drag)).map_err(fail)?;
    let layer = e.layer_id(3).ok_or("layer")?;
    let hold = FrameCmd::HoldAcross {
        layer,
        source: 0,
        from: 0,
        to: 3,
    };
    e.apply(hold).map_err(fail)?;
    terrains(&mut e, square)?;
    terrains(&mut e, oblong)?;
    Ok(e.doc().clone())
}

/// Project files with tilesets: those of the corpus, and the crafted one.
pub fn project_seeds() -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::new();
    for entry in crate::corpus::entries()? {
        let bytes = entry.bytes().map_err(|e| format!("{}: {e}", entry.name))?;
        let doc = piks::load(&bytes).map_err(|e| format!("{}: {e}", entry.name))?;
        if !doc.tilesets.is_empty() {
            out.push(bytes);
        }
    }
    let doc = tiled_document()?;
    doc.check_tilemaps()
        .map_err(|f| format!("the crafted seed: {f}"))?;
    out.push(piks::try_save(&doc).map_err(fail)?);
    for seed in &out {
        piks::load(seed).map_err(|e| format!("a tileset seed is refused: {e}"))?;
    }
    Ok(out)
}

fn fixture_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for path in entries.flatten().map(|entry| entry.path()) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                todo.push(path);
            } else if name.ends_with(".aseprite") && FIXTURE_HINTS.iter().any(|h| name.contains(h))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found.truncate(MOST_FIXTURES);
    found
}

/// Aseprite files with tilemaps: two built from the specification, and, when
/// `PIKSLAR_ASE_FIXTURES` names a folder, the real files made by Aseprite under it that import.
pub fn ase_seeds() -> Result<Vec<Vec<u8>>, String> {
    let mut out = vec![ase_build::rich_tiles(), ase_build::rich_tiles_indexed()];
    if let Some(root) = std::env::var_os("PIKSLAR_ASE_FIXTURES") {
        for path in fixture_files(Path::new(&root)) {
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            if pikslar_format::import_ase(&bytes).is_ok() {
                out.push(bytes);
            }
        }
    }
    for seed in &out {
        let (doc, _) =
            pikslar_format::import_ase(seed).map_err(|e| format!("an .ase seed: {e}"))?;
        if doc.tilesets.is_empty() {
            return Err("an .ase seed has no tileset".to_owned());
        }
    }
    Ok(out)
}

/// How many seeds came from `PIKSLAR_ASE_FIXTURES`.
pub fn fixture_count(seeds: &[Vec<u8>]) -> usize {
    seeds.len().saturating_sub(2)
}

/// What is done with an accepted document with tilesets: its maps keep their rules, it saves
/// and opens to the same bytes, and a small one survives the whole exercise.
pub fn use_tiled(doc: Document, rng: &mut Rng, deep: bool) -> Result<(), String> {
    doc.check_tilemaps().map_err(|f| format!("tilemaps: {f}"))?;
    doc.check_links().map_err(|f| format!("links: {f}"))?;
    let saved = piks::try_save(&doc).map_err(|e| format!("a save is refused: {e}"))?;
    let again = piks::load(&saved).map_err(|e| format!("its own save will not open: {e}"))?;
    again
        .check_tilemaps()
        .map_err(|f| format!("tilemaps after a save: {f}"))?;
    let twice = piks::try_save(&again).map_err(fail)?;
    if twice != saved {
        let what = crate::same::first_difference(&doc, &again);
        let at = saved.iter().zip(&twice).position(|(a, b)| a != b);
        return Err(format!(
            "a save of a document with tilesets is not stable: {} at byte {at:?} of {}",
            what.unwrap_or_else(|| "bytes only".to_owned()),
            saved.len()
        ));
    }
    match deep {
        true => exercise(&mut Editor::new(doc), rng),
        false => Ok(()),
    }
}

/// What `spikes/e2e-kill-tiles.ps1` compares with its model, for a project file: "OK|" then
/// the layer and frame counts, each layer's kind from the bottom (P paint, M map, G group, R
/// reference) and the groups of linked cels as "layer.frame" members joined by "+", sorted;
/// or "BAD|" and the first broken rule of the maps or links, or a save that does not hold.
pub fn tile_state(doc: &Document) -> String {
    use pikslar_core::LayerKind;
    if let Err(fault) = doc.check_tilemaps() {
        return format!("BAD|tilemaps: {fault}");
    }
    if let Err(fault) = doc.check_links() {
        return format!("BAD|links: {fault}");
    }
    let mut rng = Rng::new(1);
    if let Err(e) = use_tiled(doc.clone(), &mut rng, false) {
        return format!("BAD|{e}");
    }
    let kinds: String = doc
        .layers()
        .iter()
        .map(|l| match l.kind {
            LayerKind::Tilemap(_) => 'M',
            LayerKind::Group => 'G',
            LayerKind::Reference => 'R',
            _ => 'P',
        })
        .collect();
    let mut groups: std::collections::BTreeMap<u32, Vec<String>> = Default::default();
    for (li, layer) in doc.layers().iter().enumerate() {
        for (fi, link) in layer.links().enumerate() {
            if link != 0 {
                groups
                    .entry(link)
                    .or_default()
                    .push(format!("{}.{}", li + 1, fi + 1));
            }
        }
    }
    let mut text: Vec<String> = groups
        .into_values()
        .filter(|g| g.len() > 1)
        .map(|g| g.join("+"))
        .collect();
    text.sort();
    let (layers, frames) = (doc.layers().len(), doc.frames.len());
    format!("OK|L{layers}F{frames}|{kinds}|{}", text.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crafted_seed_has_every_kind_of_map_part() {
        let doc = tiled_document().expect("made");
        assert_eq!(doc.check_tilemaps(), Ok(()));
        assert_eq!(doc.check_links(), Ok(()));
        assert_eq!(doc.tilesets.len(), 3);
        let maps: Vec<_> = doc.layers().iter().filter(|l| l.is_tilemap()).collect();
        assert_eq!(maps.len(), 3);
        assert!(doc.layers()[1].link(0) != 0 && doc.layers()[1].link(0) == doc.layers()[2].link(2));
        assert!((0..4).all(|f| doc.layers()[3].cel(f).same_image(doc.layers()[3].cel(0))));
        let terrains: usize = doc.tilesets.iter().map(|s| s.terrains().len()).sum();
        assert_eq!(terrains, 6);
        let flags = |layer: usize| {
            let cel = doc.layers()[layer].cel(0);
            (0..24).any(|x| (0..16).any(|y| cel.get(x, y) >> 28 != 0))
        };
        assert!(flags(1) && flags(2) && flags(3), "flipped cells");
    }

    #[test]
    fn the_state_names_kinds_and_links_and_finds_a_broken_map() {
        let doc = tiled_document().expect("made");
        assert_eq!(tile_state(&doc), "OK|L4F4|PMMM|2.1+3.3,4.1+4.2+4.3+4.4");
        let mut broken = doc.clone();
        let cel = pikslar_core::Cel::new(Grid::from_cells(1, 1, &[99]));
        broken.put_cel(1, 1, cel).expect("fits");
        assert!(
            tile_state(&broken).starts_with("BAD|tilemaps"),
            "{}",
            tile_state(&broken)
        );
    }

    #[test]
    fn every_seed_is_used_without_fault() {
        let mut rng = Rng::new(1);
        for seed in project_seeds().expect("seeds") {
            use_tiled(piks::load(&seed).expect("opens"), &mut rng, true).expect("used");
        }
        for seed in ase_seeds().expect("seeds") {
            let (doc, _) = pikslar_format::import_ase(&seed).expect("imports");
            use_tiled(doc, &mut rng, true).expect("used");
        }
    }
}
