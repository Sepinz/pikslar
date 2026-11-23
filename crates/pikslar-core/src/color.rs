//! Colors and the project color table used by palette layers.

use crate::ids::SharedFloor;
use std::collections::HashMap;

/// Packs channels into the straight-alpha cell format.
#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    if a == 0 {
        0
    } else {
        u32::from_le_bytes([r, g, b, a])
    }
}

pub const MAX_COLORS: usize = 65_536;
/// A palette holds at most this many groups.
pub const MAX_GROUPS: usize = 256;
/// A group's name is cut to this many characters.
pub const MAX_GROUP_NAME: usize = 64;

/// A group name as the palette keeps it: control, direction and zero-width characters turned
/// to spaces (as palette file names are, see [`crate::names`]), cut to [`MAX_GROUP_NAME`]
/// characters.
pub fn clean_group_name(name: &str) -> String {
    crate::names::spaced(name)
        .chars()
        .take(MAX_GROUP_NAME)
        .collect()
}

/// What a group of swatches is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKind {
    /// Swatches the person put together.
    Group,
    /// Colours stepping from one swatch to another (made by phase 9.2).
    Ramp,
}

/// A named, ordered list of swatch ids. It only names swatches: it never owns a colour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteGroup {
    pub name: String,
    pub kind: GroupKind,
    pub ids: Vec<u32>,
}

/// How the swatches are put in order. Ties are broken by id, so the order is always the same.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy {
    /// Greys first, then around the colour wheel from red.
    Hue,
    /// Darkest first.
    Lightness,
    /// Most pixels first.
    Usage,
}

/// A whole-number lightness, 0 to 2,550,000: Rec. 709 luma weights in integers.
/// Integers only, so the order is the same on every machine.
pub fn lightness_key(color: u32) -> u32 {
    let [r, g, b, _] = color.to_le_bytes();
    2126 * u32::from(r) + 7152 * u32::from(g) + 722 * u32::from(b)
}

/// Where a colour sits around the wheel, in integers: greys (no hue) are `(0, 0)`, any other
/// colour is `(1, h)` with `h` from 0 to 6,143, red at 0, green at 2,048, blue at 4,096.
pub fn hue_key(color: u32) -> (u8, u32) {
    let [r, g, b, _] = color.to_le_bytes();
    let (r, g, b) = (i64::from(r), i64::from(g), i64::from(b));
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let delta = max - min;
    if delta == 0 {
        return (0, 0);
    }
    let sixth = |from: i64, to: i64, base: i64| base + ((from - to) * 1024).div_euclid(delta);
    let hue = if max == r {
        sixth(g, b, 0)
    } else if max == g {
        sixth(b, r, 2048)
    } else {
        sixth(r, g, 4096)
    };
    (1, hue.rem_euclid(6144) as u32)
}

/// `(hue_key, lightness_key)` packed in one number that sorts the same way: the grey bit, then
/// the 13 bits of hue, then the 22 bits of lightness. One key type keeps one sort in the program.
pub fn hue_order(color: u32) -> u64 {
    let (coloured, hue) = hue_key(color);
    (u64::from(coloured) << 35) | (u64::from(hue) << 22) | u64::from(lightness_key(color))
}

/// `(lightness_key, hue_key)` packed the same way: lightness, then the grey bit and the hue.
pub fn light_order(color: u32) -> u64 {
    let (coloured, hue) = hue_key(color);
    (u64::from(lightness_key(color)) << 14) | (u64::from(coloured) << 13) | u64::from(hue)
}

/// The table cannot hold another distinct color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableFull;

/// Maps color IDs to colors. ID 0 is always transparent.
///
/// `swatches` is the ordered list the user sees. The table may hold more colors than
/// that, because painting with a new color adds it automatically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorTable {
    colors: Vec<u32>,
    lookup: HashMap<u32, u32>,
    swatches: Vec<u32>,
    version: u64,
    groups: Vec<PaletteGroup>,
    /// Ids up to this have been seen by a script run: a new color goes above them, so an id a
    /// script was given never comes to name another color while the document is open. See
    /// `docs/stable-ids.md`.
    floor: SharedFloor,
}

/// What fills the ids a new color skips: transparent, never looked up, never a swatch.
const RETIRED: u32 = 0;

impl Default for ColorTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorTable {
    pub fn new() -> Self {
        ColorTable {
            colors: vec![0],
            lookup: HashMap::new(),
            swatches: Vec::new(),
            version: 1,
            groups: Vec::new(),
            floor: SharedFloor::default(),
        }
    }

    /// The highest id a new color may not take: the highest a script run has seen.
    pub fn id_floor(&self) -> u32 {
        self.floor.get()
    }

    /// Retires every id up to `id`, so a new color goes above them: for ids a script run was
    /// given or could read. Copies share their floor, so this reaches every copy.
    pub fn retire_up_to(&self, id: u32) {
        self.floor.raise(id);
    }

    /// Retires every id the table holds now ([`Self::retire_up_to`]).
    pub fn retire_held(&self) {
        self.retire_up_to(u32::try_from(self.colors.len().saturating_sub(1)).unwrap_or(u32::MAX));
    }

    /// Takes on the ids `other`, a copy of this table, holds, so a new color goes above them.
    /// Copies share their floor, so this reaches every copy. A table that is not a copy, such
    /// as one a palette load or remap made, numbers its colors afresh on purpose: nothing is
    /// taken on from it.
    #[cfg(test)]
    pub(crate) fn take_on_floor(&self, other: &ColorTable) {
        if self.floor.shared_with(&other.floor) {
            let held = u32::try_from(other.colors.len().saturating_sub(1)).unwrap_or(u32::MAX);
            self.floor.raise(held);
        }
    }

    /// The id the next new color takes: the first above the floor, unless that is past the
    /// limit, when it is the first free one, as before.
    fn next_id(&self) -> usize {
        let above = self.floor.get() as usize + 1;
        if above > self.colors.len() && above < MAX_COLORS {
            above
        } else {
            self.colors.len()
        }
    }

    pub fn len(&self) -> usize {
        self.colors.len()
    }

    /// What this table keeps alive while only history holds it: the shared allocation it lives
    /// in, its two lists and its lookup.
    pub(crate) fn kept_bytes(&self) -> usize {
        let entry = size_of::<(u32, u32)>();
        2 * size_of::<usize>()
            + size_of::<ColorTable>()
            + (self.colors.capacity() + self.swatches.capacity()) * size_of::<u32>()
            + crate::history::hash_table_bytes(self.lookup.capacity(), entry)
            + self.groups.capacity() * size_of::<PaletteGroup>()
            + self
                .groups
                .iter()
                .map(|g| g.name.capacity() + g.ids.capacity() * size_of::<u32>())
                .sum::<usize>()
    }

    pub fn is_empty(&self) -> bool {
        self.colors.len() <= 1
    }

    pub fn colors(&self) -> &[u32] {
        &self.colors
    }

    pub fn swatches(&self) -> &[u32] {
        &self.swatches
    }

    pub fn groups(&self) -> &[PaletteGroup] {
        &self.groups
    }

    /// Changes whenever a color's value changes, for render cache invalidation.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn color(&self, id: u32) -> u32 {
        self.colors.get(id as usize).copied().unwrap_or(0)
    }

    /// Returns the ID of `color` only if the table already has it.
    pub fn id_for_existing(&self, color: u32) -> Option<u32> {
        if color == 0 {
            return Some(0);
        }
        self.lookup.get(&color).copied()
    }

    /// Returns the ID of `color`, adding it to the table if new.
    pub fn id_for(&mut self, color: u32) -> Result<u32, TableFull> {
        if color == 0 {
            return Ok(0);
        }
        if let Some(&id) = self.lookup.get(&color) {
            return Ok(id);
        }
        if self.colors.len() >= MAX_COLORS {
            return Err(TableFull);
        }
        let next = self.next_id();
        self.colors.resize(next, RETIRED);
        let id = next as u32;
        self.colors.push(color);
        self.lookup.insert(color, id);
        self.version += 1;
        Ok(id)
    }

    /// Adds a color to the visible palette, returning its ID.
    pub fn add_swatch(&mut self, color: u32) -> Result<u32, TableFull> {
        let id = self.id_for(color)?;
        if id != 0 && !self.swatches.contains(&id) {
            self.swatches.push(id);
        }
        Ok(id)
    }

    /// Takes `id` out of the swatches and every group; a group this leaves empty goes, as with
    /// [`Self::set_swatches`].
    pub fn remove_swatch(&mut self, id: u32) {
        self.swatches.retain(|&s| s != id);
        for group in &mut self.groups {
            group.ids.retain(|&g| g != id);
        }
        self.groups.retain(|g| !g.ids.is_empty());
    }

    /// Moves the swatch at position `from` to position `to`, the others closing up. False if
    /// either is past the end or they are the same. Ids and colours are untouched.
    pub fn move_swatch(&mut self, from: usize, to: usize) -> bool {
        let len = self.swatches.len();
        if from >= len || to >= len || from == to {
            return false;
        }
        let id = self.swatches.remove(from);
        self.swatches.insert(to, id);
        true
    }

    /// The swatches in the order of `key`, ties by id. The table is not changed. Ids differ, so
    /// the pairs never tie and an unstable sort gives the one order a stable sort would.
    pub fn sorted_swatches(&self, key: impl Fn(u32) -> u64) -> Vec<u32> {
        let mut keyed: Vec<u128> = (self.swatches.iter())
            .map(|&id| (u128::from(key(id)) << 64) | u128::from(id))
            .collect();
        keyed.sort_unstable();
        keyed.into_iter().map(|k| k as u32).collect()
    }

    /// Puts the swatches in `order`, which must hold the same ids as now.
    pub(crate) fn set_swatch_order(&mut self, order: Vec<u32>) {
        debug_assert_eq!(order.len(), self.swatches.len());
        self.swatches = order;
    }

    /// Makes `ids` the swatches (the caller has checked that they are in the table, once each).
    /// A group keeps the members that are still swatches; one that this leaves empty goes.
    pub(crate) fn set_swatches(&mut self, ids: Vec<u32>) {
        let keep: std::collections::HashSet<u32> = ids.iter().copied().collect();
        self.swatches = ids;
        for group in &mut self.groups {
            group.ids.retain(|id| keep.contains(id));
        }
        self.groups.retain(|g| !g.ids.is_empty());
    }

    /// Adds a group at the end. The caller has checked its ids and the limit.
    pub(crate) fn push_group(&mut self, group: PaletteGroup) {
        self.groups.push(group);
    }

    pub(crate) fn groups_mut(&mut self) -> &mut Vec<PaletteGroup> {
        &mut self.groups
    }

    /// Gives the table the groups a file holds. An id that is not a swatch is dropped, and
    /// repeats within a group too; at most [`MAX_GROUPS`] are kept, names cleaned by
    /// [`clean_group_name`]. Returns true if anything was dropped.
    ///
    /// The work is in step with the number of ids: which ids are swatches is worked out once.
    pub fn set_groups(&mut self, groups: Vec<PaletteGroup>) -> bool {
        let mut dropped = groups.len() > MAX_GROUPS;
        let mut is_swatch = vec![false; self.colors.len()];
        for &id in &self.swatches {
            if let Some(slot) = is_swatch.get_mut(id as usize) {
                *slot = true;
            }
        }
        // The last group (counted from 1) each id was kept in, so repeats go.
        let mut kept_in = vec![0u32; self.colors.len()];
        self.groups = Vec::with_capacity(groups.len().min(MAX_GROUPS));
        for (at, mut group) in groups.into_iter().take(MAX_GROUPS).enumerate() {
            let wanted = group.ids.len();
            let mark = at as u32 + 1;
            group.ids.retain(|&id| {
                let fresh = is_swatch.get(id as usize).copied().unwrap_or(false)
                    && kept_in[id as usize] != mark;
                if fresh {
                    kept_in[id as usize] = mark;
                }
                fresh
            });
            dropped |= group.ids.len() != wanted;
            let name = clean_group_name(&group.name);
            if name != group.name {
                dropped = true;
                group.name = name;
            }
            self.groups.push(group);
        }
        dropped
    }

    /// Replaces the value behind an ID. Every palette pixel using it follows.
    ///
    /// Two IDs may briefly hold the same color. Lookups keep the lowest such ID so new
    /// painting stays deterministic.
    pub fn set_color(&mut self, id: u32, color: u32) {
        let Some(slot) = self.colors.get_mut(id as usize).filter(|_| id != 0) else {
            return;
        };
        let old = std::mem::replace(slot, color);
        if self.lookup.get(&old) == Some(&id) {
            self.lookup.remove(&old);
            let replacement = self.colors.iter().skip(1).position(|&c| c == old);
            if let Some(found) = replacement {
                self.lookup.insert(old, found as u32 + 1);
            }
        }
        let keep = self.lookup.get(&color).map_or(id, |&e| e.min(id));
        self.lookup.insert(color, keep);
        self.version += 1;
    }

    /// This table after a remap: the colours of the old ids `keep` take ids 1, 2, 3 and so on, in
    /// that order, and every swatch and group id goes through `to` (old id to new id). A swatch
    /// or group member that lands on id 0 is dropped, and one that lands twice is kept once, at
    /// its first place. The version moves on only if a colour now has another id, since that is
    /// what makes a cached drawing stale; merging ids changes no id's colour.
    pub(crate) fn remapped(&self, keep: &[u32], to: &[u32]) -> ColorTable {
        let at = |id: u32| to.get(id as usize).copied().unwrap_or(0);
        let mut colors = Vec::with_capacity(keep.len() + 1);
        colors.push(0);
        colors.extend(keep.iter().map(|&id| self.color(id)));
        let mut lookup = HashMap::new();
        for (id, &color) in colors.iter().enumerate().skip(1) {
            lookup.entry(color).or_insert(id as u32);
        }
        let moved = keep.iter().copied().ne(1..self.colors.len() as u32);
        let mapped = |ids: &[u32]| {
            let mut seen = std::collections::HashSet::new();
            let kept = ids.iter().map(|&id| at(id));
            kept.filter(|&id| id != 0 && seen.insert(id)).collect()
        };
        ColorTable {
            colors,
            lookup,
            swatches: mapped(&self.swatches),
            version: self.version + u64::from(moved),
            floor: SharedFloor::default(),
            groups: self
                .groups
                .iter()
                .map(|g| PaletteGroup {
                    ids: mapped(&g.ids),
                    ..g.clone()
                })
                .collect(),
        }
    }

    /// Rebuilds a table from stored colors and swatches. Colors beyond the limit are dropped.
    pub fn from_parts(colors: &[u32], swatches: &[u32]) -> Self {
        let mut table = ColorTable::new();
        for &c in colors.iter().skip(1).take(MAX_COLORS - 1) {
            let id = table.colors.len() as u32;
            table.colors.push(c);
            table.lookup.entry(c).or_insert(id);
        }
        table.swatches = swatches
            .iter()
            .copied()
            .filter(|&s| (s as usize) < table.len() && s != 0)
            .collect();
        table
    }
}

#[cfg(test)]
#[path = "color_group_tests.rs"]
mod group_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const RED: u32 = rgba(255, 0, 0, 255);
    const BLUE: u32 = rgba(0, 0, 255, 255);

    #[test]
    fn a_kept_table_is_charged_for_its_lists_and_lookup() {
        let mut t = ColorTable::new();
        for n in 1..=200u32 {
            t.id_for(rgba(n as u8, 1, 2, 255)).expect("room");
        }
        let lists = t.colors.capacity() * 4;
        let lookup = t.lookup.capacity() * size_of::<(u32, u32)>();
        assert!(t.kept_bytes() > lists + lookup);
    }

    #[test]
    fn same_color_gets_same_id() {
        let mut t = ColorTable::new();
        let a = t.id_for(RED).expect("room");
        assert_eq!(t.id_for(RED), Ok(a));
        assert_eq!(t.color(a), RED);
    }

    #[test]
    fn transparent_is_always_id_zero() {
        assert_eq!(ColorTable::new().id_for(0), Ok(0));
        assert_eq!(rgba(9, 9, 9, 0), 0);
    }

    #[test]
    fn editing_a_color_keeps_its_id_and_moves_lookup() {
        let mut t = ColorTable::new();
        let id = t.id_for(RED).expect("room");
        let before = t.version();
        t.set_color(id, BLUE);
        assert_eq!(t.color(id), BLUE);
        assert_eq!(t.id_for(BLUE), Ok(id));
        assert!(t.version() > before);
        assert_ne!(t.id_for(RED), Ok(id));
    }

    #[test]
    fn overflow_reports_table_full() {
        let mut t = ColorTable::new();
        for i in 1..MAX_COLORS as u32 {
            t.id_for(i | 0xff00_0000).expect("room");
        }
        assert_eq!(t.id_for(0xff00_0000), Err(TableFull));
        assert!(
            t.id_for(1 | 0xff00_0000).is_ok(),
            "existing colors still resolve"
        );
    }

    #[test]
    fn id_zero_cannot_be_recolored() {
        let mut t = ColorTable::new();
        t.set_color(0, RED);
        assert_eq!(t.color(0), 0);
    }

    #[test]
    fn swatches_are_unique() {
        let mut t = ColorTable::new();
        let a = t.add_swatch(RED).expect("room");
        t.add_swatch(RED).expect("room");
        assert_eq!(t.swatches(), &[a]);
    }

    #[test]
    fn from_parts_restores_ids_and_ignores_bad_swatches() {
        let t = ColorTable::from_parts(&[0, RED, BLUE], &[2, 99, 0]);
        assert_eq!(t.color(2), BLUE);
        assert_eq!(t.swatches(), &[2]);
    }
}

#[cfg(test)]
#[path = "color_floor_tests.rs"]
mod floor_tests;
