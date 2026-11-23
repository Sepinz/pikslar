//! The lossless remap: which id each colour has afterwards, and the table that goes with it.
//!
//! Three explicit commands, each keeping every composited frame identical. They only decide the
//! new numbering here; `rewrite` carries it to every image that holds palette ids.

use crate::color::ColorTable;
use std::collections::{HashMap, HashSet};

/// Which of the three lossless remaps to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemapKind {
    /// The swatches take ids 1 to n in the order shown; the other colours follow in their old
    /// order. Indexed export and engines that use palette indices need this.
    ToSwatchOrder,
    /// Pixels (and swatches and groups) move to the lowest id with the same colour value. The
    /// emptied ids stay in the table until Remove unused. Clear ids are a value like any other:
    /// they merge among themselves, never into id 0.
    MergeDuplicates,
    /// Ids that no pixel, swatch or group uses go, and the rest close up in order.
    RemoveUnused,
}

/// Where each old id goes. Id 0 stays 0. An id past the end is not in the table, so it is left
/// as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdMap {
    to: Vec<u32>,
}

impl IdMap {
    /// The map that moves nothing but the pixels of id `from`, which take id `to`, for a table
    /// of `len` colours.
    pub(crate) fn replacing(len: usize, from: u32, to: u32) -> Self {
        let mut ids: Vec<u32> = (0..len as u32).collect();
        if let Some(slot) = ids.get_mut(from as usize) {
            *slot = to;
        }
        IdMap { to: ids }
    }

    pub(crate) fn from_vec(to: Vec<u32>) -> Self {
        IdMap { to }
    }

    pub(crate) fn as_slice(&self) -> &[u32] {
        &self.to
    }

    /// The id that `id` becomes.
    pub fn get(&self, id: u32) -> u32 {
        self.to.get(id as usize).copied().unwrap_or(id)
    }

    /// True if no id moves.
    pub fn is_identity(&self) -> bool {
        self.to.iter().enumerate().all(|(id, &to)| id as u32 == to)
    }
}

/// What a remap does: the id map for the images, and the table that replaces the old one.
pub(crate) struct Plan {
    pub map: IdMap,
    pub table: ColorTable,
}

/// Plans `kind` over `table`. `used` says which ids a pixel shows, and is only asked for by
/// Remove unused.
pub(crate) fn plan(kind: RemapKind, table: &ColorTable, used: impl FnOnce() -> Vec<bool>) -> Plan {
    let len = table.len();
    let (keep, to) = match kind {
        RemapKind::ToSwatchOrder => swatch_order(table),
        RemapKind::MergeDuplicates => merged(table),
        RemapKind::RemoveUnused => closed_up(table, &used()),
    };
    Plan {
        table: table.remapped(&keep, &to),
        map: IdMap::from_vec(pad(to, len)),
    }
}

fn pad(mut to: Vec<u32>, len: usize) -> Vec<u32> {
    to.resize(len, 0);
    to
}

/// The old ids in their new order, and where each old id goes.
type Moves = (Vec<u32>, Vec<u32>);

fn swatch_order(table: &ColorTable) -> Moves {
    let len = table.len();
    let mut seen = HashSet::new();
    let first = table
        .swatches()
        .iter()
        .copied()
        .filter(|&id| id != 0 && (id as usize) < len && seen.insert(id));
    let mut keep: Vec<u32> = first.collect();
    keep.extend((1..len as u32).filter(|id| !seen.contains(id)));
    (keep.clone(), destinations(len, &keep))
}

fn merged(table: &ColorTable) -> Moves {
    let len = table.len();
    let mut lowest: HashMap<u32, u32> = HashMap::new();
    let mut to = vec![0u32; len];
    for id in 1..len as u32 {
        // A clear colour merges only with other clear ids, never into id 0: a swatch made clear
        // stays a swatch, and its pixels come back when it is given a colour again.
        to[id as usize] = *lowest.entry(table.color(id)).or_insert(id);
    }
    ((1..len as u32).collect(), to)
}

fn closed_up(table: &ColorTable, used: &[bool]) -> Moves {
    let len = table.len();
    let mut wanted = vec![false; len];
    let in_swatches = table.swatches().iter();
    let in_groups = table.groups().iter().flat_map(|g| g.ids.iter());
    for &id in in_swatches.chain(in_groups) {
        if let Some(slot) = wanted.get_mut(id as usize) {
            *slot = true;
        }
    }
    let keep: Vec<u32> = (1..len as u32)
        .filter(|&id| wanted[id as usize] || used.get(id as usize).copied().unwrap_or(false))
        .collect();
    (keep.clone(), destinations(len, &keep))
}

/// Where each old id goes when the colours `keep` take ids 1, 2, 3 and so on. An id not kept
/// goes to 0, which nothing refers to.
fn destinations(len: usize, keep: &[u32]) -> Vec<u32> {
    let mut to = vec![0u32; len];
    for (at, &old) in keep.iter().enumerate() {
        to[old as usize] = at as u32 + 1;
    }
    to
}

#[cfg(test)]
#[path = "remap_tests.rs"]
mod tests;
