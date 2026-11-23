//! Identities that never come to name another object: a frame's key and persisted id, and the
//! highest persisted layer and frame ids a document has given. Keys are not saved; the ids and
//! floors are, in the project file's `Nids` section. None of them is part of what a document
//! is, so two documents that hold the same work are equal. See `docs/stable-ids.md`.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

/// Which frame this is for as long as the program runs, wherever the frame moves. No two
/// frames ever have the same key, in any document; a frame opened from a file gets a new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameKey(u64);

impl FrameKey {
    /// A key no frame has had before.
    fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        FrameKey(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

/// The largest persisted frame id. Ids above it are refused when read.
pub const MAX_FRAME_ID: u32 = 1 << 31;

/// One frame: its timing, the key that tells it apart in this run of the program, and the id
/// that tells it apart in its document, which is saved.
#[derive(Clone, Copy, Debug)]
pub struct FrameInfo {
    pub duration_ms: u32,
    key: FrameKey,
    /// 1 to [`MAX_FRAME_ID`], distinct within one document; 0 until a document numbers it.
    id: u32,
}

impl FrameInfo {
    /// A new frame, with a key of its own and no id yet: a document gives it one.
    pub fn new(duration_ms: u32) -> Self {
        FrameInfo {
            duration_ms,
            key: FrameKey::fresh(),
            id: 0,
        }
    }

    pub fn key(&self) -> FrameKey {
        self.key
    }

    /// The frame's persisted id in its document, kept across saving and opening.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// A new frame with this one's timing and the given id, for a copy or a frame read again
    /// from a file.
    pub(crate) fn renewed(self, id: u32) -> Self {
        FrameInfo {
            id,
            ..FrameInfo::new(self.duration_ms)
        }
    }

    pub(crate) fn set_id(&mut self, id: u32) {
        self.id = id;
    }

    /// The same timing and the same frame: what history compares, so that undo puts back
    /// the order of frames that look alike.
    pub(crate) fn same(&self, other: &FrameInfo) -> bool {
        self.duration_ms == other.duration_ms && self.key == other.key
    }
}

/// Compares the timing. The keys only tell frames apart, as layers' session ids do.
impl PartialEq for FrameInfo {
    fn eq(&self, other: &Self) -> bool {
        self.duration_ms == other.duration_ms
    }
}

impl Eq for FrameInfo {}

/// The highest persisted layer or frame id a document has given or opened with while it has
/// been open. It never goes down, and undo leaves it alone, so an id is never given twice in a
/// session. Saved when it is above the ids in use, so reopening does not lower it.
#[derive(Clone, Copy, Debug, Default)]
pub struct IdFloor(u32);

impl IdFloor {
    pub fn get(self) -> u32 {
        self.0
    }

    pub(crate) fn raise(&mut self, to: u32) {
        self.0 = self.0.max(to);
    }
}

/// Not part of what the document is: documents that hold the same work are equal.
impl PartialEq for IdFloor {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for IdFloor {}

/// The highest color id any script run has seen in a table or a copy of it. Copies share one
/// floor, so undo, which puts an older copy back, never lowers it. Not saved, and not compared.
#[derive(Clone, Debug, Default)]
pub struct SharedFloor(Arc<AtomicU32>);

impl SharedFloor {
    pub fn get(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }

    pub(crate) fn raise(&self, to: u32) {
        self.0.fetch_max(to, Ordering::Relaxed);
    }

    /// True if both are the floor of copies of one table.
    #[cfg(test)]
    pub(crate) fn shared_with(&self, other: &SharedFloor) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Not part of what the table is.
impl PartialEq for SharedFloor {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for SharedFloor {}

/// Which saved document this is, so Rewind can find the history kept for it. Random, given
/// once and kept by every save. Not part of what the document is: documents that hold the same
/// work are equal whatever their ids, and no undo step holds or changes it.
#[derive(Clone, Copy, Debug, Default)]
pub struct DocumentId(Option<u128>);

impl DocumentId {
    pub fn new(id: u128) -> Self {
        DocumentId(Some(id))
    }

    pub fn get(self) -> Option<u128> {
        self.0
    }
}

impl PartialEq for DocumentId {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for DocumentId {}
