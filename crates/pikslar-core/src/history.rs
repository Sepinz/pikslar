//! Undo and redo. Pixel edits store only the tile references that changed.

use crate::document::Document;
use crate::grid::Cell;
use crate::image::TileKey;
use crate::mask::Mask;
use std::collections::HashSet;
use std::sync::Arc;

/// Default retained history. Older steps are dropped beyond this.
pub const DEFAULT_BUDGET: usize = 256 * 1024 * 1024;

/// A layer and a frame: where the artist is.
pub type Place = (usize, usize);

mod held;
mod rewind;
mod saved;
mod snapshot;
mod tile_images;

#[cfg(test)]
mod audit_tests;
#[cfg(test)]
mod charge_tests;
#[cfg(test)]
mod tile_charge_tests;
pub use rewind::RewindMarks;
pub use saved::SavedCopy;
pub use snapshot::Snapshot;

/// What a hash table with room for `room` entries of `entry` bytes takes: an eighth more slots
/// than its room, at least one more for a small table, a control byte for each slot, and one
/// group of control bytes more.
pub(crate) fn hash_table_bytes(room: usize, entry: usize) -> usize {
    const GROUP: usize = 16;
    if room == 0 {
        return 0;
    }
    let slots = if room < 8 { room + 1 } else { room / 7 * 8 };
    slots * (entry + 1) + GROUP
}

/// The selection to restore with an edit, for edits that move it.
pub type SavedSelection = Option<Option<Arc<Mask>>>;

pub enum Edit {
    /// Tile references to restore on one cel. The indices are narrow because thousands of these
    /// steps are kept and a document has at most 256 layers and 4,096 frames, which leaves room
    /// for the charge without making any step larger.
    Tiles {
        layer: u16,
        frame: u16,
        /// What the step is charged for the drawing it split off while only history's steps
        /// keep it. History sets it; a new step starts at nothing.
        left: u32,
        /// How much more the tiles it keeps take once it is next played, when it keeps the
        /// tiles it swaps in. Each play turns it around.
        swing: i32,
        changes: Box<[(TileKey, Cell)]>,
        selection: SavedSelection,
        /// The color table before this, if painting or pasting added to it.
        colors: Option<Arc<crate::color::ColorTable>>,
        /// The drawing the write split off, shown again by undoing it.
        split: crate::layer::Split,
    },
    /// Cels moved together, and how far. Undo moves them back; no pixel is kept.
    Move {
        layer: usize,
        frames: Box<[u32]>,
        dx: i32,
        dy: i32,
    },
    /// The selection before a change to it. Selecting does not change the file.
    Selection(Option<Arc<Mask>>),
    /// Structural change or color edit: the whole previous state, which is cheap to hold
    /// because cels are shared.
    Snapshot(Box<Snapshot>),
    /// The images some tiles of one tileset had before an edit of their pixels. Playing the
    /// step swaps them with the images the tiles have now.
    TileImages {
        tileset: crate::tileset::TilesetId,
        before: Box<[(crate::tileset::TileId, Arc<crate::image::Image>)]>,
        /// The color table before this, if the edit added to it.
        colors: Option<Arc<crate::color::ColorTable>>,
        /// What the step is charged for the images only history's steps keep. History sets it.
        charge: usize,
    },
}

impl Edit {
    /// False for an edit that leaves the file as it was.
    pub(crate) fn modifies(&self) -> bool {
        !matches!(self, Edit::Selection(_))
    }

    /// What this edit keeps, with its own room in history's lists, so that the budget bounds
    /// how many steps are kept however little each keeps besides.
    fn size(&self) -> usize {
        SLOT + match self {
            Edit::Tiles {
                changes,
                selection,
                colors,
                left,
                ..
            } => {
                let tiles = tile_bytes(changes);
                let shape: usize = selection.iter().flatten().map(|m| m.kept_bytes()).sum();
                let table = colors.as_ref().map_or(0, |c| c.kept_bytes());
                let list = size_of_val::<[(TileKey, Cell)]>(changes);
                tiles + shape + table + list + *left as usize
            }
            Edit::Move { frames, .. } => size_of_val::<[u32]>(frames),
            Edit::Selection(saved) => saved.iter().map(|m| m.kept_bytes()).sum(),
            Edit::Snapshot(s) => s.size(),
            Edit::TileImages { .. } => self.tile_images_size(),
        }
    }

    /// Applies this edit in place, leaving its inverse, and moves `place` to where it can be seen.
    /// Returns how much more, or less, the step takes afterwards.
    fn apply(
        &mut self,
        doc: &mut Document,
        current: &mut Option<Arc<Mask>>,
        place: &mut Place,
        held: &held::Held,
    ) -> isize {
        match self {
            Edit::Tiles { .. } => self.play_tiles(doc, current, place, held),
            Edit::Move {
                layer,
                frames,
                dx,
                dy,
            } => {
                (*dx, *dy) = (-*dx, -*dy);
                move_cels(doc, place, (*layer, frames), (*dx, *dy));
                0
            }
            Edit::Selection(saved) => swap_selection(saved, current),
            Edit::Snapshot(s) => {
                s.swap_with(doc, place, current);
                0
            }
            Edit::TileImages { .. } => {
                self.swap_tile_images(doc);
                0
            }
        }
    }

    /// Plays a pixel step. It keeps the other tiles, selection and color table afterwards, and
    /// the write either shows a drawing it split off again or splits one off that history alone
    /// may keep.
    fn play_tiles(
        &mut self,
        doc: &mut Document,
        current: &mut Option<Arc<Mask>>,
        place: &mut Place,
        held: &held::Held,
    ) -> isize {
        let Edit::Tiles {
            layer,
            frame,
            changes,
            selection,
            colors,
            left,
            swing,
            split,
        } = self
        else {
            return 0;
        };
        // The tiles it keeps now are the ones it took out, worked out when recorded.
        let mut shape = std::mem::replace(swing, swing.wrapping_neg()) as isize;
        shape += match selection {
            Some(saved) => swap_selection(saved, current),
            None => 0,
        };
        if let Some(table) = colors {
            let before = table.kept_bytes() as isize;
            std::mem::swap(table, &mut doc.colors);
            shape += table.kept_bytes() as isize - before;
        }
        let at = (*layer as usize, *frame as usize);
        if at.0 >= doc.layers.len() {
            return shape;
        }
        doc.swap_tiles(at.0, at.1, changes, split);
        if !shows_same_drawing(doc, *place, at) {
            *place = at;
        }
        // Mostly nothing was split either way, and the charge stays nothing.
        if split.is_some() || *left != 0 {
            return shape + self.charge_again(held);
        }
        shape
    }
}

/// What the tiles in a pixel step's list take.
pub(crate) fn tile_bytes(changes: &[(TileKey, Cell)]) -> usize {
    changes
        .iter()
        .map(|(_, c)| c.as_ref().map_or(0, |t| t.heap_size()))
        .sum()
}

/// Moves cels of one layer, and shows the first of them unless one is shown already.
fn move_cels(
    doc: &mut Document,
    place: &mut Place,
    (layer, frames): (usize, &[u32]),
    by: (i32, i32),
) {
    let Some(l) = doc.layers.get_mut(layer) else {
        return;
    };
    l.move_cels(frames, by.0, by.1);
    let here = place.0 == layer && frames.contains(&(place.1 as u32));
    if let (false, Some(&first)) = (here, frames.first()) {
        *place = (layer, first as usize);
    }
}

/// Swaps the selection a step keeps with the current one, and returns how much more, or less, the
/// step takes for it.
fn swap_selection(saved: &mut Option<Arc<Mask>>, current: &mut Option<Arc<Mask>>) -> isize {
    let bytes = |m: &Option<Arc<Mask>>| m.as_ref().map_or(0, |m| m.kept_bytes()) as isize;
    let before = bytes(saved);
    std::mem::swap(saved, current);
    bytes(saved) - before
}

/// Charges a pixel step again, once played, for the drawing it split off: playing it either
/// shows that drawing again, or splits one off that history alone may now keep. Returns the
/// change. Out of line, as most pixel steps split nothing.
impl Edit {
    #[cold]
    #[inline(never)]
    fn charge_again(&mut self, held: &held::Held) -> isize {
        let Edit::Tiles {
            split,
            changes,
            left,
            ..
        } = self
        else {
            return 0;
        };
        let before = *left;
        *left = left_to_history(held, (split, changes));
        *left as isize - before as isize
    }
}

/// What a pixel step is charged for the drawing it split off, if only history's steps keep it:
/// the drawing, less the tiles it replaced, which the step is charged for already and which are
/// the drawing's own.
fn left_to_history(
    held: &held::Held,
    (split, changes): (&crate::layer::Split, &[(TileKey, Cell)]),
) -> u32 {
    let replaced = u32::try_from(tile_bytes(changes)).unwrap_or(u32::MAX);
    held.left_to_history(split).saturating_sub(replaced)
}

/// True if both places show one linked drawing, on any layers, so either shows a change to it.
fn shows_same_drawing(doc: &Document, a: Place, b: Place) -> bool {
    let link = |(layer, frame): Place| doc.layers.get(layer).map_or(0, |l| l.link(frame));
    // `b` is the cel just played, so its link is the one at hand, and mostly none.
    a == b || {
        let edited = link(b);
        edited != 0 && link(a) == edited
    }
}

/// The room one step takes in history's lists.
const SLOT: usize = size_of::<(Edit, usize)>();

// A pixel step keeps its layer and frame in 16 bits each, which every document's limits fit, so
// that its charge fits beside them and no step grows.
const _: () = assert!(crate::document::MAX_LAYERS <= u16::MAX as usize);
const _: () = assert!(crate::document::MAX_FRAMES <= u16::MAX as usize);
// 72 bytes is what a step took before pixel steps kept their charge. Most kept steps are pixel
// steps, so a larger step would make every history larger.
const _: () = assert!(SLOT <= 72);

pub struct History {
    undo: Vec<(Edit, usize)>,
    redo: Vec<(Edit, usize)>,
    used: usize,
    budget: usize,
    /// How many steps have ever been recorded. What a step in progress is joined against.
    pushes: u64,
    /// The drawings structural steps keep, so each is charged once.
    held: held::Held,
    /// What could be redone before the newest step, a value in progress. It comes back if that
    /// step is taken back as nothing, and is dropped by anything else that changes history.
    aside: Vec<(Edit, usize)>,
    /// Where the undo list stands for Rewind: see [`rewind::Marks`].
    marks: rewind::Marks,
    /// Where the file was saved: see [`saved`].
    saved: saved::Saved,
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        Self::with_budget(DEFAULT_BUDGET)
    }

    pub fn with_budget(budget: usize) -> Self {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            used: 0,
            budget,
            pushes: 0,
            held: held::Held::default(),
            aside: Vec::new(),
            marks: rewind::Marks::default(),
            saved: saved::Saved::default(),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// How many steps can be undone.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// How many steps can be redone.
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// How many cels the steps between here and `depth` draw on.
    ///
    /// Growing a table frees the one it replaces, so a cel costs one table however many steps
    /// pass through it. A snapshot brings back images that already exist and costs none.
    #[inline(never)]
    pub(crate) fn images_edited_on_the_way_to(&self, depth: usize) -> usize {
        let now = self.undo.len();
        let walked = if depth < now {
            &self.undo[depth..]
        } else {
            let steps = (depth - now).min(self.redo.len());
            &self.redo[self.redo.len() - steps..]
        };
        let cels = walked.iter().filter_map(|(edit, _)| match edit {
            Edit::Tiles { layer, frame, .. } => Some((*layer, *frame)),
            Edit::Move { .. }
            | Edit::Selection(_)
            | Edit::Snapshot(_)
            | Edit::TileImages { .. } => None,
        });
        cels.collect::<HashSet<_>>().len()
    }

    /// How many steps have been recorded and not taken back as though they never happened.
    pub fn pushes(&self) -> u64 {
        self.pushes
    }

    /// What history keeps: its steps, and the lists that hold them. The budget is for the
    /// steps; the lists cost a few dozen bytes a step on top.
    pub fn memory_used(&self) -> usize {
        self.used + self.lists()
    }

    /// True if the steps kept take more than the budget: no step is taken back from disk.
    pub fn is_over_budget(&self) -> bool {
        self.used > self.budget
    }

    /// The room history's lists keep for steps they do not hold yet, or no longer hold, and
    /// the counts of drawings. Each step's own room is in what the step is charged.
    fn lists(&self) -> usize {
        let spare = |list: &Vec<(Edit, usize)>| list.capacity() - list.len();
        let room = spare(&self.undo) + spare(&self.redo) + spare(&self.aside);
        room * SLOT + self.held.bytes()
    }

    /// True if undoing the next step changes the file.
    pub(crate) fn undo_modifies(&self) -> bool {
        self.undo.last().is_none_or(|(edit, _)| edit.modifies())
    }

    /// True if redoing the next step changes the file.
    pub(crate) fn redo_modifies(&self) -> bool {
        self.redo.last().is_none_or(|(edit, _)| edit.modifies())
    }

    /// Takes the newest step back off if it turned out to change nothing.
    /// Has the newest pixel step also restore this selection, unless it already restores one.
    pub(crate) fn keep_selection_on_last(&mut self, kept: Option<Arc<Mask>>) {
        if let Some((Edit::Tiles { selection, .. }, size)) = self.undo.last_mut() {
            if selection.is_none() {
                // The step was charged when it was recorded, so it is charged for this too.
                let shape = kept.as_ref().map_or(0, |m| m.kept_bytes());
                *selection = Some(kept);
                *size += shape;
                self.used += shape;
            }
        }
    }

    pub(crate) fn pop_if_same(&mut self, doc: &Document, selection: Option<&Arc<Mask>>) -> bool {
        let same = match self.undo.last() {
            Some((Edit::Snapshot(s), _)) => s.matches(doc) && s.keeps_selection(selection),
            Some((Edit::Move { dx, dy, .. }, _)) => (*dx, *dy) == (0, 0),
            _ => false,
        };
        if same {
            self.mark_top();
            self.newest_step_changed();
            if let Some((edit, size)) = self.undo.pop() {
                self.drop_step(edit, size);
                self.pushes -= 1;
                self.bring_back_aside();
            }
        }
        same
    }

    /// Adds to the newest step if it is a move, as a drag continues. Its size does not change.
    pub(crate) fn join_move(&mut self, by: (i32, i32)) {
        self.mark_top();
        self.newest_step_changed();
        if let Some((Edit::Move { dx, dy, .. }, _)) = self.undo.last_mut() {
            (*dx, *dy) = (*dx + by.0, *dy + by.1);
        }
    }

    pub fn push(&mut self, edit: Edit) {
        self.push_with(edit, false);
    }

    /// Records the step of a value in progress, setting what could be redone aside so that it
    /// comes back if the value comes to nothing.
    pub(crate) fn push_setting_redo_aside(&mut self, edit: Edit) {
        self.push_with(edit, true);
    }

    fn push_with(&mut self, mut edit: Edit, set_aside: bool) {
        edit.take_on(&mut self.held);
        self.store(edit, set_aside);
    }

    /// Forgets what was set aside to redo. Most calls find nothing, so that test is inline.
    #[inline]
    pub(crate) fn drop_aside(&mut self) {
        if !self.aside.is_empty() {
            self.drop_aside_now();
        }
    }

    #[inline(never)]
    fn drop_aside_now(&mut self) {
        for (edit, size) in std::mem::take(&mut self.aside) {
            self.drop_step(edit, size);
        }
    }

    /// Puts back what was set aside to redo, once the step that set it aside is gone. Only a
    /// value still in progress is taken back, and the editor's undo and redo end the value in
    /// the same call, which drops the list. So redo is empty here; if it ever is not, the list
    /// is dropped rather than put over it.
    fn bring_back_aside(&mut self) {
        debug_assert!(self.redo.is_empty(), "redo while a list is set aside");
        if self.redo.is_empty() {
            self.redo = std::mem::take(&mut self.aside);
        } else {
            self.drop_aside();
        }
    }

    /// What the steps alone keep, which is what the budget is for.
    #[cfg(test)]
    pub(crate) fn steps_used(&self) -> usize {
        self.used
    }

    #[cfg(test)]
    pub(crate) fn aside_depth(&self) -> usize {
        self.aside.len()
    }

    pub(crate) fn set_budget(&mut self, budget: usize) {
        self.budget = budget;
    }

    /// Forgets a step, letting go of what it keeps.
    fn drop_step(&mut self, mut edit: Edit, size: usize) {
        edit.let_go(&mut self.held);
        self.used -= size;
    }

    /// Keeps only what the newest step altered, if it is a structural step still open, and
    /// charges it for that. Only right while the document is as that step's change left it.
    /// Says whether it finished one.
    pub(crate) fn finish_last(&mut self, doc: &Document) -> bool {
        self.change_last(|s, held| {
            if !s.is_open() {
                return false;
            }
            s.finish(doc);
            // What the step kept of the tilesets is known only now.
            s.take_on_tiles(held);
            true
        })
    }

    /// Opens the newest step again, finished by `finish_last` while the document is unchanged
    /// since, so a value in progress can go on.
    pub(crate) fn reopen_last(&mut self, doc: &Document) {
        // The value goes on, so the state this step leads to changes.
        self.mark_top();
        self.newest_step_changed();
        self.change_last(|s, held| {
            if !s.is_open() {
                s.let_go_tiles(held);
            }
            s.reopen(doc);
            true
        });
    }

    /// Changes the newest step if it is structural, and charges it again if `change` says so.
    fn change_last(&mut self, change: impl FnOnce(&mut Snapshot, &mut held::Held) -> bool) -> bool {
        let Some((Edit::Snapshot(s), size)) = self.undo.last_mut() else {
            return false;
        };
        let changed = change(s, &mut self.held);
        if changed {
            self.used = self.used - *size + SLOT + s.size();
            *size = SLOT + s.size();
        }
        changed
    }

    fn store(&mut self, mut edit: Edit, set_aside: bool) {
        self.pushes += 1;
        // What could be redone goes, or is set aside: the states of those steps are not reached
        // by the steps recorded from here, even where they come to hold the same positions.
        self.forget_saved_from(self.position() + 1);
        self.drop_aside();
        let redo = std::mem::take(&mut self.redo);
        if set_aside {
            self.aside = redo;
        } else {
            for (edit, size) in redo {
                self.drop_step(edit, size);
            }
        }
        // Only once what could be redone is gone: a drawing a pixel step split off that only
        // those steps kept is gone with them.
        if let Edit::Tiles {
            split: split @ Some(_),
            changes,
            left,
            ..
        } = &mut edit
        {
            *left = left_to_history(&self.held, (split, changes));
        }
        let size = edit.size();
        self.used += size;
        self.undo.push((edit, size));
        self.mark_top();
        if self.used > self.budget {
            self.drop_aside();
        }
        let (mut dropped, mut left) = (0, self.used);
        while left > self.budget && dropped + 1 < self.undo.len() {
            left -= self.undo[dropped].1;
            dropped += 1;
        }
        for (edit, size) in self.undo.drain(..dropped).collect::<Vec<_>>() {
            self.drop_step(edit, size);
        }
        self.marks.below = self.marks.below.saturating_add(dropped as u64);
        if dropped > 0 {
            self.forget_saved_below_kept();
        }
    }

    /// Undoes the newest step without keeping it to redo.
    /// Out of line: inlined, it slowed pasting by a fifth through how the code was laid out
    /// (`paste_and_cancel`, 67 against 85 µs), though pasting never plays a step.
    #[inline(never)]
    pub(crate) fn discard_last(
        &mut self,
        doc: &mut Document,
        selection: &mut Option<Arc<Mask>>,
        place: &mut Place,
    ) -> bool {
        self.mark_top();
        self.newest_step_changed();
        match self.undo.pop() {
            Some((mut edit, size)) => {
                edit.let_go(&mut self.held);
                self.used -= size;
                self.pushes -= 1;
                // The step is gone, and its charge with it.
                let _ = edit.apply(doc, selection, place, &self.held);
                self.bring_back_aside();
                true
            }
            None => false,
        }
    }

    pub fn undo(
        &mut self,
        doc: &mut Document,
        selection: &mut Option<Arc<Mask>>,
        place: &mut Place,
    ) -> bool {
        let (from, to) = (&mut self.undo, &mut self.redo);
        Self::step(
            from,
            to,
            (&mut self.held, &mut self.used),
            doc,
            selection,
            place,
        )
    }

    pub fn redo(
        &mut self,
        doc: &mut Document,
        selection: &mut Option<Arc<Mask>>,
        place: &mut Place,
    ) -> bool {
        let (from, to) = (&mut self.redo, &mut self.undo);
        Self::step(
            from,
            to,
            (&mut self.held, &mut self.used),
            doc,
            selection,
            place,
        )
    }

    /// Moves the newest step from one list to the other, undoing or redoing it. A structural
    /// step keeps other drawings afterwards, and is charged again for them.
    fn step(
        from: &mut Vec<(Edit, usize)>,
        to: &mut Vec<(Edit, usize)>,
        (held, used): (&mut held::Held, &mut usize),
        doc: &mut Document,
        selection: &mut Option<Arc<Mask>>,
        place: &mut Place,
    ) -> bool {
        let Some((mut edit, size)) = from.pop() else {
            return false;
        };
        let size = match &mut edit {
            Edit::Snapshot(s) => {
                Self::play_structural(s, size, (held, used), doc, selection, place)
            }
            Edit::TileImages { .. } => edit.play_tile_images(size, (held, used), doc),
            // Named, so that a pixel step's arm holds only what it plays: it is nearly every step.
            Edit::Tiles { .. } => {
                let change = edit.play_tiles(doc, selection, place, held);
                Self::charge(size, used, change)
            }
            Edit::Move { .. } | Edit::Selection(_) => {
                let change = edit.apply(doc, selection, place, held);
                Self::charge(size, used, change)
            }
        };
        to.push((edit, size));
        true
    }

    /// Charges a step played by `change` more or less, and says what it is charged now.
    #[inline(always)]
    fn charge(size: usize, used: &mut usize, change: isize) -> usize {
        // A step never gives back more than it was charged, which the random checks test in
        // debug builds. Wrapping keeps undo as quick as it was.
        debug_assert!(size.checked_add_signed(change).is_some());
        *used = used.wrapping_add_signed(change);
        size.wrapping_add_signed(change)
    }

    /// Plays a structural step back and charges it again for what it keeps afterwards. Out of
    /// line so that undoing and redoing pixel steps stays as quick as it was.
    #[inline(never)]
    fn play_structural(
        s: &mut Snapshot,
        size: usize,
        (held, used): (&mut held::Held, &mut usize),
        doc: &mut Document,
        selection: &mut Option<Arc<Mask>>,
        place: &mut Place,
    ) -> usize {
        s.let_go(held);
        s.swap_with(doc, place, selection);
        s.take_on(held);
        *used = *used - size + SLOT + s.size();
        SLOT + s.size()
    }
}
