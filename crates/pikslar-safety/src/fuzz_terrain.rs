//! The terrain target of the fuzzer (M11): project files whose `Atil` section is damaged. The
//! seeds are autotiled maps of the three kinds, with pins, sets with missing slots, a tile in two
//! slots, removed sets that leave gaps in the ids, and two tilesets. Most damage is put inside
//! `Atil` itself, the section's length kept right and the file's checksum resealed, so that the
//! terrain reader, not the file's outer checks, is what is tested. A file that opens keeps the
//! rules of its maps, saves stably, and takes terrain strokes of every shape with each of its
//! sets, all of which undo back to the document it opened as.

use crate::craft;
use crate::mutate::{mutate, Rng};
use pikslar_core::autotile::TerrainKind;
use pikslar_core::command::{Corners, TerrainShape, TerrainTool};
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{rgba, ColorMode, Document, Editor, Grid, TerrainCmd, TileCmd, TileSource};
use pikslar_core::{StepCmd, TileTool};
use pikslar_format::piks;
use std::sync::Arc;

const TAG: &[u8; 4] = b"Atil";

fn fail(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// A tileset of `count` tiles that differ, and a tilemap layer of it made active.
fn tileset_with_map(
    e: &mut Editor,
    name: &str,
    side: u16,
    count: u32,
) -> Result<TilesetId, String> {
    let at = e.doc().layers().len();
    let made = e
        .apply(TileCmd::NewMapLayer {
            at,
            name: name.into(),
            mode: ColorMode::Rgba,
            tile_w: side,
            tile_h: side,
        })
        .map_err(fail)?;
    let set = TilesetId::new(made.created.ok_or("no tileset")?);
    let s = usize::from(side);
    for n in 0..count {
        let px: Vec<u32> = (0..s * s)
            .map(|i| rgba((n * 5) as u8, (i * 9) as u8, 200, 255))
            .collect();
        let from = TileSource::Image(Arc::new(Grid::from_cells(s, s, &px).into_image()));
        e.apply(TileCmd::AddTile { tileset: set, from })
            .map_err(fail)?;
    }
    Ok(set)
}

fn new_set(
    e: &mut Editor,
    tileset: TilesetId,
    kind: TerrainKind,
    template: bool,
) -> Result<u16, String> {
    let name = format!("{kind:?}");
    let made = e
        .apply(TerrainCmd::New {
            tileset,
            kind,
            name,
            template,
        })
        .map_err(fail)?;
    u16::try_from(made.created.ok_or("no terrain id")?).map_err(fail)
}

fn tool(
    tileset: TilesetId,
    set: u16,
    shape: TerrainShape,
    clear: bool,
    corners: Corners,
) -> TileTool {
    TileTool::Terrain(TerrainTool {
        tileset,
        set,
        shape,
        clear,
        corners,
    })
}

/// Strokes through a pattern of points on a 24 pixel canvas.
fn paint(e: &mut Editor, tool: TileTool, path: Vec<(i32, i32)>) -> Result<(), String> {
    e.apply(TileCmd::Stroke { tool, path }).map_err(fail)?;
    Ok(())
}

/// Edge4 and Blob47 sets from templates on a square tileset, painted, cleared and pinned.
fn square_seed() -> Result<Document, String> {
    let mut e = Editor::new(Document::new(24, 24, ColorMode::Rgba).map_err(fail)?);
    let ts = tileset_with_map(&mut e, "Ground", 3, 2)?;
    let edge = new_set(&mut e, ts, TerrainKind::Edge4, true)?;
    let blob = new_set(&mut e, ts, TerrainKind::Blob47, true)?;
    let c = Corners::Cell;
    paint(
        &mut e,
        tool(ts, edge, TerrainShape::Rect, false, c),
        vec![(0, 0), (11, 8)],
    )?;
    paint(
        &mut e,
        tool(ts, blob, TerrainShape::Brush, false, c),
        vec![(13, 13), (23, 13), (23, 23)],
    )?;
    paint(
        &mut e,
        tool(ts, edge, TerrainShape::Brush, true, c),
        vec![(4, 4)],
    )?;
    // A hand-placed terrain tile is pinned; a plain one is not.
    let tiles: Vec<TileId> = e
        .doc()
        .tileset(ts)
        .map_err(fail)?
        .tiles()
        .iter()
        .map(|t| t.id)
        .collect();
    e.set_current_tile(ts, tiles[3]).map_err(fail)?;
    paint(&mut e, TileTool::Place, vec![(1, 20), (7, 20)])?;
    e.set_current_tile(ts, tiles[0]).map_err(fail)?;
    paint(&mut e, TileTool::Place, vec![(10, 20)])?;
    Ok(e.doc().clone())
}

/// A Corner4 set painted by corners on its own tileset, one with missing slots and a tile in
/// two of them, an empty set, and a removed set leaving a gap in the ids.
fn mixed_seed() -> Result<Document, String> {
    let mut e = Editor::new(Document::new(24, 16, ColorMode::Rgba).map_err(fail)?);
    let a = tileset_with_map(&mut e, "Corners", 4, 0)?;
    let corner = new_set(&mut e, a, TerrainKind::Corner4, true)?;
    let v = Corners::Vertex;
    paint(
        &mut e,
        tool(a, corner, TerrainShape::Brush, false, v),
        vec![(4, 4), (16, 8)],
    )?;
    paint(
        &mut e,
        tool(a, corner, TerrainShape::Rect, true, v),
        vec![(8, 4), (12, 8)],
    )?;
    let b = tileset_with_map(&mut e, "Patchy", 4, 6)?;
    let gone = new_set(&mut e, b, TerrainKind::Blob47, false)?;
    let patchy = new_set(&mut e, b, TerrainKind::Edge4, false)?;
    new_set(&mut e, b, TerrainKind::Blob47, false)?;
    e.apply(TerrainCmd::Remove {
        tileset: b,
        set: gone,
    })
    .map_err(fail)?;
    let tiles: Vec<TileId> = e
        .doc()
        .tileset(b)
        .map_err(fail)?
        .tiles()
        .iter()
        .map(|t| t.id)
        .collect();
    for (slot, tile) in [(0u8, 0usize), (5, 1), (10, 2), (15, 2), (3, 4)] {
        let (tileset, set, tile) = (b, patchy, tiles[tile]);
        e.apply(TerrainCmd::Assign {
            tileset,
            set,
            slot,
            tile,
        })
        .map_err(fail)?;
    }
    paint(
        &mut e,
        tool(b, patchy, TerrainShape::Fill, false, Corners::Cell),
        vec![(2, 2)],
    )?;
    paint(
        &mut e,
        tool(b, patchy, TerrainShape::Brush, true, Corners::Cell),
        vec![(6, 6), (14, 6)],
    )?;
    Ok(e.doc().clone())
}

/// The seeds, each checked to open with its terrain sets.
pub fn seeds() -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::new();
    for doc in [square_seed()?, mixed_seed()?] {
        doc.check_tilemaps()
            .map_err(|f| format!("a terrain seed: {f}"))?;
        let bytes = piks::try_save(&doc).map_err(fail)?;
        let back = piks::load(&bytes).map_err(|e| format!("a terrain seed is refused: {e}"))?;
        if terrain_sets(&back) == 0 || section(&bytes).is_none() {
            return Err("a terrain seed lost its terrain sets".to_owned());
        }
        out.push(bytes);
    }
    Ok(out)
}

/// Where the `Atil` section's data is in a project file: the place of its length field and
/// the data's range.
fn section(bytes: &[u8]) -> Option<(usize, std::ops::Range<usize>)> {
    let mut from = 0;
    while let Some(at) = bytes[from..].windows(4).position(|w| w == TAG) {
        let head = from + at + 4;
        let len = u32::from_le_bytes(bytes.get(head..head + 4)?.try_into().ok()?) as usize;
        let data = head + 4..head + 4 + len;
        if data.end + 4 <= bytes.len() {
            return Some((head, data));
        }
        from += at + 1;
    }
    None
}

/// A damaged copy of a seed: nine times in ten the damage is inside `Atil`, its length field
/// set to the new length; the checksum is resealed nearly always.
pub fn damaged(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let bytes = match section(seed).filter(|_| !rng.chance(10)) {
        Some((head, data)) => {
            let inner = mutate(rng, &seed[data.clone()]);
            let mut out = seed[..head].to_vec();
            out.extend_from_slice(&(inner.len() as u32).to_le_bytes());
            out.extend_from_slice(&inner);
            out.extend_from_slice(&seed[data.end..]);
            out
        }
        None => mutate(rng, seed),
    };
    match rng.chance(20) {
        true => bytes,
        false => craft::resealed(bytes),
    }
}

/// How many terrain sets a document has, over all its tilesets.
pub fn terrain_sets(doc: &Document) -> usize {
    doc.tilesets.iter().map(|s| s.terrains().len()).sum()
}

/// Terrain strokes of every shape with each set, on the first map layer of its tileset, then
/// every step undone: the maps keep their rules after each, and the end is the start.
pub fn use_terrain(doc: Document, rng: &mut Rng) -> Result<(), String> {
    let start = doc.clone();
    let mut e = Editor::new(doc);
    let sets: Vec<(TilesetId, u16)> = e
        .doc()
        .tilesets
        .iter()
        .flat_map(|s| s.terrains().iter().map(move |t| (s.id(), t.id())))
        .collect();
    for (tileset, set) in sets.into_iter().take(8) {
        let Some(layer) = e
            .doc()
            .layers()
            .iter()
            .position(|l| l.tileset() == Some(tileset))
        else {
            continue;
        };
        e.layer = layer;
        for shape in [
            TerrainShape::Brush,
            TerrainShape::Rect,
            TerrainShape::Fill,
            TerrainShape::Retile,
        ] {
            let corners = [Corners::Cell, Corners::Vertex][rng.below(2)];
            let t = tool(tileset, set, shape, rng.chance(3), corners);
            let path = (0..1 + rng.below(3))
                .map(|_| (rng.between(-2, 30), rng.between(-2, 30)))
                .collect();
            let _ = e.apply(TileCmd::Stroke { tool: t, path });
            e.doc()
                .check_tilemaps()
                .map_err(|f| format!("after a terrain stroke: {f}"))?;
        }
    }
    while e.apply(StepCmd::Undo).map_err(fail)?.changed {}
    match crate::same::first_difference(&start, e.doc()) {
        Some(what) => Err(format!("terrain strokes undone left a change: {what}")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seeds_hold_terrain_of_every_kind_with_pins_and_gaps() {
        let docs: Vec<Document> = seeds()
            .expect("seeds")
            .iter()
            .map(|b| piks::load(b).expect("opens"))
            .collect();
        let kinds: Vec<TerrainKind> = docs
            .iter()
            .flat_map(|d| {
                d.tilesets
                    .iter()
                    .flat_map(|s| s.terrains().iter().map(|t| t.kind()))
            })
            .collect();
        for kind in [
            TerrainKind::Edge4,
            TerrainKind::Corner4,
            TerrainKind::Blob47,
        ] {
            assert!(kinds.contains(&kind), "{kind:?}");
        }
        let pinned = docs[0].layers()[1].cel(0).get(0, 6);
        assert!(
            pikslar_core::tilemap::MapCell::from_bits(pinned).is_pinned(),
            "{pinned:#x}"
        );
        // The patchy tileset's first set was removed, leaving ids 2 and 3.
        let patchy = docs[1].tilesets.iter().last().expect("two tilesets");
        let ids: Vec<u16> = patchy.terrains().iter().map(|t| t.id()).collect();
        assert_eq!(ids, [2, 3]);
        let missing = patchy.terrains()[0]
            .slots()
            .iter()
            .filter(|s| s.is_none())
            .count();
        assert!(missing > 0, "a set with missing slots");
    }

    #[test]
    fn damage_lands_in_the_section_and_keeps_its_length_right() {
        let seed = &seeds().expect("seeds")[0];
        let (head, data) = section(seed).expect("a section");
        let mut rng = Rng::new(4);
        let mut inside = 0;
        for _ in 0..200 {
            let out = damaged(&mut rng, seed);
            if out.get(..head) == Some(&seed[..head]) {
                inside += 1;
                if let Some((_, got)) = section(&out) {
                    assert!(got.end <= out.len());
                }
            }
        }
        assert!(inside > 150, "{inside}");
        assert!(data.len() > 100);
    }

    #[test]
    fn every_seed_takes_terrain_strokes_and_undoes_them() {
        let mut rng = Rng::new(1);
        for seed in seeds().expect("seeds") {
            use_terrain(piks::load(&seed).expect("opens"), &mut rng).expect("used");
        }
    }
}
