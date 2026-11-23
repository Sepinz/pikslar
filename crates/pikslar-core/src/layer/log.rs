//! What each change to a layer's list of cels replaced, kept while a step of history is made.
//!
//! An undo step plays the log backwards. Playing it logs again, which gives the log for redo.

use super::{Cel, Layer};
use std::fmt;
use std::sync::Arc;

/// One change to a layer's list of cels, with what undoing it needs.
#[derive(Clone, Debug)]
pub(crate) enum CelEntry {
    /// A frame's cel before it was replaced, moved or drawn on.
    Put(usize, Cel),
    /// Frames inserted at a place.
    Inserted { at: usize, count: usize },
    /// Frames removed from a place, with their cels.
    Removed { at: usize, cels: Vec<Cel> },
    /// A frame moved.
    Moved { from: usize, to: usize },
    /// The whole list, before a change to every frame.
    List(Arc<Vec<Cel>>),
}

impl CelEntry {
    /// The cels this entry keeps.
    /// True if undoing this entry finds the frames it needs, as it always should.
    fn fits(&self, len: usize) -> bool {
        match self {
            CelEntry::Put(frame, _) => *frame < len,
            CelEntry::Inserted { at, count } => at + count <= len,
            CelEntry::Removed { at, .. } => *at <= len,
            CelEntry::Moved { from, to } => (*from).max(*to) < len,
            CelEntry::List(_) => true,
        }
    }

    pub(crate) fn cels(&self) -> &[Cel] {
        match self {
            CelEntry::Put(_, cel) => std::slice::from_ref(cel),
            CelEntry::Removed { cels, .. } => cels,
            CelEntry::List(list) => list,
            CelEntry::Inserted { .. } | CelEntry::Moved { .. } => &[],
        }
    }

    /// What the entry costs, apart from the images its cels show.
    pub(crate) fn bytes(&self) -> usize {
        let cels = match self {
            CelEntry::Removed { cels, .. } => cels.capacity(),
            // A list the document still shares costs the step nothing.
            CelEntry::List(list) if Arc::strong_count(list) == 1 => list.capacity(),
            CelEntry::List(_) => 0,
            CelEntry::Put(..) | CelEntry::Inserted { .. } | CelEntry::Moved { .. } => 0,
        };
        size_of::<CelEntry>() + cels * size_of::<Cel>()
    }
}

/// While on, the changes made to a layer's list. A copy is always off, so copying a layer or a
/// document never copies a log, and the log takes no part in comparing layers.
#[derive(Default)]
pub(crate) struct CelLog(Option<Vec<CelEntry>>);

impl Clone for CelLog {
    fn clone(&self) -> Self {
        CelLog(None)
    }
}

impl fmt::Debug for CelLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CelLog")
    }
}

impl Layer {
    /// Starts logging every change to the list of cels.
    pub(crate) fn start_log(&mut self) {
        self.log.0 = Some(Vec::new());
    }

    /// Stops logging and returns what was logged.
    pub(crate) fn take_log(&mut self) -> Vec<CelEntry> {
        self.log.0.take().unwrap_or_default()
    }

    pub(super) fn note(&mut self, entry: CelEntry) {
        if let Some(entries) = &mut self.log.0 {
            entries.push(entry);
        }
    }

    pub(super) fn logging(&self) -> bool {
        self.log.0.is_some()
    }

    /// Logs a frame's cel as it is, before it changes.
    pub(super) fn keep(&mut self, frame: usize) {
        if let (true, Some(cel)) = (self.logging(), self.cels.get(frame)) {
            let cel = cel.clone();
            self.note(CelEntry::Put(frame, cel));
        }
    }

    /// Logs the whole list as it is, before every frame changes.
    pub(super) fn keep_list(&mut self) {
        if self.logging() {
            let list = self.cels.clone();
            self.note(CelEntry::List(list));
        }
    }

    /// Takes what `other`, a copy of this layer changed elsewhere, holds: its cels, logged as a
    /// whole list, and its settings. The session id, the log and how the artist looks at the
    /// layer (shown, locked, folded) stay this layer's.
    pub(crate) fn take_on(&mut self, other: Layer) {
        if !Arc::ptr_eq(&self.cels, &other.cels) {
            self.keep_list();
            self.cels = other.cels;
        }
        self.persisted_id = other.persisted_id;
        self.name = other.name;
        self.kind = other.kind;
        self.depth = other.depth;
        self.opacity = other.opacity;
        self.blend = other.blend;
        self.mode = other.mode;
    }

    /// Undoes these changes, newest first, logging what redoes them.
    pub(crate) fn play_back(&mut self, entries: Vec<CelEntry>) {
        entries
            .into_iter()
            .rev()
            .for_each(|entry| self.undo_entry(entry));
    }

    fn undo_entry(&mut self, entry: CelEntry) {
        debug_assert!(
            entry.fits(self.cels.len()),
            "a step's entry for frames not there"
        );
        match entry {
            CelEntry::Put(frame, cel) => {
                self.keep(frame);
                if let Some(slot) = self.cels_mut().get_mut(frame) {
                    *slot = cel;
                }
            }
            CelEntry::Inserted { at, count } => {
                let end = (at + count).min(self.cels.len());
                let at = at.min(end);
                let cels: Vec<Cel> = self.cels_mut().drain(at..end).collect();
                self.note(CelEntry::Removed { at, cels });
            }
            CelEntry::Removed { at, cels } => {
                let (at, count) = (at.min(self.cels.len()), cels.len());
                self.cels_mut().splice(at..at, cels);
                self.note(CelEntry::Inserted { at, count });
            }
            CelEntry::Moved { from, to } => {
                if from.max(to) < self.cels.len() {
                    self.move_frame(to, from);
                }
            }
            CelEntry::List(list) => {
                let old = std::mem::replace(&mut self.cels, list);
                self.note(CelEntry::List(old));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::ColorMode;
    use crate::grid::Grid;
    use crate::image::Image;

    /// A small, repeatable source of choices.
    struct Dice(u64);

    impl Dice {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n.max(1) as u64) as usize
        }
    }

    fn dot(value: u32) -> Cel {
        Cel::new(Grid::from_cells(2, 1, &[value, 0]))
    }

    fn layer(count: usize) -> Layer {
        let mut layer = Layer::new("test".into(), ColorMode::Rgba);
        (0..count).for_each(|v| layer.push_cel(dot(v as u32 + 1)));
        layer
    }

    /// Everything about each frame, including which frames show one image.
    fn state(layer: &Layer) -> Vec<(Cel, usize)> {
        let cels = layer.cels();
        let first_with_image = |cel: &Cel| {
            cels.iter()
                .position(|other| other.same_image(cel))
                .unwrap_or(0)
        };
        cels.iter()
            .map(|c| (c.clone(), first_with_image(c)))
            .collect()
    }

    fn some_change(l: &mut Layer, dice: &mut Dice, value: u32) {
        let n = l.frame_count();
        let (a, b) = (dice.below(n), dice.below(n));
        match dice.below(13) {
            0 => l.insert_frame(dice.below(n + 1), dot(value), 0),
            1 if n > 1 => {
                // A layer always keeps one frame.
                let from = a.min(b);
                l.remove_frames(from..=a.max(b).min(from + 2).min(from + n - 2));
            }
            2 => l.move_frame(a, b),
            3 => l.hold_frame(a, b, 1 + dice.below(3) as u32),
            4 => l.set_link(a, dice.below(3) as u32),
            5 => {
                let ids: Vec<u32> = l.links().collect();
                let _ = l.set_holds(&ids, 0);
            }
            6 => drop(l.put_cel(a, dot(value))),
            7 => {
                let mut changes = vec![(crate::image::TileKey { x: 0, y: 0 }, None)];
                l.swap_tiles(a, &mut changes, &mut None);
            }
            8 => l.move_cels(&[a as u32, b as u32], 3, -2),
            9 => l.share_held_cels(),
            10 => {
                let images = (0..n).map(|_| Image::blank()).collect();
                l.replace_images(images);
            }
            11 => l.let_go_of_drawing(a),
            _ => drop(l.shift_holds(dice.below(4) as u32)),
        }
    }

    #[test]
    fn playing_a_log_back_restores_the_list_and_its_log_restores_the_change() {
        let mut dice = Dice(0x9e37_79b9_7f4a_7c15);
        for round in 0..400 {
            let mut l = layer(1 + dice.below(6));
            // Holds and moved cels from before the step, which the step must leave as they were.
            for step in 0..dice.below(6) {
                some_change(&mut l, &mut dice, 1000 + step as u32);
            }
            let before = state(&l);
            l.start_log();
            for step in 0..1 + dice.below(8) {
                some_change(&mut l, &mut dice, (round * 10 + step) as u32);
            }
            let after = state(&l);
            let log = l.take_log();
            l.start_log();
            l.play_back(log);
            assert_eq!(state(&l), before, "undo, round {round}");
            let redo = l.take_log();
            l.start_log();
            l.play_back(redo);
            assert_eq!(state(&l), after, "redo, round {round}");
            let undo = l.take_log();
            l.play_back(undo);
            assert_eq!(state(&l), before, "undo again, round {round}");
        }
    }

    #[test]
    fn nothing_is_logged_while_the_log_is_off() {
        let mut l = layer(3);
        l.move_frame(0, 2);
        l.start_log();
        assert!(l.take_log().is_empty());
    }

    #[test]
    fn a_copy_of_a_layer_has_no_log() {
        let mut l = layer(3);
        l.start_log();
        l.move_frame(0, 2);
        let mut copy = l.clone();
        assert!(copy.take_log().is_empty());
        assert_eq!(l.take_log().len(), 1);
    }

    #[test]
    fn a_frame_change_with_nothing_sharing_the_list_copies_no_list() {
        let mut l = layer(3);
        let list = Arc::as_ptr(&l.cels);
        l.start_log();
        l.insert_frame(1, dot(9), 0);
        l.move_frame(0, 3);
        l.remove_frames(2..=2);
        assert_eq!(Arc::as_ptr(&l.cels), list, "edited in place");
    }

    #[test]
    fn a_write_inside_a_step_keeps_the_old_image() {
        let mut l = layer(1);
        let old = l.cel(0).clone();
        l.start_log();
        let mut changes = vec![(crate::image::TileKey { x: 0, y: 0 }, None)];
        l.swap_tiles(0, &mut changes, &mut None);
        assert_ne!(l.cel(0), &old);
        let log = l.take_log();
        l.play_back(log);
        assert_eq!(l.cel(0), &old);
    }

    #[test]
    fn a_hold_put_back_shares_one_drawing_again() {
        let mut l = layer(3);
        (0..3).for_each(|f| l.set_link(f, 1));
        l.share_held_cels();
        l.start_log();
        l.remove_frames(2..=2);
        let log = l.take_log();
        // The log keeps the hold's drawing, so a stroke copies it, and undoing the stroke
        // shows the kept drawing again.
        let (mut changes, mut split) = (vec![(crate::image::TileKey { x: 0, y: 0 }, None)], None);
        l.swap_tiles(0, &mut changes, &mut split);
        assert!(
            split.is_some(),
            "the stroke split the drawing the log keeps"
        );
        l.swap_tiles(0, &mut changes, &mut split);
        l.play_back(log);
        assert!(l.cel(0).same_image(l.cel(2)), "the hold is one drawing");
        assert!(l.cel(1).same_image(l.cel(2)), "the hold is one drawing");
    }

    #[test]
    fn redoing_a_write_and_then_a_copy_of_its_frame_shares_one_drawing_again() {
        let mut l = layer(1);
        let (mut changes, mut split) = (vec![(crate::image::TileKey { x: 0, y: 0 }, None)], None);
        l.swap_tiles(0, &mut changes, &mut split);
        l.start_log();
        let copied = l.cel(0).clone();
        l.insert_frame(1, copied, 0);
        let add = l.take_log();
        // Undo the copy, whose redo keeps the drawing, then the write, which must split it.
        l.start_log();
        l.play_back(add);
        let redo = l.take_log();
        l.swap_tiles(0, &mut changes, &mut split);
        assert!(
            split.is_some(),
            "undoing the write split the drawing the log keeps"
        );
        // Redo both.
        l.swap_tiles(0, &mut changes, &mut split);
        l.start_log();
        l.play_back(redo);
        drop(l.take_log());
        assert!(
            l.cel(1).same_image(l.cel(0)),
            "the copy and its source share one drawing"
        );
    }
}
