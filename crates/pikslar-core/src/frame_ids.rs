//! Persisted frame ids, and the floors a project file keeps for layer and frame ids.
//!
//! Every frame of a document has an id from 1 to [`MAX_FRAME_ID`], distinct within it. A new
//! frame takes one more than the highest id given while the document has been open, so a
//! removed frame's id is never given again; undo and redo bring frames back whole, ids and
//! all. See `docs/stable-ids.md`.

use crate::document::{Document, FrameInfo, LimitError, MAX_FRAMES};
use crate::ids::MAX_FRAME_ID;
use crate::layer::PersistedLayerId;

impl Document {
    /// The highest persisted frame id given while this document has been open.
    pub fn frame_floor(&self) -> u32 {
        self.frame_floor.get()
    }

    /// A new frame with `like`'s timing, a key of its own and an id never given in this
    /// document while it has been open.
    pub(crate) fn new_frame_like(&mut self, like: FrameInfo) -> FrameInfo {
        let id = self.new_frame_id();
        like.renewed(id)
    }

    /// One past the highest frame id given; once those run out, the smallest free one. The
    /// floor is at least every live id (commands and `keep_*` raise it), so this is constant
    /// time until the ids run out, and linear in the frames after.
    fn new_frame_id(&mut self) -> u32 {
        let id = self.fresh_frame_ids(1).first().copied().unwrap_or(1);
        self.frame_floor.raise(id);
        id
    }

    /// `count` ids no frame holds and, while there is room, above every id given. Past
    /// [`MAX_FRAME_ID`] the smallest free ones, found in one pass: there are at most
    /// [`MAX_FRAMES`] frames, so the free ones are among the first `MAX_FRAMES + count`.
    fn fresh_frame_ids(&self, count: usize) -> Vec<u32> {
        let floor = self.frame_floor.get();
        if MAX_FRAME_ID.saturating_sub(floor) as usize >= count {
            return (floor + 1..).take(count).collect();
        }
        let reach = MAX_FRAMES + count;
        let mut taken = vec![false; reach + 1];
        for frame in &self.frames {
            if let Some(slot) = taken.get_mut(frame.id() as usize) {
                *slot = true;
            }
        }
        let free = (1..=reach).filter(|&id| !taken[id]);
        free.take(count).map(|id| id as u32).collect()
    }

    /// Gives the frames, in timeline order, the ids a file saved for them. Refused, changing
    /// nothing, unless there is one per frame and they are distinct, not 0 and not past
    /// [`MAX_FRAME_ID`].
    pub fn keep_frame_ids(&mut self, ids: &[u32]) -> Result<(), LimitError> {
        let some: Vec<Option<u32>> = ids.iter().copied().map(Some).collect();
        self.keep_some_frame_ids(&some)
    }

    /// As [`Document::keep_frame_ids`], for a file that kept ids for some frames only: a frame
    /// with `None` gets a new id above every one given, the kept ones included.
    pub fn keep_some_frame_ids(&mut self, ids: &[Option<u32>]) -> Result<(), LimitError> {
        let mut sorted: Vec<u32> = ids.iter().flatten().copied().collect();
        sorted.sort_unstable();
        let distinct = sorted.windows(2).all(|pair| pair[0] != pair[1]);
        let in_range = sorted.first() != Some(&0) && sorted.last() <= Some(&MAX_FRAME_ID);
        if ids.len() != self.frames.len() || !in_range || !distinct {
            return Err(LimitError::NoSuchItem);
        }
        self.frame_floor.raise(sorted.last().copied().unwrap_or(0));
        for (frame, id) in self.frames.iter_mut().zip(ids) {
            frame.set_id(id.unwrap_or(0));
        }
        let wanted = ids.iter().filter(|id| id.is_none()).count();
        let mut fresh = self.fresh_frame_ids(wanted).into_iter();
        for frame in self.frames.iter_mut().filter(|f| f.id() == 0) {
            frame.set_id(fresh.next().unwrap_or(0));
        }
        let highest = self.frames.iter().map(FrameInfo::id).max();
        self.frame_floor.raise(highest.unwrap_or(0));
        Ok(())
    }

    /// Gives every frame a new id above every id given, for frames a saved list no longer
    /// describes: none of the old ids then names any frame (until the ids run out).
    pub fn renumber_frames(&mut self) {
        let none = vec![None; self.frames.len()];
        // One `None` per frame always fits.
        let _ = self.keep_some_frame_ids(&none);
    }

    /// Raises the frame floor to a value a file saved, at most [`MAX_FRAME_ID`].
    pub fn raise_frame_floor(&mut self, to: u32) {
        self.frame_floor.raise(to.min(MAX_FRAME_ID));
    }

    /// Gives the layer at `index`, from the bottom, a new persisted id above every id given,
    /// for a layer a build that did not keep the floor numbered: its id may have been a removed
    /// layer's. Returns the new id.
    pub fn renumber_layer(&mut self, index: usize) -> Result<u32, LimitError> {
        if index >= self.layers.len() {
            return Err(LimitError::NoSuchItem);
        }
        let id = self.new_persisted_id();
        let layer = self.layers.get_mut(index).ok_or(LimitError::NoSuchItem)?;
        layer.set_persisted_id(id);
        Ok(id.get())
    }

    /// Raises the layer floor to a value a file saved, at most [`PersistedLayerId::MAX`].
    pub fn raise_layer_floor(&mut self, to: u32) {
        self.layer_floor.raise(to.min(PersistedLayerId::MAX));
    }
}
