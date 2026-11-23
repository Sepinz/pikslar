//! The model's own reading of the terrain rules (plan section 7), written apart from
//! `pikslar-core`'s rule engine, and the checks a terrain stroke or a hand placement must pass
//! when the map before and after it are compared cell by cell.

use super::autotile_footprint::Footprint;
use pikslar_core::autotile::{TerrainKind, TerrainSet};
use pikslar_core::tilemap::MapCell;
use pikslar_core::tileset::Tileset;
use pikslar_core::Image;
use std::ops::RangeInclusive;

/// The eight neighbours, clockwise from north, with the weights of the blob convention:
/// N 1, NE 2, E 4, SE 8, S 16, SW 32, W 64, NW 128.
const AROUND: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// A corner bit is kept only when both side neighbours beside it are set.
fn reduced(mask: u8) -> u8 {
    let (n, ne, e, se, s, sw, w, nw) = (1, 2, 4, 8, 16, 32, 64, 128);
    let mut out = mask & (n | e | s | w);
    for (corner, a, b) in [(ne, n, e), (se, e, s), (sw, s, w), (nw, w, n)] {
        if mask & corner != 0 && mask & a != 0 && mask & b != 0 {
            out |= corner;
        }
    }
    out
}

/// The 47 reduced masks, ascending: Blob47's slot order.
fn blob_masks() -> Vec<u8> {
    (0..=255u8).filter(|&m| reduced(m) == m).collect()
}

/// The slot a cell wants from its neighbour mask.
pub(super) fn wanted(kind: TerrainKind, mask: u8) -> u8 {
    match kind {
        TerrainKind::Edge4 => {
            let side = |weight: u8, bit: u8| if mask & weight != 0 { bit } else { 0 };
            side(1, 1) | side(4, 2) | side(16, 4) | side(64, 8)
        }
        TerrainKind::Corner4 => {
            let corner = |weight: u8, bit: u8| if mask & weight != 0 { bit } else { 0 };
            corner(2, 1) | corner(8, 2) | corner(32, 4) | corner(128, 8)
        }
        TerrainKind::Blob47 => {
            let r = reduced(mask);
            blob_masks().iter().position(|&m| m == r).unwrap_or(0) as u8
        }
    }
}

/// The bits a slot stands for, to compare slots by.
fn slot_bits(kind: TerrainKind, slot: usize, blob: &[u8]) -> u8 {
    match kind {
        TerrainKind::Blob47 => blob.get(slot).copied().unwrap_or(0),
        _ => slot as u8,
    }
}

/// The slot used for `want`: itself if filled, else the filled slot agreeing on most bits, ties
/// to the lower; `None` when every slot is empty.
pub(super) fn chosen(set: &TerrainSet, want: u8) -> Option<usize> {
    let slots = set.slots();
    if slots.get(usize::from(want)).copied().flatten().is_some() {
        return Some(usize::from(want));
    }
    let blob = blob_masks();
    let goal = slot_bits(set.kind(), usize::from(want), &blob);
    let filled = (0..slots.len()).filter(|&s| slots[s].is_some());
    filled.min_by_key(|&s| ((slot_bits(set.kind(), s, &blob) ^ goal).count_ones(), s))
}

pub(super) fn member(set: &TerrainSet, bits: u32) -> bool {
    MapCell::from_bits(bits)
        .tile()
        .is_some_and(|t| set.holds(t))
}

fn mask_at(set: &TerrainSet, map: &Image, (x, y): (i32, i32)) -> u8 {
    let bit = |i: usize| u8::from(member(set, map.get(x + AROUND[i].0, y + AROUND[i].1))) << i;
    (0..8).fold(0, |m, i| m | bit(i))
}

/// The cells either map draws, as inclusive ranges of x and y.
fn drawn(before: &Image, after: &Image) -> Option<(RangeInclusive<i32>, RangeInclusive<i32>)> {
    let ends = |b: (i32, i32, i32, i32)| (b.0, b.1, b.0 + b.2 - 1, b.1 + b.3 - 1);
    let both = [before.drawn_bounds(), after.drawn_bounds()];
    let (x0, y0, x1, y1) = both
        .into_iter()
        .flatten()
        .map(ends)
        .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))?;
    Some((x0..=x1, y0..=y1))
}

/// Every cell that differs between two maps, in row order, with its values before and after.
pub(super) fn changed(before: &Image, after: &Image) -> Vec<((i32, i32), u32, u32)> {
    let mut out = Vec::new();
    let Some((xs, ys)) = drawn(before, after) else {
        return out;
    };
    for y in ys {
        for x in xs.clone() {
            let (a, b) = (before.get(x, y), after.get(x, y));
            if a != b {
                out.push(((x, y), a, b));
            }
        }
    }
    out
}

/// What a terrain stroke must leave, cell by cell, given what it painted (`foot`) and whether a
/// selection was active (it takes painted points away, so what needs them exact is skipped).
///
/// * Every cell the stroke changed is within its reach, never keeps or gains a pin, and is never
///   turned. One left in the set holds a tile of the set unturned; for Edge4 and Blob47 it is
///   exactly the tile its eight neighbours after the stroke ask for, for Corner4 the tile the
///   corners it ends with ask for. One that left the set, or was never in it, is empty or is
///   what it was without its pin.
/// * Every pinned cell the stroke did not itself paint is exactly as it was.
/// * Completeness: every unpinned member cell within reach holds what the rules want of it after
///   the stroke, changed or not.
pub(super) fn stroke_result(
    set: &TerrainSet,
    (before, after): (&Image, &Image),
    foot: &Footprint,
    selection: bool,
) -> Result<usize, String> {
    let cells = changed(before, after);
    pins_kept(before, after, foot)?;
    for &(c, was, now) in &cells {
        if !foot.reaches(c) {
            return Err(format!(
                "a terrain stroke changed cell {c:?} out of its reach"
            ));
        }
        if MapCell::from_bits(now).is_pinned() {
            return Err(format!(
                "a terrain stroke left cell {c:?} pinned ({now:#x})"
            ));
        }
        if !member(set, now) {
            if now != 0 && now != MapCell::from_bits(was).with_pinned(false).bits() {
                return Err(format!("cell {c:?} left the set as {now:#x}, was {was:#x}"));
            }
            continue;
        }
        if now != MapCell::from_bits(now).tile().map_or(0, |t| t.get()) {
            return Err(format!("cell {c:?} holds a turned terrain tile {now:#x}"));
        }
    }
    let touched: Vec<_> = cells.iter().map(|&(c, _, _)| c).collect();
    // A retile votes corners from the other cells at a vertex: not the stroke's corner rule.
    let exact = !selection && foot.updates_neighbours();
    holds_wanted(set, (before, after), foot, &touched, exact)?;
    if !selection {
        holds_wanted(set, (before, after), foot, &foot.reach(), exact)?;
    }
    Ok(cells.len())
}

/// Every cell that was pinned before and is not painted by the stroke is byte-identical after.
fn pins_kept(before: &Image, after: &Image, foot: &Footprint) -> Result<(), String> {
    let Some((xs, ys)) = drawn(before, after) else {
        return Ok(());
    };
    for y in ys {
        for x in xs.clone() {
            let was = before.get(x, y);
            if MapCell::from_bits(was).is_pinned()
                && !foot.painted((x, y))
                && after.get(x, y) != was
            {
                let now = after.get(x, y);
                return Err(format!(
                    "pinned cell {:?} was {was:#x} and not painted, now {now:#x}",
                    (x, y)
                ));
            }
        }
    }
    Ok(())
}

/// Every unpinned cell of `cells` holds what the rules want of it after the stroke. A Corner4
/// set is held to its corners only when `corners` (they need the exact footprint).
fn holds_wanted(
    set: &TerrainSet,
    (before, after): (&Image, &Image),
    foot: &Footprint,
    cells: &[(i32, i32)],
    corners: bool,
) -> Result<(), String> {
    for &c in cells {
        let (was, now) = (before.get(c.0, c.1), after.get(c.0, c.1));
        if MapCell::from_bits(now).is_pinned() {
            continue;
        }
        let expected = if set.kind() == TerrainKind::Corner4 {
            match corners {
                true => corner_expected(set, was, foot, c),
                false => continue,
            }
        } else if member(set, now) {
            let want = wanted(set.kind(), mask_at(set, after, c));
            let slot = chosen(set, want).ok_or("a member of a set with no tile")?;
            set.slots()[slot].map_or(0, |t| t.get())
        } else {
            continue;
        };
        if now != expected {
            return Err(format!(
                "cell {c:?} was {was:#x} and holds {now:#x} after the stroke; the rules want {expected:#x} ({:?} {:?} clear {} by {} {:?})",
                set.kind(),
                foot.shape,
                foot.clear,
                if foot.vertices { "corners" } else { "cells" },
                foot.points
            ));
        }
    }
    Ok(())
}

/// What a Corner4 cell must hold: each corner is set where the stroke painted its vertex (clear
/// where it cleared it) and as the cell's own tile had it elsewhere. No corner set leaves slot
/// 0's tile, or empty without one, for a cell that was in the set; a cell that was not stays.
fn corner_expected(set: &TerrainSet, was: u32, foot: &Footprint, (x, y): (i32, i32)) -> u32 {
    let tile = MapCell::from_bits(was).tile().map(|t| t.get());
    let own = tile.and_then(|t| {
        let mut slots = set.slots().iter();
        slots.position(|s| s.is_some_and(|s| s.get() == t))
    });
    let own_bits = own.map_or(0, |s| s as u8);
    // Corner bits NE 1, SE 2, SW 4, NW 8 and the vertex each stands at from the cell.
    let bits =
        [(1, 0, 1u8), (1, 1, 2), (0, 1, 4), (0, 0, 8)]
            .iter()
            .fold(0, |m, &(dx, dy, bit)| {
                m | match foot.vertex_painted((x + dx, y + dy)) {
                    true if foot.clear => 0,
                    true => bit,
                    false => own_bits & bit,
                }
            });
    let stays = match foot.painted((x, y)) {
        true => MapCell::from_bits(was).with_pinned(false).bits(),
        false => was,
    };
    let slot_tile = |s: usize| set.slots().get(s).copied().flatten().map_or(0, |t| t.get());
    match (bits, own) {
        (0, Some(_)) => slot_tile(0),
        (0, None) => stays,
        (want, _) => chosen(set, want).map_or(stays, slot_tile),
    }
}

/// What a hand placement must leave: every cell it filled is pinned exactly when its tile fills
/// a slot of one of the tileset's terrain sets.
pub(super) fn placement_pins(
    tileset: &Tileset,
    before: &Image,
    after: &Image,
) -> Result<usize, String> {
    let mut pinned = 0;
    for (c, _, now) in changed(before, after) {
        let cell = MapCell::from_bits(now);
        let Some(tile) = cell.tile() else {
            continue;
        };
        let terrain = tileset.terrains().iter().any(|t| t.holds(tile));
        if cell.is_pinned() != terrain {
            return Err(format!(
                "cell {c:?} placed by hand as {now:#x}: pinned {}, a terrain tile {terrain}",
                cell.is_pinned()
            ));
        }
        pinned += usize::from(terrain);
    }
    Ok(pinned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_are_47_blob_masks_from_isolated_to_full() {
        let masks = blob_masks();
        assert_eq!(masks.len(), 47);
        assert_eq!((masks[0], masks[46]), (0, 255));
    }

    #[test]
    fn the_rules_read_neighbours_as_the_plan_says() {
        // North and east alone: Edge4 N | E, Blob47 without the corner between them.
        assert_eq!(wanted(TerrainKind::Edge4, 1 | 4), 1 | 2);
        assert_eq!(wanted(TerrainKind::Edge4, 2 | 8 | 32 | 128), 0);
        assert_eq!(wanted(TerrainKind::Corner4, 2 | 128), 1 | 8);
        let lone_corner = wanted(TerrainKind::Blob47, 2);
        assert_eq!(
            lone_corner, 0,
            "a corner without its sides counts for nothing"
        );
        let with_corner = wanted(TerrainKind::Blob47, 1 | 2 | 4);
        assert_eq!(blob_masks()[usize::from(with_corner)], 7);
    }
}
