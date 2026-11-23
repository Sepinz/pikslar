//! A layer's cels and the holds between them.
//!
//! This file is the only code that knows how cels are stored. Everything else reads a [`Cel`] in
//! canvas coordinates and changes one through the methods here, so the storage can change alone.
//!
//! Other crates read through the API:
//!
//! ```
//! use pikslar_core::{ColorMode, Document};
//! let doc = Document::new(8, 8, ColorMode::Rgba).unwrap();
//! assert_eq!(doc.layers()[0].frame_count(), 1);
//! assert!(doc.layers()[0].cel(0).is_blank());
//! ```
//!
//! and cannot reach the storage:
//!
//! ```compile_fail
//! use pikslar_core::{ColorMode, Document};
//! let doc = Document::new(8, 8, ColorMode::Rgba).unwrap();
//! let _ = doc.layers()[0].cels.len();
//! ```
//!
//! ```compile_fail
//! use pikslar_core::{ColorMode, Document};
//! let doc = Document::new(8, 8, ColorMode::Rgba).unwrap();
//! let _ = doc.layers()[0].links.len();
//! ```
//!
//! ```compile_fail
//! use pikslar_core::{ColorMode, Document};
//! let doc = Document::new(8, 8, ColorMode::Rgba).unwrap();
//! let _ = doc.layers.len();
//! ```
//!
//! A layer inside a document keeps one cel per frame, and a file cannot choose its session id:
//!
//! ```compile_fail
//! use pikslar_core::{Cel, ColorMode, Document, Grid};
//! let mut doc = Document::new(8, 8, ColorMode::Rgba).unwrap();
//! doc.layers_mut()[0].push_cel(Cel::new(Grid::new(8, 8)));
//! ```
//!
//! ```compile_fail
//! use pikslar_core::{ColorMode, Document};
//! let mut doc = Document::new(8, 8, ColorMode::Rgba).unwrap();
//! doc.layers_mut()[0].id = 1;
//! ```

mod cel;
mod log;
mod turn;
mod write;

pub(crate) use cel::kept_bytes;
pub use cel::{Cel, CelReader, OFFSET_LIMIT};
pub(crate) use log::CelEntry;
pub use turn::Turn;
pub(crate) use write::CanvasWriter;

use crate::document::{BlendMode, ColorMode, LimitError};
use crate::grid::Cell;
use crate::image::{Image, TileKey};
use crate::tileset::TilesetId;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};

/// The drawing a write split away from the frames or history that shared it. Undoing the write
/// shows that drawing again, if it is still kept and shows the same tiles, so the frames share
/// one drawing as they did. Held weakly, it keeps nothing alive.
pub(crate) type Split = Option<Weak<Image>>;

/// A weak hold on a drawing a write is about to split from whatever else shares it.
fn split_off(image: &Arc<Image>) -> Split {
    (Arc::strong_count(image) > 1).then(|| Arc::downgrade(image))
}

/// Moves one item to a new place, shifting only the items between the two places, as removing
/// it and inserting it again would leave them.
pub(crate) fn move_within<T>(list: &mut [T], from: usize, to: usize) {
    if from < to {
        list[from..=to].rotate_left(1);
    } else if to < from {
        list[to..=from].rotate_right(1);
    }
}

/// The number a layer will keep in saved files. It tells layers apart within one document only.
///
/// A type of its own, because a number read from a file must never be taken for a session id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct PersistedLayerId(pub(crate) u32);

impl PersistedLayerId {
    /// The highest number a layer may have. Files with higher ones are refused, and a document
    /// never gives one out, so every save can be opened again.
    pub const MAX: u32 = 1 << 24;

    /// The number as a file stores it.
    pub fn get(self) -> u32 {
        self.0
    }
}

/// What a layer holds. Only a paint layer can be drawn on with the pixel tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayerKind {
    #[default]
    Paint,
    /// A picture to draw from: shown on the screen, left out of everything made from the file.
    Reference,
    /// Holds the layers below it that are one deeper. It has no pixels of its own.
    Group,
    /// Places the tiles of one tileset. Its cels hold [`crate::tilemap::MapCell`] values in
    /// cell coordinates, not pixels, and its colour mode is its tileset's.
    Tilemap(TilesetId),
}

#[derive(Clone, Debug)]
pub struct Layer {
    /// Which layer this is for the length of the session, wherever it is moved. Not saved.
    id: u32,
    /// Given when the layer joins a document, and kept by files from version 3 on.
    persisted_id: PersistedLayerId,
    pub name: String,
    pub visible: bool,
    /// Refuses drawing. Like `visible`, how the artist works on the file: saved, never undone.
    pub locked: bool,
    pub kind: LayerKind,
    /// How many groups this layer is in, from 0 to [`crate::groups::MAX_GROUP_DEPTH`]. A group's
    /// contents are the layers straight below it that are deeper than it.
    pub depth: u8,
    /// A group shows only its own row in the layer list. Like `visible`, never undone.
    pub folded: bool,
    pub opacity: u8,
    pub blend: BlendMode,
    pub mode: ColorMode,
    /// One per frame. Frames with one hold id show one drawing.
    ///
    /// Shared with undo steps until it changes, so a step that leaves the cels alone costs
    /// nothing per frame.
    cels: Arc<Vec<Cel>>,
    /// While a step of history is made, what each change to `cels` replaced.
    log: log::CelLog,
}

/// Compares what a layer is. The ids only tell layers apart.
impl PartialEq for Layer {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.visible == other.visible
            && self.locked == other.locked
            && self.kind == other.kind
            && self.depth == other.depth
            && self.folded == other.folded
            && self.opacity == other.opacity
            && self.blend == other.blend
            && self.mode == other.mode
            && self.cels == other.cels
    }
}

impl Eq for Layer {}

impl Layer {
    /// Which layer this is for the length of the session.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Which layer this is within its document, across saves.
    pub fn persisted_id(&self) -> PersistedLayerId {
        self.persisted_id
    }

    pub(crate) fn set_persisted_id(&mut self, id: PersistedLayerId) {
        self.persisted_id = id;
    }

    /// A copy under another name that shares every drawing, with a session id of its own and
    /// no history. Its persisted id and holds are the document's to give.
    pub(crate) fn duplicate(&self, name: String) -> Layer {
        Layer {
            id: Layer::next_id(),
            name,
            log: log::CelLog::default(),
            ..self.clone()
        }
    }

    /// A number no other layer in this session has.
    fn next_id() -> u32 {
        static NEXT: AtomicU32 = AtomicU32::new(1);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    /// A visible, opaque layer with no frames yet.
    pub fn new(name: String, mode: ColorMode) -> Self {
        Layer {
            id: Layer::next_id(),
            persisted_id: PersistedLayerId::default(),
            name,
            visible: true,
            locked: false,
            kind: LayerKind::Paint,
            depth: 0,
            folded: false,
            opacity: 255,
            blend: BlendMode::Normal,
            mode,
            cels: Arc::new(Vec::new()),
            log: log::CelLog::default(),
        }
    }

    /// True when the layer itself is part of the picture: shown, and a paint or tilemap layer.
    /// The groups it is in can hide it too: [`crate::Document::picture_layers`] asks them.
    pub fn in_picture(&self) -> bool {
        self.visible && matches!(self.kind, LayerKind::Paint | LayerKind::Tilemap(_))
    }

    /// The tileset this layer places, if it is a tilemap layer.
    pub fn tileset(&self) -> Option<TilesetId> {
        match self.kind {
            LayerKind::Tilemap(id) => Some(id),
            LayerKind::Paint | LayerKind::Reference | LayerKind::Group => None,
        }
    }

    /// True for a layer whose cels hold map cells rather than pixels.
    pub fn is_tilemap(&self) -> bool {
        self.tileset().is_some()
    }

    pub fn frame_count(&self) -> usize {
        self.cels.len()
    }

    /// The cel of a frame that exists.
    #[inline]
    pub fn cel(&self, frame: usize) -> &Cel {
        &self.cels[frame]
    }

    pub fn cels(&self) -> &[Cel] {
        &self.cels
    }

    /// The cels to change, copied first if an undo step still shares the list.
    fn cels_mut(&mut self) -> &mut Vec<Cel> {
        Arc::make_mut(&mut self.cels)
    }

    /// Which hold a frame belongs to, or 0 for none and for a frame that does not exist.
    pub fn link(&self, frame: usize) -> u32 {
        self.cels.get(frame).map_or(0, Cel::link)
    }

    /// Every frame's hold, in frame order.
    pub fn links(&self) -> impl Iterator<Item = u32> + '_ {
        self.cels.iter().map(Cel::link)
    }

    /// This layer with these cels, one per frame and in no hold, before it joins a document.
    pub fn with_cels(mut self, mut cels: Vec<Cel>) -> Self {
        cels.iter_mut().for_each(|cel| cel.link = 0);
        self.cels = Arc::new(cels);
        self
    }

    /// This layer with these holds, before it joins a document.
    pub fn with_holds(mut self, ids: &[u32]) -> Result<Self, LimitError> {
        self.set_holds(ids, 0)?;
        Ok(self)
    }

    /// Adds a frame at the end, in no hold.
    pub(crate) fn push_cel(&mut self, mut cel: Cel) {
        cel.link = 0;
        let at = self.cels.len();
        self.note(CelEntry::Inserted { at, count: 1 });
        self.cels_mut().push(cel);
    }

    /// Replaces one frame's cel. Its hold, if any, is left as it is.
    pub(crate) fn put_cel(&mut self, frame: usize, mut cel: Cel) -> Result<(), LimitError> {
        self.keep(frame);
        let slot = self
            .cels_mut()
            .get_mut(frame)
            .ok_or(LimitError::NoSuchItem)?;
        cel.link = slot.link;
        *slot = cel;
        Ok(())
    }

    /// Sets every frame's hold from ids of any size, then makes each hold really one drawing.
    /// The holds are numbered from just after `used`, which no other layer's holds go beyond.
    pub(crate) fn set_holds(&mut self, ids: &[u32], used: u32) -> Result<(), LimitError> {
        if ids.len() != self.cels.len() {
            return Err(LimitError::NoSuchItem);
        }
        let mut links = ids.to_vec();
        crate::animation::tidy_links(&mut links);
        self.keep_list();
        for (cel, id) in self.cels_mut().iter_mut().zip(links) {
            cel.link = if id == 0 { 0 } else { id + used };
        }
        self.share_held_cels();
        Ok(())
    }

    /// Moves this layer's holds to numbers after `used`, and returns the last number now in use.
    pub(crate) fn shift_holds(&mut self, used: u32) -> u32 {
        self.keep_list();
        for cel in self.cels_mut().iter_mut().filter(|cel| cel.link != 0) {
            cel.link += used;
        }
        self.links().max().unwrap_or(0).max(used)
    }

    /// Makes every frame of a hold share one drawing again, after cels were rebuilt.
    pub fn share_held_cels(&mut self) {
        let mut first: HashMap<u32, usize> = HashMap::new();
        self.keep_list();
        let cels = self.cels_mut();
        for frame in 0..cels.len() {
            let id = cels[frame].link;
            if id != 0 {
                let owner = cels[*first.entry(id).or_insert(frame)].clone();
                cels[frame].show_drawing_of(&owner);
            }
        }
    }

    pub(crate) fn insert_frame(&mut self, at: usize, mut cel: Cel, link: u32) {
        cel.link = link;
        self.note(CelEntry::Inserted { at, count: 1 });
        self.cels_mut().insert(at, cel);
    }

    pub(crate) fn remove_frames(&mut self, range: RangeInclusive<usize>) {
        let at = *range.start();
        let cels: Vec<Cel> = self.cels_mut().drain(range).collect();
        self.note(CelEntry::Removed { at, cels });
    }

    pub(crate) fn move_frame(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        self.note(CelEntry::Moved { from, to });
        move_within(self.cels_mut(), from, to);
    }

    /// Makes a frame part of a hold, showing the whole cel of another frame, wherever it is.
    pub(crate) fn hold_frame(&mut self, frame: usize, source: usize, id: u32) {
        self.keep(frame);
        let cels = self.cels_mut();
        let mut held = cels[source].clone();
        held.link = id;
        cels[frame] = held;
    }

    /// Sets every frame's link id as given, logged as one change of the whole list.
    pub(crate) fn put_links(&mut self, ids: &[u32]) {
        debug_assert_eq!(ids.len(), self.cels.len());
        self.keep_list();
        for (cel, &id) in self.cels_mut().iter_mut().zip(ids) {
            cel.link = id;
        }
    }

    pub(crate) fn set_link(&mut self, frame: usize, id: u32) {
        self.keep(frame);
        self.cels_mut()[frame].link = id;
    }

    /// Swaps in rebuilt drawings, one per frame. Positions and holds stay.
    pub(crate) fn replace_images(&mut self, images: Vec<Image>) {
        debug_assert_eq!(images.len(), self.cels.len());
        self.keep_list();
        for (cel, image) in self.cels_mut().iter_mut().zip(images) {
            cel.image = Arc::new(image);
        }
    }

    /// Edits a frame's image where it is by first letting go of it in the rest of the hold, so
    /// that a stroke on a held frame costs no copy of the table, then shows the result there.
    /// Returns the drawing the edit split off, if something outside the hold shares it.
    fn edit_held_image(&mut self, frame: usize, edit: impl FnOnce(&mut Image)) -> Split {
        if self.logging() {
            self.members(frame)
                .into_iter()
                .for_each(|f| self.keep(f as usize));
        }
        let cels = self.cels_mut();
        let id = cels[frame].link;
        let is_other = |(index, cel): &(usize, &mut Cel)| *index != frame && cel.link == id;
        let nothing = Arc::new(Image::blank());
        for (_, cel) in cels.iter_mut().enumerate().filter(is_other) {
            cel.image = nothing.clone();
        }
        let before = split_off(&cels[frame].image);
        edit(cels[frame].image_mut());
        let edited = cels[frame].image.clone();
        for (_, cel) in cels.iter_mut().enumerate().filter(is_other) {
            cel.image = edited.clone();
        }
        before
    }

    /// Swaps tiles into a frame's image and leaves what they replaced in `changes`, then shows
    /// the result in the rest of the frame's hold. `split` goes the same way: in, the drawing
    /// the frame showed before the write this undoes; out, the drawing this write split.
    pub(crate) fn swap_tiles(
        &mut self,
        frame: usize,
        changes: &mut [(TileKey, Cell)],
        split: &mut Split,
    ) {
        // Nothing to write must not split an image that frames share, or saved files would grow.
        let Some(cel) = self.cels.get(frame).filter(|_| !changes.is_empty()) else {
            return;
        };
        let before = if cel.link == 0 {
            let before = split_off(&cel.image);
            self.keep(frame);
            self.cels_mut()[frame].image_mut().swap_cells(changes);
            before
        } else {
            self.edit_held_image(frame, |image| image.swap_cells(changes))
        };
        // Mostly there is nothing to go back to and nothing split, and the step is left alone.
        if before.is_some() || split.is_some() {
            if let Some(old) = std::mem::replace(split, before).and_then(|old| old.upgrade()) {
                self.show_again(frame, old);
            }
        }
    }

    /// Shows a drawing a write split off again in the frame and its hold, if the frame's
    /// drawing holds the same tiles. Only which copy is shown changes, so nothing is logged.
    fn show_again(&mut self, frame: usize, old: Arc<Image>) {
        let link = self.link(frame);
        if !self.cels[frame].image.same_tiles(&old) {
            return;
        }
        let cels = self.cels_mut();
        let shown =
            |(index, cel): &(usize, &mut Cel)| *index == frame || (link != 0 && cel.link == link);
        for (_, cel) in cels.iter_mut().enumerate().filter(shown) {
            cel.image = old.clone();
        }
    }

    /// The drawing a frame shows, if the frame exists.
    pub(crate) fn drawing(&self, frame: usize) -> Option<&Arc<Image>> {
        self.cels.get(frame).map(|cel| &cel.image)
    }

    /// Lets go of a frame's drawing while a cel of another layer in its link edits it, so the
    /// edit copies nothing for this frame. Logged first inside a step.
    pub(crate) fn let_go_of_drawing(&mut self, frame: usize) {
        self.keep(frame);
        if let Some(cel) = self.cels_mut().get_mut(frame) {
            cel.image = Arc::new(Image::blank());
        }
    }

    /// Shows a drawing in a frame after a cel of another layer in its link edited it. Only
    /// which copy is shown changes; [`Layer::let_go_of_drawing`] logged the frame.
    pub(crate) fn show_drawing(&mut self, frame: usize, image: &Arc<Image>) {
        if let Some(cel) = self.cels_mut().get_mut(frame) {
            cel.image = Arc::clone(image);
        }
    }

    /// The frames that show this frame's drawing and so move with it: its hold, or itself.
    pub(crate) fn members(&self, frame: usize) -> Vec<u32> {
        let id = self.link(frame);
        if id == 0 {
            return if frame < self.cels.len() {
                vec![frame as u32]
            } else {
                Vec::new()
            };
        }
        (0..self.cels.len() as u32)
            .filter(|&f| self.cels[f as usize].link == id)
            .collect()
    }

    /// The part of a move every one of these frames can make and stay in range.
    pub(crate) fn possible_move(&self, frames: &[u32], dx: i32, dy: i32) -> (i32, i32) {
        frames
            .iter()
            .filter_map(|&f| self.cels.get(f as usize))
            .fold((dx, dy), |(dx, dy), cel| {
                let ((x0, x1), (y0, y1)) = cel.room_to_move();
                (dx.clamp(x0, x1), dy.clamp(y0, y1))
            })
    }

    /// Moves these frames' cels. No pixel is copied: only the positions change.
    pub(crate) fn move_cels(&mut self, frames: &[u32], dx: i32, dy: i32) {
        frames.iter().for_each(|&f| self.keep(f as usize));
        let cels = self.cels_mut();
        for &f in frames {
            if let Some(cel) = cels.get_mut(f as usize) {
                cel.move_by(dx, dy);
            }
        }
    }

    /// What storing this layer's list of cels costs in an undo step.
    pub(crate) fn record_bytes(&self) -> usize {
        self.cels.len() * size_of::<Cel>() + 96
    }
}

#[cfg(test)]
mod tests;
