//! How many times history's steps keep each drawing, so that a drawing only history keeps is
//! charged once, to one step, however many steps keep it.

use crate::image::Image;
use crate::layer::{kept_bytes, Cel, Split};
use crate::layers::StepLog;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::{Arc, Weak};

/// A drawing's address. It stays the drawing's own while any step keeps it, and a step lets go
/// of its drawings before they can be dropped, so an address is never taken for another drawing.
type Key = usize;

fn key(cel: &Cel) -> Key {
    cel.image_key() as Key
}

/// The cels of a step that show a drawing. A blank cel has nothing to charge, and a kept image
/// never changes, so taking a step on and letting it go pass over the same cels.
fn drawn(log: &StepLog) -> impl Iterator<Item = &Cel> {
    log.kept_cels().filter(|cel| !cel.is_blank())
}

/// Mixes a drawing's address. Addresses are already spread out, and are never chosen by a file,
/// so a multiply is enough where the standard hash would take several times as long.
#[derive(Default)]
struct Mix(u64);

impl Hasher for Mix {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        bytes.iter().for_each(|&b| self.write_u64(u64::from(b)));
    }

    fn write_u64(&mut self, n: u64) {
        let h = (self.0 ^ n).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        // The table picks a slot from the low bits, and aligned addresses have none.
        self.0 = h ^ (h >> 29);
    }

    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
}

/// The drawings history's steps keep, and how many cels in the steps show each.
#[derive(Default)]
pub(super) struct Held {
    counts: HashMap<Key, usize, BuildHasherDefault<Mix>>,
    /// The most the counts had room for. Removing an entry can leave its slot marked rather than
    /// free, which the table no longer reports as room, though it still holds it.
    room: usize,
}

impl Held {
    /// What the counts take.
    pub(super) fn bytes(&self) -> usize {
        let room = self.room.max(self.counts.capacity());
        super::hash_table_bytes(room, size_of::<(Key, usize)>())
    }

    /// Counts the drawings a step keeps, and returns what the step is charged: the drawings
    /// that only history keeps, each when its last holder is counted.
    ///
    /// One pass is enough. Each cel that shows a drawing holds it, so while later cels of this
    /// step still show it, it has more holders than counts, and the counts reach the holders
    /// only at its last cel.
    ///
    /// No drawing is charged twice. Once its counts reach its holders, nothing outside history
    /// holds it, and only playing a step back can put it in the document again. A step is let
    /// go before it is played back, and the step charged is the newest to hold the drawing, so
    /// it is played back, or dropped, before any other step that holds it. Steps play back to
    /// the frames they logged, which hold the drawings they had: a change that only swaps
    /// drawings that look the same is put back, not left in place unrecorded.
    pub(super) fn take_on(&mut self, log: &StepLog) -> usize {
        let mut charge = 0;
        for cel in drawn(log) {
            let times = self.counts.entry(key(cel)).or_default();
            *times += 1;
            if cel.image_holders() == *times {
                charge += cel.kept_bytes();
            }
        }
        charge
    }

    /// Counts tile images a step keeps, and returns what the step is charged: each image that
    /// only history's steps hold, when its last holder is counted, as for drawings. An image
    /// two tiles of one kept tileset table share is two holders, and charged once.
    pub(super) fn take_on_images<'a>(
        &mut self,
        images: impl Iterator<Item = &'a Arc<Image>>,
    ) -> usize {
        let mut charge = 0;
        for image in images {
            let times = self.counts.entry(Arc::as_ptr(image) as Key).or_default();
            *times += 1;
            if Arc::strong_count(image) == *times {
                charge += kept_bytes(image);
            }
        }
        charge
    }

    /// Lets go of tile images a step keeps, the same images it took on.
    pub(super) fn let_go_images<'a>(&mut self, images: impl Iterator<Item = &'a Arc<Image>>) {
        self.room = self.room.max(self.counts.capacity());
        for image in images {
            let k = Arc::as_ptr(image) as Key;
            if let Some(times) = self.counts.get_mut(&k) {
                *times -= 1;
                if *times == 0 {
                    self.counts.remove(&k);
                }
            }
        }
    }

    /// What a drawing a pixel step split off keeps alive, if only history's steps still hold
    /// it, for that step to be charged. Past what a count can say, it says the most it can.
    ///
    /// No other step is charged for it. The document showed the drawing until the split, and
    /// a step is charged for a drawing only while the document does not show it. Every step
    /// that holds it is older than the pixel step, or was undone before it, so the pixel step is
    /// played, which charges it again, or dropped before any of them is played back.
    pub(super) fn left_to_history(&self, split: &Split) -> u32 {
        let Some(image) = split.as_ref().and_then(Weak::upgrade) else {
            return 0;
        };
        let times = self.counts.get(&(Arc::as_ptr(&image) as Key)).copied();
        // One holder is the reference upgraded here.
        match times {
            Some(times) if Arc::strong_count(&image) - 1 == times => {
                u32::try_from(kept_bytes(&image)).unwrap_or(u32::MAX)
            }
            _ => 0,
        }
    }

    /// Lets go of what a step keeps, before the step is played back or dropped. A drawing it
    /// was charged for is charged to no step until another step takes it on.
    pub(super) fn let_go(&mut self, log: &StepLog) {
        self.room = self.room.max(self.counts.capacity());
        for cel in drawn(log) {
            let k = key(cel);
            if let Some(times) = self.counts.get_mut(&k) {
                *times -= 1;
                if *times == 0 {
                    self.counts.remove(&k);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{ColorMode, Document};
    use crate::grid::Grid;

    fn doc() -> Document {
        let mut doc = Document::new(4, 2, ColorMode::Rgba).expect("size");
        doc.add_layer(1, ColorMode::Rgba).expect("room");
        doc.add_frame(1, false).expect("room");
        doc.add_frame(2, false).expect("room");
        doc
    }

    fn drawn(value: u32) -> Grid {
        Grid::from_cells(4, 2, &[value; 8])
    }

    /// The charge for replacing frame 1 of layer 0, with or without something else holding
    /// the drawing it replaces.
    fn charge_for_replacing(something_else_holds_it: bool) -> (usize, usize) {
        let mut d = doc();
        d.set_cel(0, 1, drawn(7)).expect("canvas size");
        let drawing = d.layers()[0].cel(1).kept_bytes();
        let elsewhere = something_else_holds_it.then(|| d.layers()[0].cel(1).clone());
        d.layers.start_log();
        d.set_cel(0, 1, drawn(8)).expect("canvas size");
        let log = d.layers.take_log();
        let charge = Held::default().take_on(&log);
        drop(elsewhere);
        (charge, drawing)
    }

    #[test]
    fn a_step_is_charged_for_a_drawing_only_it_keeps() {
        let (alone, drawing) = charge_for_replacing(false);
        let (shared, _) = charge_for_replacing(true);
        assert!(drawing > 0);
        assert_eq!(alone - shared, drawing);
    }

    #[test]
    fn aligned_addresses_spread_over_the_low_bits_the_table_picks_slots_by() {
        use std::hash::BuildHasher;
        let mix = BuildHasherDefault::<Mix>::default();
        let slots: std::collections::HashSet<u64> = (0..1024usize)
            .map(|n| mix.hash_one(0x7ff0_0000_0000 + n * 64) & 1023)
            .collect();
        assert!(slots.len() > 600, "{} of 1024 slots", slots.len());
    }

    #[test]
    fn a_kept_drawing_is_charged_for_the_allocation_it_lives_in() {
        let cel = doc().layers()[0].cel(0).clone();
        let room = 2 * size_of::<usize>() + size_of::<crate::image::Image>();
        assert_eq!(cel.kept_bytes(), cel.heap_size() + room);
    }

    #[test]
    fn the_counts_are_charged_for_every_slot_their_table_has() {
        let mut held = Held::default();
        assert_eq!(held.bytes(), 0, "an empty table has no slots");
        for k in 0..100 {
            held.counts.insert(k, 1);
        }
        let entry = size_of::<(Key, usize)>() + 1;
        let slots = (held.bytes() - 16) / entry;
        assert_eq!((held.bytes() - 16) % entry, 0);
        // The table keeps an eighth of its slots free, and has a power of two of them.
        assert!(slots.is_power_of_two(), "{slots} slots");
        assert_eq!(slots / 8 * 7, held.counts.capacity());
    }

    #[test]
    fn removing_a_frame_is_charged_for_that_frame_alone() {
        let mut d = doc();
        for frame in 0..3 {
            for layer in 0..2 {
                d.set_cel(layer, frame, drawn(frame as u32 * 2 + layer as u32 + 1))
                    .expect("canvas size");
            }
        }
        let removed: usize = d.layers().iter().map(|l| l.cel(1).kept_bytes()).sum();
        d.layers.start_log();
        d.remove_frame(1).expect("exists");
        let log = d.layers.take_log();
        let charge = Held::default().take_on(&log);
        assert!(charge >= removed, "{charge} keeps the frame");
        assert!(charge < removed + 1024, "{charge} keeps only the frame");
    }

    #[test]
    fn a_drawing_is_charged_again_once_the_step_charged_lets_go() {
        let mut d = doc();
        d.set_cel(0, 1, drawn(7)).expect("canvas size");
        let drawing = d.layers()[0].cel(1).kept_bytes();
        let mut held = Held::default();
        d.layers.start_log();
        d.set_cel(0, 1, drawn(8)).expect("canvas size");
        let first = d.layers.take_log();
        let kept = d.layers()[0].cel(1).clone();
        d.layers.start_log();
        d.set_cel(0, 1, drawn(9)).expect("canvas size");
        let second = d.layers.take_log();
        assert_eq!(
            held.take_on(&first),
            drawing,
            "only history keeps the first drawing"
        );
        let other = held.take_on(&second);
        assert_eq!(other, 0, "a copy outside history shows the second");
        drop(kept);
        held.let_go(&first);
        assert!(
            held.counts.values().all(|&times| times == 1),
            "the second's own"
        );
        assert_eq!(
            held.take_on(&first),
            drawing,
            "charged again when taken on again"
        );
    }
}
