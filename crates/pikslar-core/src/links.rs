//! Links: cels that show one drawing. A hold is a link whose cels are all on one layer.
//!
//! The ids live in the cels, and one id names one link in the whole document, whichever layers
//! its cels are on. Only this file decides ids, counts a link's members across the document and
//! ends a link, so that no other code can split one or leave one with a single member. The
//! Links table is built from the cels when it is asked for and never kept beside them, so it
//! cannot disagree with them.

use crate::document::{Document, LimitError};
use crate::grid::Cell;
use crate::image::TileKey;
use crate::layer::{Cel, Layer, LayerKind, Split};
use crate::sorting::{sort_by_u64, sort_packed, sort_packed_wide};
use std::collections::HashMap;

#[cfg(test)]
#[path = "links_command_tests.rs"]
mod command_tests;
#[cfg(test)]
#[path = "links_size_tests.rs"]
mod size_tests;
#[cfg(test)]
#[path = "links_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "links_write_tests.rs"]
mod write_tests;

/// Gives holds small ids that keep their order, and ends holds of a single frame.
///
/// Ids come from files too, so nothing may assume they are small.
pub fn tidy_links(links: &mut [u32]) {
    let mut sorted: Vec<u32> = links.iter().copied().filter(|&id| id != 0).collect();
    sorted.sort_unstable();
    let shared: Vec<u32> = sorted
        .chunk_by(|a, b| a == b)
        .filter(|run| run.len() > 1)
        .map(|run| run[0])
        .collect();
    for id in links.iter_mut().filter(|id| **id != 0) {
        *id = shared.binary_search(id).map_or(0, |rank| rank as u32 + 1);
    }
}

/// One linked cel: its link, and its layer from the bottom and frame. Eight bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Member {
    pub id: u32,
    pub layer: u16,
    pub frame: u16,
}

impl Member {
    /// The member as one number, ordered as members are: by link, then layer, then frame.
    fn key(&self) -> u64 {
        (u64::from(self.id) << 32) | (u64::from(self.layer) << 16) | u64::from(self.frame)
    }

    fn from_key(key: u64) -> Member {
        Member {
            id: (key >> 32) as u32,
            layer: (key >> 16) as u16,
            frame: key as u16,
        }
    }
}

/// Sorts members by link, then layer, then frame.
fn sort_members(members: &mut [Member]) {
    sort_packed(members, Member::key, Member::from_key);
}

/// Every linked cel of a document, sorted by link, then layer, then frame. Built in one pass
/// over the cels by [`Document::link_table`] and dropped after use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkTable {
    members: Vec<Member>,
}

impl LinkTable {
    /// Every linked cel, in order.
    pub fn all(&self) -> &[Member] {
        &self.members
    }

    /// The cels of one link, in order. None for 0 or an id no cel has.
    pub fn members(&self, id: u32) -> &[Member] {
        let start = self.members.partition_point(|m| m.id < id);
        let end = self.members.partition_point(|m| m.id <= id);
        &self.members[start..end]
    }

    /// True when the link's cels are on more than one layer.
    pub fn reaches_across(&self, id: u32) -> bool {
        let members = self.members(id);
        members.first().map(|m| m.layer) != members.last().map(|m| m.layer)
    }

    /// Each link, once, with its cels.
    pub fn links(&self) -> impl Iterator<Item = &[Member]> {
        self.members.chunk_by(|a, b| a.id == b.id)
    }
}

/// The links that reach more than one layer, from [`Document::links_reaching_across`]. Built
/// from the cels when asked and dropped after use, like [`LinkTable`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Across {
    ids: Vec<u32>,
}

impl Across {
    /// True when no link reaches across layers, the common case.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// True when the link `id` has cels on more than one layer. False for 0.
    pub fn contains(&self, id: u32) -> bool {
        id != 0 && self.ids.binary_search(&id).is_ok()
    }
}

/// A broken rule of the links, as `Document::check_links` finds it, naming the link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkFault {
    /// L1: a link with one cel.
    Lone(u32),
    /// L2: cels of one link that show different drawings.
    Unshared(u32),
    /// L3: a link joining layers of both color modes.
    MixedModes(u32),
    /// L4: a link with a reference cel and a cel of another layer.
    Reference(u32),
    /// L5: a group cel in a link.
    Group(u32),
    /// L3, for tilemaps: a link joining layers of different tilesets, or a tilemap layer and a
    /// layer of another kind. A link has one colour mode for paint members and one tileset for
    /// tilemap members, never mixed.
    MixedTilesets(u32),
}

impl std::fmt::Display for LinkFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkFault::Lone(id) => write!(f, "L1: link {id} has one cel"),
            LinkFault::Unshared(id) => write!(f, "L2: link {id} shows more than one drawing"),
            LinkFault::MixedModes(id) => write!(f, "L3: link {id} joins both color modes"),
            LinkFault::Reference(id) => write!(f, "L4: link {id} joins a reference to another"),
            LinkFault::Group(id) => write!(f, "L5: link {id} has a group cel"),
            LinkFault::MixedTilesets(id) => {
                write!(f, "L3: link {id} joins layers of different tilesets")
            }
        }
    }
}

/// What is wrong with one link's cels, if anything.
fn fault_of(doc: &Document, members: &[Member]) -> Option<LinkFault> {
    let first = members.first()?;
    let id = first.id;
    let layer = |m: &Member| &doc.layers[usize::from(m.layer)];
    let cel = |m: &Member| layer(m).cel(usize::from(m.frame));
    let lead = layer(first);
    if members.len() < 2 {
        return Some(LinkFault::Lone(id));
    }
    if members.iter().any(|m| layer(m).kind == LayerKind::Group) {
        return Some(LinkFault::Group(id));
    }
    if members.iter().any(|m| layer(m).mode != lead.mode) {
        return Some(LinkFault::MixedModes(id));
    }
    if members.iter().any(|m| layer(m).tileset() != lead.tileset()) {
        return Some(LinkFault::MixedTilesets(id));
    }
    let reference = members
        .iter()
        .any(|m| layer(m).kind == LayerKind::Reference);
    if reference && members.iter().any(|m| m.layer != first.layer) {
        return Some(LinkFault::Reference(id));
    }
    if members.iter().any(|m| !cel(m).same_image(cel(first))) {
        return Some(LinkFault::Unshared(id));
    }
    None
}

impl Document {
    /// Every linked cel of the document, sorted by link. One pass over the cels.
    pub fn link_table(&self) -> LinkTable {
        let mut members = Vec::new();
        for (layer, l) in self.layers.iter().enumerate() {
            for (frame, id) in l.links().enumerate().filter(|&(_, id)| id != 0) {
                // Within a document's limits, which fit 16 bits.
                members.push(Member {
                    id,
                    layer: layer as u16,
                    frame: frame as u16,
                });
            }
        }
        sort_members(&mut members);
        LinkTable { members }
    }

    /// The cels that show this cel's drawing and change with it, on every layer, from the
    /// bottom: its link, or the cel alone. Nothing for a cel that does not exist.
    pub fn link_members(&self, layer: usize, frame: usize) -> Vec<(usize, usize)> {
        let Some(l) = self.layers.get(layer).filter(|l| frame < l.frame_count()) else {
            return Vec::new();
        };
        let id = l.link(frame);
        if id == 0 {
            return vec![(layer, frame)];
        }
        let mut found = Vec::new();
        for (index, other) in self.layers.iter().enumerate() {
            let frames = other.links().enumerate().filter(|&(_, link)| link == id);
            found.extend(frames.map(|(f, _)| (index, f)));
        }
        found
    }

    /// The links whose cels are on more than one layer. One pass over the cels, sorting only
    /// each layer's distinct ids, so it is asked once per repaint or command and then answers
    /// each cel by a binary search.
    pub fn links_reaching_across(&self) -> Across {
        // Each id with its layer, the id in the high half: sorted, a link's entries are a run.
        let mut seen: Vec<u128> = Vec::new();
        let mut ids: Vec<u32> = Vec::new();
        for (index, layer) in self.layers.iter().enumerate() {
            ids.clear();
            ids.extend(layer.links().filter(|&id| id != 0));
            // Holds are runs, so most repeats go before the sort.
            ids.dedup();
            ids.sort_unstable();
            ids.dedup();
            seen.extend(ids.iter().map(|&id| (u128::from(id) << 64) | index as u128));
        }
        seen.sort_unstable();
        let across = seen
            .chunk_by(|a, b| a >> 64 == b >> 64)
            .filter(|run| run.len() > 1);
        Across {
            ids: across.map(|run| (run[0] >> 64) as u32).collect(),
        }
    }

    /// True when a cel of this layer is in a link with a cel of another layer. One pass.
    pub fn shares_with_other_layers(&self, layer: usize) -> bool {
        let Some(l) = self.layers.get(layer) else {
            return false;
        };
        if l.links().all(|id| id == 0) {
            return false;
        }
        let across = self.links_reaching_across();
        !across.is_empty() && l.links().any(|id| across.contains(id))
    }

    /// True when this cel is in a link with a cel of another layer. One pass over the cels,
    /// so code asking about many cels asks [`Self::links_reaching_across`] once instead.
    pub fn links_across(&self, layer: usize, frame: usize) -> bool {
        let id = self.layers.get(layer).map_or(0, |l| l.link(frame));
        id != 0
            && self
                .layers
                .iter()
                .enumerate()
                .any(|(index, other)| index != layer && other.links().any(|link| link == id))
    }

    /// Checks the rules every document keeps, L1 to L5 of `docs/links-and-tween.md`, and names
    /// the first link that breaks one.
    pub fn check_links(&self) -> Result<(), LinkFault> {
        let table = self.link_table();
        let fault = table.links().find_map(|members| fault_of(self, members));
        match fault {
            Some(fault) => Err(fault),
            None => Ok(()),
        }
    }

    /// An id for a new link, unused on any layer. Counted from what is there, so that undoing
    /// and redoing the same edits gives the same numbers.
    pub(crate) fn new_link_id(&self) -> u32 {
        let last = self.layers.iter().flat_map(Layer::links).max();
        last.map_or(1, |id| id + 1)
    }

    fn check_cel(&self, layer: usize, frame: usize) -> Result<(), LimitError> {
        let found = self
            .layers
            .get(layer)
            .is_some_and(|l| frame < l.frame_count());
        found.then_some(()).ok_or(LimitError::NoSuchItem)
    }

    pub fn is_held(&self, layer: usize, frame: usize) -> bool {
        self.layers.get(layer).is_some_and(|l| l.link(frame) != 0)
    }

    /// True when this frame and the next show one held drawing.
    pub fn held_into_next(&self, layer: usize, frame: usize) -> bool {
        let Some(l) = self.layers.get(layer) else {
            return false;
        };
        l.link(frame) != 0 && l.link(frame) == l.link(frame + 1)
    }

    /// Makes this frame show the same drawing as the frame before it, joining its link.
    pub fn hold_previous(&mut self, layer: usize, frame: usize) -> Result<(), LimitError> {
        self.check_cel(layer, frame)?;
        if frame == 0 {
            return Err(LimitError::NoSuchItem);
        }
        let fresh = self.new_link_id();
        let l = &mut self.layers[layer];
        let id = if l.link(frame - 1) != 0 {
            l.link(frame - 1)
        } else {
            fresh
        };
        l.set_link(frame - 1, id);
        l.hold_frame(frame, frame - 1, id);
        self.tidy_holds();
        Ok(())
    }

    /// Shows one frame's drawing in every frame of a range.
    pub fn hold_across(
        &mut self,
        layer: usize,
        source: usize,
        from: usize,
        to: usize,
    ) -> Result<(), LimitError> {
        self.check_cel(layer, source)?;
        self.check_cel(layer, to)?;
        let fresh = self.new_link_id();
        let l = &mut self.layers[layer];
        let id = if l.link(source) != 0 {
            l.link(source)
        } else {
            fresh
        };
        for frame in from.min(source)..=to.max(source) {
            if (from..=to).contains(&frame) || frame == source {
                l.hold_frame(frame, source, id);
            }
        }
        self.tidy_holds();
        Ok(())
    }

    /// Replaces one cel, for [`Document::put_cel`]: a cel showing the link's drawing stays in
    /// it, any other leaves it.
    pub(crate) fn put_cel_in_link(
        &mut self,
        layer: usize,
        frame: usize,
        cel: Cel,
    ) -> Result<(), LimitError> {
        let l = self.layers.get_mut(layer).ok_or(LimitError::NoSuchItem)?;
        let linked = frame < l.frame_count() && l.link(frame) != 0;
        let leaves = linked && !l.cel(frame).same_image(&cel);
        l.put_cel(frame, cel)?;
        if leaves {
            l.set_link(frame, 0);
            self.tidy_holds();
        }
        Ok(())
    }

    /// Takes this frame out of its link, whichever layers the link reaches. It keeps showing
    /// the drawing until it is drawn on; the other cels keep theirs.
    pub fn release_hold(&mut self, layer: usize, frame: usize) -> Result<(), LimitError> {
        self.check_cel(layer, frame)?;
        self.layers[layer].set_link(frame, 0);
        self.tidy_holds();
        Ok(())
    }

    /// The cels of other layers in this cel's link, from the bottom.
    fn members_elsewhere(&self, layer: usize, frame: usize) -> Vec<(usize, usize)> {
        let id = self.layers.get(layer).map_or(0, |l| l.link(frame));
        if id == 0 {
            return Vec::new();
        }
        let mut found = Vec::new();
        for (index, other) in self.layers.iter().enumerate().filter(|(i, _)| *i != layer) {
            let frames = other.links().enumerate().filter(|&(_, link)| link == id);
            found.extend(frames.map(|(f, _)| (index, f)));
        }
        found
    }

    /// Swaps tiles into a cel's drawing as [`Layer::swap_tiles`] does, then shows the result in
    /// every cel of its link on other layers too, each at its own offset. The other cels let go
    /// of the drawing first, so the edit copies no table. Returns whether it reached another
    /// layer.
    pub(crate) fn swap_tiles(
        &mut self,
        layer: usize,
        frame: usize,
        changes: &mut [(TileKey, Cell)],
        split: &mut Split,
    ) -> bool {
        let Some(edited) = self.layers.get_mut(layer) else {
            return false;
        };
        // A cel in no link, or no change, reaches no other layer: the usual case, kept direct.
        if changes.is_empty() || edited.link(frame) == 0 {
            edited.swap_tiles(frame, changes, split);
            return false;
        }
        self.swap_linked_tiles(layer, frame, changes, split)
    }

    /// [`Self::swap_tiles`] for a cel in a link, which may reach other layers.
    #[inline(never)]
    fn swap_linked_tiles(
        &mut self,
        layer: usize,
        frame: usize,
        changes: &mut [(TileKey, Cell)],
        split: &mut Split,
    ) -> bool {
        let elsewhere = self.members_elsewhere(layer, frame);
        for &(l, f) in &elsewhere {
            self.layers[l].let_go_of_drawing(f);
        }
        self.layers[layer].swap_tiles(frame, changes, split);
        if elsewhere.is_empty() {
            // Most edits reach no other layer, so nothing more is needed.
            return false;
        }
        let Some(edited) = self.layers[layer].drawing(frame).cloned() else {
            return false;
        };
        for &(l, f) in &elsewhere {
            self.layers[l].show_drawing(f, &edited);
        }
        !elsewhere.is_empty()
    }

    /// Gives the copies at `first_copy` and up of the layers `from..=to` links of their own: a
    /// link wholly inside those layers is copied as a new link the copies share, and a link
    /// reaching outside them is left to the originals, so its copies are in no link.
    pub(crate) fn link_copies(&mut self, (from, to): (usize, usize), first_copy: usize) {
        let table = self.link_table();
        let inside = |id: u32| {
            let members = table.members(id);
            members
                .iter()
                .all(|m| (from..=to).contains(&usize::from(m.layer)))
        };
        let mut fresh = self.new_link_id();
        let mut renamed = HashMap::new();
        for row in 0..=(to - from) {
            let links: Vec<u32> = self.layers[from + row].links().collect();
            for (frame, id) in links.into_iter().enumerate() {
                let copy = match id {
                    0 => 0,
                    id if !inside(id) => 0,
                    id => renamed_id(&mut renamed, id, &mut fresh),
                };
                self.layers[first_copy + row].set_link(frame, copy);
            }
        }
        self.tidy_holds();
    }

    /// Takes the layer's cels out of every link that reaches another layer. Links of its own
    /// cels with each other stay, as holds. Returns how many links across layers it ended.
    pub(crate) fn unlink_from_other_layers(&mut self, index: usize) -> usize {
        let across = self.links_reaching_across();
        let mut ended: Vec<u32> = self.layers[index]
            .links()
            .filter(|&id| across.contains(id))
            .collect();
        ended.sort_unstable();
        ended.dedup();
        let mut fresh = self.new_link_id();
        let mut renamed = HashMap::new();
        let links: Vec<u32> = self.layers[index].links().collect();
        for (frame, id) in links.into_iter().enumerate() {
            if ended.binary_search(&id).is_ok() {
                let own = renamed_id(&mut renamed, id, &mut fresh);
                self.layers[index].set_link(frame, own);
            }
        }
        self.tidy_holds();
        ended.len()
    }

    /// Sets every cel's link from ids of any size, one list per layer from the bottom, as a
    /// file gives them. No cel's drawing changes, and an id on one cel is no link. A link that
    /// breaks a rule of the links, such as joining layers of both color modes or cels showing
    /// different drawings, is taken apart by layer: each layer's part that keeps every rule on
    /// its own stays, as a hold, and its other cels are in no link. So nothing is lost that was
    /// sound. The ids are then ranked across the document, keeping their order. Returns how
    /// many links broke a rule.
    pub fn set_links(&mut self, links: &[Vec<u32>]) -> Result<usize, LimitError> {
        let fits = links.len() == self.layers.len()
            && self
                .layers
                .iter()
                .zip(links)
                .all(|(layer, ids)| ids.len() == layer.frame_count());
        if !fits {
            return Err(LimitError::NoSuchItem);
        }
        let mut members: Vec<Member> = Vec::new();
        for (layer, ids) in links.iter().enumerate() {
            let linked = ids.iter().enumerate().filter(|&(_, &id)| id != 0);
            // Within a document's limits, which fit 16 bits.
            members.extend(linked.map(|(frame, &id)| Member {
                id,
                layer: layer as u16,
                frame: frame as u16,
            }));
        }
        sort_members(&mut members);
        let (parts, broken) = self.sound_parts(&members);
        let mut ranked: Vec<Vec<u32>> = links.iter().map(|ids| vec![0; ids.len()]).collect();
        for (rank, part) in parts.iter().enumerate() {
            for m in part.iter() {
                ranked[usize::from(m.layer)][usize::from(m.frame)] = rank as u32 + 1;
            }
        }
        for (layer, ids) in self.layers.iter_mut().zip(&ranked) {
            layer.put_links(ids);
        }
        Ok(broken)
    }

    /// The links of `members`, sorted by id, that keep every rule, in order, with each broken
    /// link's sound parts in its place; and how many links were broken.
    fn sound_parts(&self, members: &[Member]) -> (Vec<Vec<Member>>, usize) {
        let mut parts = Vec::new();
        let mut broken = 0;
        for link in members.chunk_by(|a, b| a.id == b.id) {
            match fault_of(self, link) {
                None => parts.push(link.to_vec()),
                Some(LinkFault::Lone(_)) => {}
                Some(_) => {
                    broken += 1;
                    for own in link.chunk_by(|a, b| a.layer == b.layer) {
                        parts.extend(self.holds_within(own));
                    }
                }
            }
        }
        (parts, broken)
    }

    /// The cels of one layer's part of a broken link that show one drawing, as holds that keep
    /// every rule, ordered by their first frame so that the numbering never depends on memory.
    fn holds_within(&self, own: &[Member]) -> Vec<Vec<Member>> {
        let image = |m: &Member| self.layers[usize::from(m.layer)].cel(usize::from(m.frame));
        let mut by_image: Vec<(usize, Member)> =
            own.iter().map(|m| (image(m).image_id(), *m)).collect();
        sort_packed_wide(
            &mut by_image,
            |&(image, m)| ((image as u128) << 64) | u128::from(m.key()),
            |key| ((key >> 64) as u64 as usize, Member::from_key(key as u64)),
        );
        let mut holds: Vec<Vec<Member>> = by_image
            .chunk_by(|a, b| a.0 == b.0)
            .map(|run| run.iter().map(|&(_, m)| m).collect::<Vec<Member>>())
            .filter(|hold| fault_of(self, hold).is_none())
            .collect();
        // One layer's holds share no cel, so no two begin on one frame.
        sort_by_u64(&mut holds, |hold| u64::from(hold[0].frame));
        holds
    }

    /// Ends every link left with one cel anywhere in the document. Run after anything that
    /// can take cels out of a link.
    pub(crate) fn tidy_holds(&mut self) {
        let mut ids = counted_ids(self.layers.iter());
        if ids.is_empty() {
            return;
        }
        ids.sort_unstable();
        let mut lone: Vec<u32> = ids
            .chunk_by(|a, b| a == b)
            .filter(|run| run.len() == 1)
            .map(|run| run[0])
            .collect();
        if lone.is_empty() {
            return;
        }
        lone.sort_unstable();
        let is_lone = |id: u32| id != 0 && lone.binary_search(&id).is_ok();
        for layer in self.layers.iter_mut() {
            let ended: Vec<usize> = layer
                .links()
                .enumerate()
                .filter(|&(_, id)| is_lone(id))
                .map(|(frame, _)| frame)
                .collect();
            ended.into_iter().for_each(|frame| layer.set_link(frame, 0));
        }
    }
}

/// Every link id of the layers, once for each run of equal ids, twice for a run of two cels or
/// more: enough to tell a link of one cel from the rest without listing every cel.
fn counted_ids<'a>(layers: impl IntoIterator<Item = &'a Layer>) -> Vec<u32> {
    let mut ids = Vec::new();
    let mut note = |id: u32, run: usize| {
        if id != 0 {
            ids.push(id);
            if run > 1 {
                ids.push(id);
            }
        }
    };
    for layer in layers {
        let mut run = (0, 0);
        for id in layer.links() {
            if id == run.0 {
                run.1 += 1;
            } else {
                note(run.0, run.1);
                run = (id, 1);
            }
        }
        note(run.0, run.1);
    }
    ids
}

/// The new id a copied link takes, the same for every cel of the link.
fn renamed_id(renamed: &mut HashMap<u32, u32>, id: u32, fresh: &mut u32) -> u32 {
    *renamed.entry(id).or_insert_with(|| {
        *fresh += 1;
        *fresh - 1
    })
}
