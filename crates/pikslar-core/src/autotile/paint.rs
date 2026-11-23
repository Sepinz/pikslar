//! Terrain painting: which cells a terrain stroke changes, and to what.
//!
//! A stroke is a set of painted *points*: cells, or for a Corner4 set painted by its corners,
//! vertices (vertex (`vx`, `vy`) is the top-left corner of cell (`vx`, `vy`)). The stroke first
//! sets membership (or corner states) for every point, then works out the painted cells and
//! their 8 neighbours once each, in row order. Each result reads only the map as it was and the
//! set of points, never another result, so the order in which points were painted cannot
//! change what a stroke does.
//!
//! * **Membership.** A painted cell is a member when the stroke paints and not one when it
//!   clears; any other cell is a member when its tile fills a slot of the set. Pinned cells
//!   count like any other.
//! * **Edge4 and Blob47.** A member gets the tile of the slot its eight neighbours want, through
//!   [`pick_slot`]; a cleared cell that was a member becomes empty; other cells stay as they were.
//! * **Corner4.** A corner of a cell is set when the stroke painted its vertex (or a cell sharing
//!   it), cleared when the stroke cleared it, and otherwise as the cell's own tile has it. A cell
//!   whose corners say no terrain becomes slot 0's tile, or empty without one, if it was a member;
//!   otherwise it takes the slot its corners want.
//! * **Pins.** A pinned cell is never changed as a neighbour. A painted cell loses its pin.
//! * **Missing slots.** [`pick_slot`]'s rule; with no slot defined the cell stays as it was.

use super::rules::{pick_slot, wanted_slot, RuleKind, E, N, NE, NW, S, SE, SW, W};
use super::terrain::TerrainSet;
use crate::sorting::{pair_key, sort_packed, unpair_key};
use crate::tilemap::{MapCell, PINNED};
use std::collections::HashSet;

/// A cell or a vertex, in cell coordinates.
pub(crate) type At = (i32, i32);

/// The eight neighbours with their weights in the rule engine's mask.
const AROUND: [(i32, i32, u8); 8] = [
    (0, -1, N),
    (1, -1, NE),
    (1, 0, E),
    (1, 1, SE),
    (0, 1, S),
    (-1, 1, SW),
    (-1, 0, W),
    (-1, -1, NW),
];

/// Corner4 bits of a slot, and the vertex each stands at from the cell.
const CORNERS: [(i32, i32, u8); 4] = [(1, 0, 1), (1, 1, 2), (0, 1, 4), (0, 0, 8)];

/// A terrain set made ready for one stroke: lookups that cost no allocation.
#[derive(Clone, Debug)]
pub(crate) struct Rule {
    kind: RuleKind,
    defined: u64,
    /// The cell value each slot places, 0 for an empty slot.
    values: Box<[u32]>,
    /// Every tile of the set with the lowest slot it fills, by tile id.
    members: Box<[(u32, u8)]>,
}

impl Rule {
    pub fn new(set: &TerrainSet) -> Rule {
        let values = set
            .slots()
            .iter()
            .map(|s| s.map_or(0, |t| t.get()))
            .collect();
        let mut members: Vec<(u32, u8)> = (set.slots().iter().enumerate())
            .filter_map(|(slot, tile)| Some((tile.as_ref()?.get(), slot as u8)))
            .collect();
        sort_packed(
            &mut members,
            |&(tile, slot)| (u64::from(tile) << 8) | u64::from(slot),
            |key| ((key >> 8) as u32, key as u8),
        );
        members.dedup_by_key(|m| m.0);
        Rule {
            kind: set.kind(),
            defined: set.defined(),
            values,
            members: members.into(),
        }
    }

    pub fn kind(&self) -> RuleKind {
        self.kind
    }

    /// The lowest slot the cell's tile fills, if the cell is a member.
    pub fn slot_of(&self, bits: u32) -> Option<u8> {
        let tile = MapCell::from_bits(bits).tile()?.get();
        let at = self.members.binary_search_by_key(&tile, |m| m.0).ok()?;
        Some(self.members[at].1)
    }

    /// The cell value for the slot chosen for `wanted`, if any slot is defined.
    fn place(&self, wanted: u8) -> Option<u32> {
        let slot = pick_slot(self.kind, wanted, self.defined)?;
        self.values.get(usize::from(slot)).copied()
    }

    /// What a Corner4 cell with no corner set becomes: slot 0's tile, or empty without one.
    /// Never the nearest slot, which would put terrain where none is wanted.
    fn no_terrain(&self) -> u32 {
        match self.defined & 1 {
            1 => self.values.first().copied().unwrap_or(0),
            _ => 0,
        }
    }
}

/// What a stroke painted: points, and whether it paints or clears them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Paint {
    pub clear: bool,
    /// The points are vertices (a Corner4 set painted by its corners), not cells.
    pub vertices: bool,
    pub points: HashSet<At>,
}

impl Paint {
    /// True when the stroke painted this cell itself.
    fn painted(&self, c: At) -> bool {
        !self.vertices && self.points.contains(&c)
    }

    /// True when the stroke set this vertex, directly or through a cell sharing it.
    fn vertex_painted(&self, (vx, vy): At) -> bool {
        if self.vertices {
            return self.points.contains(&(vx, vy));
        }
        let around = [(vx - 1, vy - 1), (vx, vy - 1), (vx - 1, vy), (vx, vy)];
        around.iter().any(|c| self.points.contains(c))
    }

    /// The cells whose result the stroke may change, in row order, each once.
    pub fn affected(&self) -> Vec<At> {
        let (low, high) = if self.vertices { (-1, 0) } else { (-1, 1) };
        let mut cells = Vec::with_capacity(self.points.len() * 9);
        for &(x, y) in &self.points {
            for dy in low..=high {
                for dx in low..=high {
                    cells.push((x + dx, y + dy));
                }
            }
        }
        sort_packed(
            &mut cells,
            |&(x, y)| pair_key(y, x),
            |key| {
                let (y, x) = unpair_key(key);
                (x, y)
            },
        );
        cells.dedup();
        cells
    }
}

/// The map as it was, for reading cells.
pub(crate) trait Cells {
    fn cell(&self, c: At) -> u32;
}

impl<F: Fn(At) -> u32> Cells for F {
    fn cell(&self, c: At) -> u32 {
        self(c)
    }
}

/// True when a cell is in the set after the stroke's membership step.
fn member(rule: &Rule, paint: &Paint, map: &impl Cells, c: At) -> bool {
    if paint.painted(c) {
        !paint.clear
    } else {
        rule.slot_of(map.cell(c)).is_some()
    }
}

/// The eight-neighbour mask of a cell after the membership step.
fn neighbours(rule: &Rule, paint: &Paint, map: &impl Cells, (x, y): At) -> u8 {
    let set =
        |&(dx, dy, bit): &(i32, i32, u8)| member(rule, paint, map, (x + dx, y + dy)) as u8 * bit;
    AROUND.iter().map(set).fold(0, |m, b| m | b)
}

/// The Corner4 corners of a cell after the stroke.
fn corners(paint: &Paint, own: u8, (x, y): At) -> u8 {
    let corner = |&(dx, dy, bit): &(i32, i32, u8)| match paint.vertex_painted((x + dx, y + dy)) {
        true if paint.clear => 0,
        true => bit,
        false => own & bit,
    };
    CORNERS.iter().map(corner).fold(0, |m, b| m | b)
}

/// What one cell becomes after the stroke, `None` to leave it as it was.
pub(crate) fn result(rule: &Rule, paint: &Paint, map: &impl Cells, c: At) -> Option<u32> {
    let current = map.cell(c);
    let painted = paint.painted(c);
    if !painted && MapCell::from_bits(current).is_pinned() {
        return None;
    }
    let was = rule.slot_of(current);
    let out = match rule.kind {
        RuleKind::Corner4 => match corners(paint, was.unwrap_or(0), c) {
            0 => was.map(|_| rule.no_terrain()),
            want => rule.place(want),
        },
        kind if member(rule, paint, map, c) => {
            rule.place(wanted_slot(kind, neighbours(rule, paint, map, c)))
        }
        _ => (painted && was.is_some()).then_some(0),
    };
    match (out, painted) {
        (None, true) => Some(current & !PINNED),
        (out, _) => out,
    }
}

/// Every cell the stroke changes, in row order, with the value it will hold. Only cells `may`
/// allows are changed.
pub(crate) fn changes(
    rule: &Rule,
    paint: &Paint,
    map: &impl Cells,
    may: impl Fn(At) -> bool,
) -> Vec<(At, u32)> {
    let mut out = Vec::new();
    for c in paint.affected() {
        if !may(c) {
            continue;
        }
        if let Some(value) = result(rule, paint, map, c) {
            if value != map.cell(c) {
                out.push((c, value));
            }
        }
    }
    out
}

/// What re-autotiling one cell makes of it: a member takes the tile its neighbours want and
/// loses its pin; for Corner4 each corner takes what most of the three other cells at its
/// vertex say. `None` leaves the cell as it was.
pub(crate) fn retile(rule: &Rule, map: &impl Cells, (x, y): At) -> Option<u32> {
    let current = map.cell((x, y));
    rule.slot_of(current)?;
    let out = match rule.kind {
        RuleKind::Corner4 => {
            let vote = |&(dx, dy, bit): &(i32, i32, u8)| {
                let claims = vertex_claims(rule, map, (x + dx, y + dy), (x, y));
                if claims >= 2 {
                    bit
                } else {
                    0
                }
            };
            match CORNERS.iter().map(vote).fold(0, |m, b| m | b) {
                0 => Some(rule.no_terrain()),
                want => rule.place(want),
            }
        }
        kind => {
            let mask = neighbours(rule, &Paint::default(), map, (x, y));
            rule.place(wanted_slot(kind, mask))
        }
    };
    Some(out.unwrap_or(current) & !PINNED)
}

/// How many of the cells at a vertex, other than `except`, have their corner there set.
fn vertex_claims(rule: &Rule, map: &impl Cells, (vx, vy): At, except: At) -> u32 {
    // The cell and which of its corners the vertex is.
    let around = [
        ((vx - 1, vy - 1), 2u8),
        ((vx, vy - 1), 4),
        ((vx - 1, vy), 1),
        ((vx, vy), 8),
    ];
    let claims = around.iter().filter(|(c, _)| *c != except);
    let set = claims.filter(|(c, bit)| rule.slot_of(map.cell(*c)).is_some_and(|s| s & bit != 0));
    set.count() as u32
}

#[cfg(test)]
#[path = "paint_tests.rs"]
mod tests;
