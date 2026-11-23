//! A document's history chain as one session knows it, and the two things the program asks of
//! it: which steps to write at a save, and the next older step when undo reaches the bottom.

use crate::frame::Head;
use crate::write::{Encoded, Link, Written};
use crate::{payload, RewindError, Store};
use pikslar_core::{Editor, OlderStep, RewindRefusal};
use std::collections::BTreeMap;

/// What this session knows of a project's stored chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chain {
    pub head: Head,
    /// Where steps end in the log, by position: the tip, every step written in this session,
    /// and the step below each one read.
    pub ends: BTreeMap<u64, u64>,
}

impl Chain {
    pub fn attached(head: Head) -> Self {
        Chain {
            ends: BTreeMap::from([(head.tip, head.tip_end)]),
            head,
        }
    }

    /// The chain a write left, known from it alone.
    pub fn written(written: &Written) -> Self {
        let mut chain = Chain::attached(written.head);
        chain.ends.extend(written.ends.iter().copied());
        chain
    }

    /// Takes on what a write left.
    pub fn take(&mut self, written: &Written) {
        if written.rewritten || written.head.gen != self.head.gen {
            *self = Chain::written(written);
            return;
        }
        self.head = written.head;
        let tip = self.head.tip;
        self.ends.retain(|&pos, _| pos <= tip);
        self.ends.extend(written.ends.iter().copied());
        self.ends.insert(tip, self.head.tip_end);
    }

    /// True if the step at `pos` is kept and its place in the log is known.
    pub fn has_step(&self, pos: u64) -> bool {
        pos > self.head.base && pos <= self.head.tip && self.ends.contains_key(&pos)
    }

    /// Reads the step at `pos`, the newest the editor is missing. `room` is the most tiles a
    /// pixel step may name: [`Editor::rewind_tile_room`].
    pub fn read_older(
        &mut self,
        store: &Store,
        pos: u64,
        room: usize,
    ) -> Result<OlderStep, RewindError> {
        let Some(&end) = self.ends.get(&pos) else {
            return Err(RewindError::Damaged("no stored step is kept there"));
        };
        let (step, prev_end) = store.read_step(&self.head, end, pos, room)?;
        self.ends.insert(pos - 1, prev_end);
        Ok(step)
    }
}

/// What opening found.
#[derive(Debug, PartialEq, Eq)]
pub enum Attach {
    Attached(Chain),
    NoHistory,
    /// History is kept for this id, but for a file that is not this one now.
    Changed,
    Damaged(String),
}

/// Looks for history for the file just opened: only its head is read, and the length of the
/// log it names, which must reach the tip.
pub fn attach(store: &Store, id: u128, print: u128) -> Attach {
    match store.read_head(id) {
        Ok(None) => Attach::NoHistory,
        Ok(Some(head)) if head.print == print => match log_reaches(store, &head) {
            true => Attach::Attached(Chain::attached(head)),
            false => Attach::Damaged("the Rewind log is shorter than its head says".into()),
        },
        Ok(Some(_)) => Attach::Changed,
        Err(e) => Attach::Damaged(e.to_string()),
    }
}

/// True if the head names no record, or its log is long enough to hold the tip.
fn log_reaches(store: &Store, head: &Head) -> bool {
    let log = std::fs::metadata(store.log_path(head.id, head.gen));
    head.tip_end == crate::frame::LOG_START || log.is_ok_and(|m| m.len() >= head.tip_end)
}

/// The steps a save writes, and how they join what is kept.
#[derive(Debug)]
pub struct Plan {
    pub link: Link,
    /// Oldest first.
    pub records: Vec<Encoded>,
}

/// The steps a save keeps, taken from the editor: pixel steps not yet encoded, sharing the
/// editor's tiles, so encoding them can be left to another thread.
pub struct Gathered {
    /// The kept step the new ones follow, and where it ends in the log.
    after: Option<(u64, u64)>,
    /// The oldest state in memory, where a new chain starts.
    below: u64,
    /// Newest first.
    steps: Vec<(u64, payload::Taken)>,
}

impl Gathered {
    /// The lowest position gathered, or `u64::MAX` if none.
    pub fn lowest(&self) -> u64 {
        self.steps.last().map_or(u64::MAX, |(pos, _)| *pos)
    }

    /// Encodes the steps, at most `keep` bytes of them: past that, the chain starts at the
    /// oldest step that fits.
    pub fn encode(self, keep: u64) -> Plan {
        let (mut records, mut total, mut cut) = (Vec::new(), 0u64, None);
        for (pos, step) in self.steps {
            let (kind, payload) = step.encode();
            total += payload.len() as u64;
            if total > keep {
                cut = Some(pos);
                break;
            }
            records.push(Encoded { pos, kind, payload });
        }
        records.reverse();
        let link = match (cut, self.after) {
            (Some(base), _) => Link::Fresh { base },
            (None, Some((pos, end))) => Link::After { pos, end },
            (None, None) => Link::Fresh { base: self.below },
        };
        Plan { link, records }
    }
}

/// Takes the steps a save keeps so the stored chain ends at the document as it is: the steps
/// above what is stored and still true, or a new chain from the oldest step in memory.
pub fn gather(editor: &mut Editor, chain: Option<&Chain>) -> Result<Gathered, RewindRefusal> {
    let m = editor.rewind_marks();
    let after = chain.and_then(|c| {
        let common = c.head.tip.min(m.unchanged_up_to).min(m.depth);
        let usable = common >= m.below && common >= c.head.base;
        let end = c.ends.get(&common).copied().filter(|_| usable)?;
        Some((common, end))
    });
    let to = after.map_or(m.below, |(pos, _)| pos);
    let mut steps = Vec::new();
    editor.walk_back(to, &mut |pos, view| {
        steps.push((pos, payload::Taken::of(view)))
    })?;
    Ok(Gathered {
        after,
        below: m.below,
        steps,
    })
}

/// [`gather`] and [`Gathered::encode`] at once.
pub fn plan(editor: &mut Editor, chain: Option<&Chain>, keep: u64) -> Result<Plan, RewindRefusal> {
    Ok(gather(editor, chain)?.encode(keep))
}
