//! Rewind: the undo side of a document's history, kept on disk so that it survives closing.
//! The design, its file layout and its rules are in `docs/rewind.md`.

pub mod compact;
pub mod frame;
pub mod hash;
pub mod payload;
pub mod session;
pub mod store;
pub mod write;

pub use frame::Head;
pub use hash::hash128;
pub use session::{attach, gather, plan, Attach, Chain, Gathered, Plan};
pub use store::Store;
pub use write::{persist, Persist, Step, Written};

use std::fmt;

/// What Rewind keeps, at most.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budgets {
    /// One project's files.
    pub project: u64,
    /// What a project's log is cut to once it passes `project`, so it is not rewritten at
    /// every save.
    pub keep: u64,
    /// Every project together.
    pub store: u64,
}

impl Default for Budgets {
    fn default() -> Self {
        Budgets {
            project: 256 << 20,
            keep: 192 << 20,
            store: 2 << 30,
        }
    }
}

#[derive(Debug)]
pub enum RewindError {
    Io(std::io::Error),
    /// Something in the store does not read back as it was written.
    Damaged(&'static str),
    /// The project's history changed since this session read or wrote it.
    Changed,
    /// Another Pikslar is writing this project's history.
    InUse,
}

impl fmt::Display for RewindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RewindError::Io(e) => e.fmt(f),
            RewindError::Damaged(what) => f.write_str(what),
            RewindError::Changed => f.write_str("its history was changed by another Pikslar"),
            RewindError::InUse => f.write_str("another Pikslar is writing its history"),
        }
    }
}

impl std::error::Error for RewindError {}

impl From<std::io::Error> for RewindError {
    fn from(e: std::io::Error) -> Self {
        RewindError::Io(e)
    }
}

impl From<pikslar_format::FormatError> for RewindError {
    fn from(e: pikslar_format::FormatError) -> Self {
        match e {
            pikslar_format::FormatError::Io(e) => RewindError::Io(e),
            _ => RewindError::Damaged("a stored step does not read"),
        }
    }
}

/// A new random document id: the system's random hash keys, the time and the process, mixed.
pub fn new_document_id() -> u128 {
    use std::hash::{BuildHasher, Hasher};
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let half = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(time);
        h.write_u32(std::process::id());
        h.write_u64(salt);
        h.finish()
    };
    (u128::from(half(1)) << 64) | u128::from(half(2))
}

/// Now, in Unix seconds.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
