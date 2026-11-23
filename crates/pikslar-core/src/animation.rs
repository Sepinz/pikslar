//! Frames over time: holds, tags, playback order and onion skin neighbours.
//!
//! A hold is one drawing used in several frames of a layer. Cels are shared until edited
//! anyway, so what a hold adds is intent: an edit to any of its frames reaches them all.

use crate::document::{Document, LimitError, MAX_FRAMES};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_TAGS: usize = 256;
pub const MAX_TAG_NAME: usize = 32;
pub const MIN_DURATION_MS: u32 = 10;
pub const MAX_DURATION_MS: u32 = 60_000;
/// The most times a tag can be set to play; 0 is forever.
pub const MAX_TAG_REPEAT: u16 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Reverse,
    PingPong,
}

impl Direction {
    pub fn next(self) -> Self {
        match self {
            Direction::Forward => Direction::PingPong,
            Direction::PingPong => Direction::Reverse,
            Direction::Reverse => Direction::Forward,
        }
    }
}

/// Names one tag for as long as the program runs, whatever is done to the tags around it:
/// renamed, moved by frame edits, or removed and brought back by undo, it keeps its id. Never
/// saved: a project read from a file gets new ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TagId(u64);

impl TagId {
    /// An id no other tag in this run has.
    pub fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        TagId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// A named run of frames, one animation of the several a sprite may hold.
#[derive(Clone, Debug)]
pub struct Tag {
    pub name: String,
    pub from: usize,
    pub to: usize,
    pub direction: Direction,
    /// How many passes it plays before stopping, 0 for forever. A ping-pong's pass is one
    /// way, so 2 goes there and back.
    pub repeat: u16,
    /// Who it is, not what it is: tags alike in everything else are equal whatever their ids.
    pub id: TagId,
}

impl PartialEq for Tag {
    fn eq(&self, other: &Self) -> bool {
        (&self.name, self.from, self.to, self.direction, self.repeat)
            == (
                &other.name,
                other.from,
                other.to,
                other.direction,
                other.repeat,
            )
    }
}

impl Eq for Tag {}

impl Tag {
    /// A frame leaving a tag shrinks it and one placed inside grows it. Others shift.
    fn after_move(&mut self, from: usize, to: usize) {
        let member = (self.from..=self.to).contains(&from);
        if member && (self.from..=self.to).contains(&to) {
            return;
        }
        if member && self.from == self.to {
            (self.from, self.to) = (to, to);
            return;
        }
        if from < self.from {
            self.from -= 1;
        }
        if from <= self.to {
            self.to -= 1;
        }
        if to <= self.from {
            self.from += 1;
            self.to += 1;
        } else if to <= self.to {
            self.to += 1;
        }
    }
}

pub use crate::links::tidy_links;

/// A stretch of frames that plays on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub from: usize,
    pub to: usize,
    pub direction: Direction,
}

impl Span {
    pub fn whole(frames: usize) -> Span {
        Span {
            from: 0,
            to: frames.saturating_sub(1),
            direction: Direction::Forward,
        }
    }

    pub fn contains(&self, frame: usize) -> bool {
        (self.from..=self.to).contains(&frame)
    }

    /// The frame after `frame`. `backwards` is which way a ping-pong is currently going.
    pub fn step(&self, frame: usize, backwards: &mut bool) -> usize {
        if !self.contains(frame) {
            *backwards = false;
            return match self.direction {
                Direction::Reverse => self.to,
                _ => self.from,
            };
        }
        match self.direction {
            Direction::Forward if frame == self.to => self.from,
            Direction::Forward => frame + 1,
            Direction::Reverse if frame == self.from => self.to,
            Direction::Reverse => frame - 1,
            Direction::PingPong if self.from == self.to => frame,
            Direction::PingPong => {
                if *backwards && frame == self.from {
                    *backwards = false;
                } else if !*backwards && frame == self.to {
                    *backwards = true;
                }
                if *backwards {
                    frame - 1
                } else {
                    frame + 1
                }
            }
        }
    }
}

/// Where playback is in a span that plays a set number of times.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Playhead {
    /// Which way a ping-pong is going.
    pub backwards: bool,
    /// Passes finished so far.
    pub passes: u16,
    /// Whether a step was taken yet: playback started on a span's last frame begins a pass
    /// there, unless that frame is the whole span.
    pub moved: bool,
}

impl Span {
    /// The frame after `frame`, or `None` when the span has played `repeat` passes; 0 plays
    /// forever.
    pub fn step_counted(&self, frame: usize, head: &mut Playhead, repeat: u16) -> Option<usize> {
        let single = self.from == self.to;
        let at_end = match self.direction {
            _ if single => true,
            Direction::Forward => frame == self.to,
            Direction::Reverse => frame == self.from,
            Direction::PingPong if head.backwards => frame == self.from,
            Direction::PingPong => frame == self.to,
        };
        if (head.moved || single) && self.contains(frame) && at_end {
            head.passes = head.passes.saturating_add(1);
            if repeat != 0 && head.passes >= repeat {
                return None;
            }
        }
        head.moved = true;
        Some(self.step(frame, &mut head.backwards))
    }
}

/// One neighbouring frame shown faintly. `distance` is negative for earlier frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ghost {
    pub frame: usize,
    pub distance: i32,
}

/// The frames to show around `frame`, farthest first so nearer ones land on top.
///
/// Within a looping span they wrap, because the frame before a loop's first is its last.
pub fn onion_ghosts(
    frame: usize,
    frames: usize,
    before: usize,
    after: usize,
    looping: Option<Span>,
) -> Vec<Ghost> {
    if frame >= frames {
        return Vec::new();
    }
    let (low, high, wraps) = match looping.filter(|s| s.contains(frame) && s.to < frames) {
        Some(span) => (span.from as i64, span.to as i64, true),
        None => (0, frames as i64 - 1, false),
    };
    let length = high - low + 1;
    let at = |distance: i64| -> Option<usize> {
        let raw = frame as i64 + distance;
        let inside = (low..=high).contains(&raw);
        let found = match (inside, wraps) {
            (true, _) => raw,
            (false, true) => low + (raw - low).rem_euclid(length),
            (false, false) => return None,
        };
        (found != frame as i64).then_some(found as usize)
    };
    let (before, after) = (before.min(MAX_FRAMES), after.min(MAX_FRAMES));
    let mut ghosts: Vec<Ghost> = Vec::with_capacity(before + after);
    let reach = before.max(after) as i64;
    let mut add = |distance: i64| {
        if let Some(frame) = at(distance).filter(|f| !ghosts.iter().any(|g| g.frame == *f)) {
            ghosts.push(Ghost {
                frame,
                distance: distance as i32,
            });
        }
    };
    // Near frames claim their place first, so a short loop shows each frame only once.
    for step in 1..=reach {
        if step as usize <= before {
            add(-step);
        }
        if step as usize <= after {
            add(step);
        }
    }
    ghosts.reverse();
    ghosts
}

impl Document {
    /// Moves a frame to a new position, taking all its cels with it.
    pub fn move_frame(&mut self, from: usize, to: usize) -> Result<(), LimitError> {
        if from >= self.frames.len() || to >= self.frames.len() {
            return Err(LimitError::NoSuchItem);
        }
        crate::layer::move_within(&mut self.frames, from, to);
        for l in &mut self.layers {
            l.move_frame(from, to);
        }
        for tag in &mut self.tags {
            tag.after_move(from, to);
        }
        Ok(())
    }

    /// Removes a run of frames in one pass. At least one frame must remain.
    pub fn remove_frames(&mut self, from: usize, to: usize) -> Result<(), LimitError> {
        if from > to || to >= self.frames.len() {
            return Err(LimitError::NoSuchItem);
        }
        if to - from + 1 >= self.frames.len() {
            return Err(LimitError::LastFrame);
        }
        self.frames.drain(from..=to);
        for layer in &mut self.layers {
            layer.remove_frames(from..=to);
        }
        (from..=to).rev().for_each(|i| self.tags_after_remove(i));
        self.tidy_holds();
        Ok(())
    }

    /// Adds a tag and returns its position.
    /// Whether a tag over these frames could be added.
    pub(crate) fn check_new_tag(&self, from: usize, to: usize) -> Result<(), LimitError> {
        if self.tags.len() >= MAX_TAGS {
            return Err(LimitError::TooManyFrames);
        }
        if from > to || to >= self.frames.len() {
            return Err(LimitError::NoSuchItem);
        }
        Ok(())
    }

    pub fn add_tag(&mut self, name: &str, from: usize, to: usize) -> Result<usize, LimitError> {
        self.check_new_tag(from, to)?;
        self.tags.push(Tag {
            name: name.chars().take(MAX_TAG_NAME).collect(),
            from,
            to,
            direction: Direction::Forward,
            repeat: 0,
            id: TagId::fresh(),
        });
        Ok(self.tags.len() - 1)
    }

    pub(crate) fn tags_after_insert(&mut self, at: usize) {
        for tag in &mut self.tags {
            if at <= tag.from {
                tag.from += 1;
                tag.to += 1;
            } else if at <= tag.to + 1 {
                tag.to += 1;
            }
        }
    }

    pub(crate) fn tags_after_remove(&mut self, index: usize) {
        self.tags.retain(|t| !(t.from == index && t.to == index));
        for tag in &mut self.tags {
            if index < tag.from {
                tag.from -= 1;
            }
            if index <= tag.to {
                tag.to -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests;
