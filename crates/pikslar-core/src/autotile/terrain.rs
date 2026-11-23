//! Terrain sets: a tileset's named tables from rule slot to tile (M11).
//!
//! A set has a kind from the rule engine ([`super::rules::RuleKind`]) and one slot for each of
//! the kind's slots, in the engine's slot order. A slot holds a tile of the set's own tileset, or
//! nothing. A cell belongs to a set when its tile fills one of the set's slots, so nothing else
//! is stored. One tile may fill several slots, and several sets.

use super::rules::RuleKind;
use crate::tileset::{TileId, MAX_TILESET_NAME};

/// The kind of a terrain set: the rule engine's kinds, with their slot counts and order.
pub type TerrainKind = RuleKind;

/// A tileset holds at most this many terrain sets.
pub const MAX_TERRAINS: usize = 64;
/// Terrain names are kept to this many characters, as tileset names are.
pub const MAX_TERRAIN_NAME: usize = MAX_TILESET_NAME;

/// One terrain set of a tileset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainSet {
    id: u16,
    pub(crate) name: String,
    kind: TerrainKind,
    slots: Box<[Option<TileId>]>,
}

impl TerrainSet {
    /// A set with every slot empty.
    pub(crate) fn new(id: u16, name: String, kind: TerrainKind) -> Self {
        TerrainSet {
            id,
            name,
            kind,
            slots: vec![None; kind.slot_count()].into(),
        }
    }

    /// A set as a file kept it. `None` unless the id is from 1 and there is one slot for each of
    /// the kind's. The name is kept as given; the tileset cleans it when it takes the set.
    pub fn from_parts(
        id: u16,
        name: String,
        kind: TerrainKind,
        slots: Vec<Option<TileId>>,
    ) -> Option<Self> {
        (id > 0 && slots.len() == kind.slot_count()).then(|| TerrainSet {
            id,
            name,
            kind,
            slots: slots.into(),
        })
    }

    /// Names the set within its tileset, from 1.
    pub fn id(&self) -> u16 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> TerrainKind {
        self.kind
    }

    /// Every slot in the rule engine's order.
    pub fn slots(&self) -> &[Option<TileId>] {
        &self.slots
    }

    /// The tile in a slot, `None` if it is empty or past the kind's slots.
    pub fn slot(&self, slot: usize) -> Option<TileId> {
        self.slots.get(slot).copied().flatten()
    }

    /// Bit `i` is set when slot `i` holds a tile: what [`super::rules::pick_slot`] takes.
    pub fn defined(&self) -> u64 {
        let filled = self.slots.iter().enumerate().filter(|(_, s)| s.is_some());
        filled.fold(0, |bits, (at, _)| bits | 1 << at)
    }

    /// True if the tile fills a slot of this set.
    pub fn holds(&self, tile: TileId) -> bool {
        self.slots.contains(&Some(tile))
    }

    /// Puts a tile in a slot, or empties it. False if the slot already held that.
    pub(crate) fn set_slot(
        &mut self,
        slot: usize,
        tile: Option<TileId>,
    ) -> Result<bool, crate::document::LimitError> {
        let place = self
            .slots
            .get_mut(slot)
            .ok_or(crate::document::LimitError::TerrainSlot)?;
        let changed = *place != tile;
        *place = tile;
        Ok(changed)
    }

    /// Empties every slot the tile fills. True if there was one.
    pub(crate) fn forget(&mut self, tile: TileId) -> bool {
        self.keep_only(|t| t != tile)
    }

    /// Empties every slot whose tile `keep` refuses. True if there was one.
    pub(crate) fn keep_only(&mut self, mut keep: impl FnMut(TileId) -> bool) -> bool {
        let mut changed = false;
        for slot in self.slots.iter_mut() {
            if slot.is_some_and(|tile| !keep(tile)) {
                *slot = None;
                changed = true;
            }
        }
        changed
    }

    /// What the set takes besides its own place in a list.
    pub(crate) fn heap_bytes(&self) -> usize {
        self.name.capacity() + self.slots.len() * size_of::<Option<TileId>>()
    }
}

/// A terrain name as a set keeps it: trimmed, without control, direction or zero-width
/// characters (see [`crate::names`]), cut short.
pub fn clean_terrain_name(name: &str) -> String {
    // Control characters go first and the cut comes before the last trim, so a cleaned name
    // is clean already and a file's name reads back as it was written.
    let kept = crate::names::without_hidden(name);
    let cut: String = kept.trim().chars().take(MAX_TERRAIN_NAME).collect();
    cut.trim_end().to_owned()
}

/// The id a new set takes in a list: one past the highest, or the lowest free one if the highest
/// is the last a `u16` holds. `None` if the list is full.
pub(crate) fn next_terrain_id(sets: &[TerrainSet]) -> Option<u16> {
    if sets.len() >= MAX_TERRAINS {
        return None;
    }
    let highest = sets.iter().map(TerrainSet::id).max().unwrap_or(0);
    highest
        .checked_add(1)
        .or_else(|| (1..=u16::MAX).find(|id| sets.iter().all(|s| s.id() != *id)))
}

#[cfg(test)]
#[path = "terrain_tests.rs"]
mod tests;
