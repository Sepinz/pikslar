//! Changes to a run of frames at once: duplicating it, moving it, and setting a tag's frames.

use crate::animation::Tag;
use crate::document::{Document, LimitError, MAX_FRAMES};
use std::collections::HashMap;

/// Where a frame goes when the block `from..=to` moves so that its first frame is at `at`.
pub fn after_block_move(frame: usize, (from, to): (usize, usize), at: usize) -> usize {
    let n = to - from + 1;
    if (from..=to).contains(&frame) {
        return at + frame - from;
    }
    // Its place among the frames left behind, then past the block if the block lands before it.
    let rest = if frame > to { frame - n } else { frame };
    if rest < at {
        rest
    } else {
        rest + n
    }
}

impl Tag {
    /// A tag inside the moved block goes with it. Otherwise its frames outside the block keep
    /// it, and a block landing among them makes it wider.
    fn after_block_move(&mut self, block: (usize, usize), at: usize) {
        let inside = |f: &usize| (block.0..=block.1).contains(f);
        let mut kept: Vec<usize> = (self.from..=self.to).filter(|f| !inside(f)).collect();
        if kept.is_empty() {
            kept = (self.from..=self.to).collect();
        }
        let moved = kept.iter().map(|&f| after_block_move(f, block, at));
        let (low, high) = moved.fold((usize::MAX, 0), |(lo, hi), f| (lo.min(f), hi.max(f)));
        (self.from, self.to) = (low, high);
    }
}

impl Document {
    fn check_run(&self, from: usize, to: usize) -> Result<(), LimitError> {
        if from > to || to >= self.frames.len() {
            return Err(LimitError::NoSuchItem);
        }
        Ok(())
    }

    /// Puts a copy of the frames `from..=to` straight after them. Copies share each drawing.
    /// A link reaching past the run grows over the copies, as a single duplicated frame's
    /// does; a link wholly inside it, on any layers, is copied as a link of its own, so
    /// editing a copy leaves the original alone.
    pub fn duplicate_frames(&mut self, from: usize, to: usize) -> Result<(), LimitError> {
        self.check_run(from, to)?;
        let n = to - from + 1;
        if self.frames.len() + n > MAX_FRAMES {
            return Err(LimitError::TooManyFrames);
        }
        let links = self.copied_links(from, to);
        for i in 0..n {
            let (source, at) = (from + i, to + 1 + i);
            for (layer, links) in self.layers.iter_mut().zip(&links) {
                let cel = layer.cel(source).clone();
                layer.insert_frame(at, cel, links[i]);
            }
            let info = self.new_frame_like(self.frames[source]);
            self.frames.insert(at, info);
            self.tags_after_insert(at);
        }
        self.hold_references()
    }

    /// The link each copy of `from..=to` joins, layer by layer, for [`Self::duplicate_frames`].
    fn copied_links(&self, from: usize, to: usize) -> Vec<Vec<u32>> {
        let table = self.link_table();
        let run = from..=to;
        let inside = |id: u32| {
            let members = table.members(id);
            members.iter().all(|m| run.contains(&usize::from(m.frame)))
        };
        let mut fresh = self.new_link_id();
        // Each link wholly inside the run, with the id its copies take, on every layer.
        let mut renamed: HashMap<u32, u32> = HashMap::new();
        let mut copied = Vec::with_capacity(self.layers.len());
        for layer in self.layers.iter() {
            let mut links = Vec::with_capacity(to - from + 1);
            for id in run.clone().map(|f| layer.link(f)) {
                if id == 0 || !inside(id) {
                    links.push(id);
                    continue;
                }
                let next = fresh;
                let new = *renamed.entry(id).or_insert(next);
                if new == next {
                    fresh += 1;
                }
                links.push(new);
            }
            copied.push(links);
        }
        copied
    }

    /// Moves the frames `from..=to`, with all their cels and timing, so that the first is at
    /// `at`. Tags follow as [`Tag`]'s rule for a moved block says.
    pub fn move_frames(&mut self, from: usize, to: usize, at: usize) -> Result<(), LimitError> {
        self.check_run(from, to)?;
        let n = to - from + 1;
        if at + n > self.frames.len() {
            return Err(LimitError::NoSuchItem);
        }
        if at == from {
            return Ok(());
        }
        // One frame at a time, in an order that keeps the block's own order.
        let steps: Vec<(usize, usize)> = if at < from {
            (0..n).map(|i| (from + i, at + i)).collect()
        } else {
            (0..n).map(|_| (from, at + n - 1)).collect()
        };
        for (a, b) in steps {
            crate::layer::move_within(&mut self.frames, a, b);
            for layer in &mut self.layers {
                layer.move_frame(a, b);
            }
        }
        for tag in &mut self.tags {
            tag.after_block_move((from, to), at);
        }
        Ok(())
    }

    /// Gives a tag other frames.
    pub fn set_tag_range(&mut self, tag: usize, from: usize, to: usize) -> Result<(), LimitError> {
        self.check_run(from, to)?;
        let found = self.tags.get_mut(tag).ok_or(LimitError::NoSuchItem)?;
        (found.from, found.to) = (from, to);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::ColorMode;

    fn order(doc: &Document) -> Vec<u32> {
        doc.frames.iter().map(|f| f.duration_ms).collect()
    }

    /// Frames timed 10, 20, 30 ... so their order can be read back.
    fn numbered(frames: usize) -> Document {
        let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("valid");
        for at in 1..frames {
            doc.add_frame(at, false).expect("room");
        }
        for (i, f) in doc.frames.iter_mut().enumerate() {
            f.duration_ms = 10 * (i as u32 + 1);
        }
        doc
    }

    #[test]
    fn a_block_moves_whole_either_way_and_the_map_agrees() {
        for (from, to, at, want) in [
            (1, 2, 3, [10, 40, 50, 20, 30]),
            (3, 4, 0, [40, 50, 10, 20, 30]),
            (0, 1, 1, [30, 10, 20, 40, 50]),
        ] {
            let mut doc = numbered(5);
            doc.move_frames(from, to, at).expect("fits");
            assert_eq!(order(&doc), want, "{from}..={to} to {at}");
            for f in 0..5 {
                let went = after_block_move(f, (from, to), at);
                assert_eq!(doc.frames[went].duration_ms, 10 * (f as u32 + 1));
            }
        }
        let mut doc = numbered(3);
        assert_eq!(doc.move_frames(1, 2, 2), Err(LimitError::NoSuchItem));
    }

    #[test]
    fn tags_follow_a_moved_block() {
        let mut doc = numbered(6);
        doc.add_tag("walk", 1, 2).expect("tag");
        doc.add_tag("idle", 3, 5).expect("tag");
        doc.add_tag("part", 0, 1).expect("tag");
        // The whole of "walk" moves to the end; "idle" closes up; "part" loses frame 1.
        doc.move_frames(1, 2, 4).expect("fits");
        let range = |i: usize| (doc.tags[i].from, doc.tags[i].to);
        assert_eq!(range(0), (4, 5));
        assert_eq!(range(1), (1, 3));
        assert_eq!(range(2), (0, 0));
        // A block landing inside a tag makes it wider.
        let mut doc = numbered(6);
        doc.add_tag("idle", 2, 4).expect("tag");
        doc.move_frames(0, 0, 3).expect("fits");
        assert_eq!((doc.tags[0].from, doc.tags[0].to), (1, 4));
    }

    #[test]
    fn duplicates_follow_their_frames_with_timing_holds_and_tags() {
        let mut doc = numbered(4);
        doc.hold_across(0, 1, 1, 2).expect("hold");
        doc.add_tag("walk", 1, 2).expect("tag");
        doc.add_tag("idle", 3, 3).expect("tag");
        doc.duplicate_frames(1, 2).expect("room");
        assert_eq!(order(&doc), [10, 20, 30, 20, 30, 40]);
        let layer = &doc.layers()[0];
        let copy = layer.link(3);
        assert!(copy != 0 && copy == layer.link(4), "the hold is copied");
        assert_ne!(copy, layer.link(1), "as a hold of its own");
        assert_eq!(
            (doc.tags[0].from, doc.tags[0].to),
            (1, 4),
            "the tag takes its copies"
        );
        assert_eq!(
            (doc.tags[1].from, doc.tags[1].to),
            (5, 5),
            "later tags shift"
        );
    }

    #[test]
    fn a_hold_reaching_past_the_run_grows_over_the_copies() {
        let mut doc = numbered(4);
        doc.hold_across(0, 0, 0, 3).expect("hold");
        let held = doc.layers()[0].link(0);
        doc.duplicate_frames(1, 2).expect("room");
        let links: Vec<u32> = doc.layers()[0].links().collect();
        assert_eq!(links, vec![held; 6], "a background follows the animation");
    }

    #[test]
    fn a_run_holding_a_whole_hold_copies_it_as_a_hold_of_its_own() {
        let mut doc = numbered(3);
        doc.hold_across(0, 0, 0, 1).expect("hold");
        let held = doc.layers()[0].link(0);
        doc.duplicate_frames(0, 2).expect("room");
        let layer = &doc.layers()[0];
        let (a, b) = (layer.link(3), layer.link(4));
        assert!(a != 0 && a == b, "the copies are held together");
        assert_ne!(a, held, "and apart from the original");
        assert_eq!(layer.link(1), held, "the original keeps its hold");
        assert_eq!(layer.link(5), 0, "the frame between is left out");
        assert!(
            layer.cel(3).same_image(layer.cel(0)),
            "the drawing is shared"
        );
    }

    #[test]
    fn a_tag_takes_any_run_of_frames_that_exists() {
        let mut doc = numbered(4);
        doc.add_tag("walk", 1, 2).expect("tag");
        doc.set_tag_range(0, 0, 3).expect("valid");
        assert_eq!((doc.tags[0].from, doc.tags[0].to), (0, 3));
        assert_eq!(doc.set_tag_range(0, 2, 1), Err(LimitError::NoSuchItem));
        assert_eq!(doc.set_tag_range(0, 0, 4), Err(LimitError::NoSuchItem));
        assert_eq!(doc.set_tag_range(1, 0, 0), Err(LimitError::NoSuchItem));
    }
}
