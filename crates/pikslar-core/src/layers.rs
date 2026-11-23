//! A document's list of layers. Layers are added, removed and moved only through the methods
//! here, so that while a step of history is made, every change to the list and to each layer's
//! cels is logged.

use crate::layer::{Cel, CelEntry, Layer};
use std::fmt;
use std::ops::{Deref, Index, IndexMut};

/// One change to the list of layers, with what undoing it needs.
#[derive(Clone, Debug)]
pub(crate) enum LayerEntry {
    Added(usize),
    Removed(usize, Box<Layer>),
    Moved { from: usize, to: usize },
}

impl LayerEntry {
    /// True if undoing this entry finds the layers it needs, as it always should.
    fn fits(&self, len: usize) -> bool {
        match self {
            LayerEntry::Added(index) => *index < len,
            LayerEntry::Removed(index, _) => *index <= len,
            LayerEntry::Moved { from, to } => (*from).max(*to) < len,
        }
    }
}

/// Everything one step changed in the layers: the list first, then each layer's cels, by id.
#[derive(Clone, Debug, Default)]
pub(crate) struct StepLog {
    layers: Vec<LayerEntry>,
    cels: Vec<(u32, Vec<CelEntry>)>,
}

/// True for a run of things all equal, which moving one of them within the run leaves as it is.
fn all_equal<T: PartialEq>(run: Option<&[T]>) -> bool {
    run.is_some_and(|run| run.windows(2).all(|pair| pair[0] == pair[1]))
}

/// True if undoing these changes would leave the layer's cels equal to what they are now.
fn cels_same(layer: &Layer, entries: &[CelEntry]) -> bool {
    let cels = layer.cels();
    match entries {
        [CelEntry::Moved { from, to }] => all_equal(cels.get(*from.min(to)..=*from.max(to))),
        _ => puts_same(cels, entries).unwrap_or_else(|| played_back_same(layer, entries)),
    }
}

/// For changes that only put cels back where they were, whether undoing them leaves the cels as
/// they are now. Undoing puts each frame's oldest cel back last, so only that one counts. None
/// if a change moves frames about.
fn puts_same(cels: &[Cel], entries: &[CelEntry]) -> Option<bool> {
    if !entries.iter().all(|e| matches!(e, CelEntry::Put(..))) {
        return None;
    }
    // The entries are in the order they were made, so a frame's first is its oldest.
    let mut seen = vec![0u64; cels.len().div_ceil(64)];
    let mut same = true;
    for entry in entries {
        let CelEntry::Put(frame, old) = entry else {
            continue;
        };
        let (word, bit) = (frame / 64, 1u64 << (frame % 64));
        match seen.get_mut(word) {
            // No such frame now: undoing would not leave the cels as they are.
            None => same = false,
            Some(w) if *w & bit != 0 => {}
            Some(w) => {
                *w |= bit;
                same = same && cels.get(*frame) == Some(old);
            }
        }
    }
    Some(same)
}

/// Plays the changes back on a copy of the layer and compares.
fn played_back_same(layer: &Layer, entries: &[CelEntry]) -> bool {
    let mut copy = layer.clone();
    copy.play_back(entries.to_vec());
    copy.cels() == layer.cels()
}

/// The widest tile table among these cels, in bytes.
pub(crate) fn widest(cels: &[Cel]) -> usize {
    cels.iter().map(Cel::table_bytes).max().unwrap_or(0)
}

/// The widest table among the cels these changes put in a layer. Frames inserted are blank or
/// share a drawing with a frame already there, and moved or removed cels were there before, so
/// none of those is wider than before.
fn widest_put_in(layer: &Layer, entries: &[CelEntry]) -> usize {
    let cels = layer.cels();
    let put = |e: &CelEntry| matches!(e, CelEntry::Put(..));
    // A frame's number in an entry is where it was then, which a later change may have moved.
    if entries.iter().any(put) && !entries.iter().all(put)
        || entries.iter().any(|e| matches!(e, CelEntry::List(_)))
    {
        return widest(cels);
    }
    let at = |e: &CelEntry| match e {
        CelEntry::Put(frame, _) => cels.get(*frame..=*frame),
        _ => None,
    };
    entries.iter().filter_map(at).map(widest).max().unwrap_or(0)
}

impl StepLog {
    /// The widest table among the cels this step put in the layers, or among all of a layer's
    /// cels where the log cannot tell which those are. A layer added is blank.
    pub(crate) fn widest_put(&self, layers: &Layers) -> usize {
        let per_layer = |(id, entries): &(u32, Vec<CelEntry>)| {
            let layer = layers.iter().find(|l| l.id() == *id);
            layer.map_or(0, |l| widest_put_in(l, entries))
        };
        self.cels.iter().map(per_layer).max().unwrap_or(0)
    }

    /// The layers this step removed.
    fn removed(&self) -> impl Iterator<Item = &Layer> {
        self.layers.iter().filter_map(|entry| match entry {
            LayerEntry::Removed(_, layer) => Some(layer.as_ref()),
            LayerEntry::Added(_) | LayerEntry::Moved { .. } => None,
        })
    }

    /// A layer this step removed, as it was then.
    pub(crate) fn removed_layer_mut(&mut self, id: u32) -> Option<&mut Layer> {
        self.layers.iter_mut().find_map(|entry| match entry {
            LayerEntry::Removed(_, layer) if layer.id() == id => Some(layer.as_mut()),
            _ => None,
        })
    }

    /// Every cel the log keeps, in entries and in removed layers.
    pub(crate) fn kept_cels(&self) -> impl Iterator<Item = &Cel> {
        let entries = self
            .cels
            .iter()
            .flat_map(|(_, e)| e)
            .flat_map(CelEntry::cels);
        entries.chain(self.removed().flat_map(Layer::cels))
    }

    /// True if the layers are equal, as values, to what they were before the step, as a step
    /// that moves a frame between two that look the same leaves them. Two layers are never the
    /// same layer, each with its own id, so a layer moved anywhere but its own place is a
    /// change, however alike the layers look.
    pub(crate) fn changes_nothing(&self, layers: &Layers) -> bool {
        let list_same = match self.layers.as_slice() {
            [] => true,
            [LayerEntry::Moved { from, to }] => from == to,
            _ => false,
        };
        list_same
            && self.cels.iter().all(|(id, entries)| {
                layers
                    .iter()
                    .find(|l| l.id() == *id)
                    .is_some_and(|layer| cels_same(layer, entries))
            })
    }

    /// What the log itself takes: its entries and the layers it removed, but not their
    /// drawings, which history charges once across all its steps.
    pub(crate) fn bytes(&self) -> usize {
        let per_layer = |(_, entries): &(u32, Vec<CelEntry>)| {
            let kept: usize = entries.iter().map(CelEntry::bytes).sum();
            kept + (entries.capacity() - entries.len()) * size_of::<CelEntry>()
        };
        let entries: usize = self.cels.iter().map(per_layer).sum();
        let lists = self.cels.capacity() * size_of::<(u32, Vec<CelEntry>)>();
        let layers = self.layers.capacity() * size_of::<LayerEntry>();
        let removed: usize = self.removed().map(Layer::record_bytes).sum();
        entries + lists + layers + removed
    }

    /// Lets go of room the log grew into and will not use, as it is kept from now on.
    fn shrink(&mut self) {
        self.cels
            .iter_mut()
            .for_each(|(_, entries)| entries.shrink_to_fit());
        self.cels.shrink_to_fit();
        self.layers.shrink_to_fit();
    }
}

#[derive(Default)]
pub(crate) struct Layers {
    list: Vec<Layer>,
    /// While a step is made, what it changed in the list, and the cels of layers it removed.
    log: Option<StepLog>,
}

/// A copy has no log, so copying a document never copies one.
impl Clone for Layers {
    fn clone(&self) -> Self {
        Layers {
            list: self.list.clone(),
            log: None,
        }
    }
}

impl PartialEq for Layers {
    fn eq(&self, other: &Self) -> bool {
        self.list == other.list
    }
}

impl Eq for Layers {}

impl fmt::Debug for Layers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.list.fmt(f)
    }
}

impl From<Vec<Layer>> for Layers {
    fn from(list: Vec<Layer>) -> Self {
        Layers { list, log: None }
    }
}

impl Deref for Layers {
    type Target = [Layer];

    fn deref(&self) -> &[Layer] {
        &self.list
    }
}

impl Index<usize> for Layers {
    type Output = Layer;

    fn index(&self, index: usize) -> &Layer {
        &self.list[index]
    }
}

impl IndexMut<usize> for Layers {
    fn index_mut(&mut self, index: usize) -> &mut Layer {
        &mut self.list[index]
    }
}

impl<'a> IntoIterator for &'a mut Layers {
    type Item = &'a mut Layer;
    type IntoIter = std::slice::IterMut<'a, Layer>;

    fn into_iter(self) -> Self::IntoIter {
        self.list.iter_mut()
    }
}

impl Layers {
    pub(crate) fn iter_mut(&mut self) -> std::slice::IterMut<'_, Layer> {
        self.list.iter_mut()
    }

    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut Layer> {
        self.list.get_mut(index)
    }

    /// Every layer, to change its settings in place outside a step. Reordering or replacing a
    /// layer through this is not logged, so undo would not know of it.
    pub(crate) fn as_mut_slice(&mut self) -> &mut [Layer] {
        &mut self.list
    }

    fn note(&mut self, entry: LayerEntry) {
        if let Some(log) = &mut self.log {
            log.layers.push(entry);
        }
    }

    pub(crate) fn insert(&mut self, index: usize, mut layer: Layer) {
        let index = index.min(self.list.len());
        if self.log.is_some() {
            layer.start_log();
        }
        self.note(LayerEntry::Added(index));
        self.list.insert(index, layer);
    }

    /// Removes a layer that exists.
    pub(crate) fn remove(&mut self, index: usize) {
        let mut layer = self.list.remove(index);
        if let Some(log) = &mut self.log {
            let entries = layer.take_log();
            if !entries.is_empty() {
                log.cels.push((layer.id(), entries));
            }
            log.layers.push(LayerEntry::Removed(index, Box::new(layer)));
        }
    }

    /// Moves a layer that exists to a place that exists.
    pub(crate) fn move_to(&mut self, from: usize, to: usize) {
        let layer = self.list.remove(from);
        self.list.insert(to, layer);
        self.note(LayerEntry::Moved { from, to });
    }

    /// The layers, from the bottom, for a document taking them on.
    pub(crate) fn into_vec(self) -> Vec<Layer> {
        self.list
    }

    /// Starts logging every change to the list and to every layer's cels.
    pub(crate) fn start_log(&mut self) {
        self.list.iter_mut().for_each(Layer::start_log);
        self.log = Some(StepLog::default());
    }

    /// Stops logging and returns what was logged.
    pub(crate) fn take_log(&mut self) -> StepLog {
        let mut log = self.log.take().unwrap_or_default();
        for layer in &mut self.list {
            let entries = layer.take_log();
            if !entries.is_empty() {
                log.cels.push((layer.id(), entries));
            }
        }
        log.shrink();
        log
    }

    /// Undoes a step's changes: the list's, newest first, then each remaining layer's cels.
    /// A layer the step added is removed whole, with its changes in it. A layer it removed comes
    /// back as it was when removed, and then its cels are undone. While logging, this logs what
    /// redoes the step.
    pub(crate) fn play_back(&mut self, log: StepLog) {
        for entry in log.layers.into_iter().rev() {
            debug_assert!(
                entry.fits(self.list.len()),
                "a step's entry for layers not there"
            );
            match entry {
                LayerEntry::Added(index) if index < self.list.len() => self.remove(index),
                LayerEntry::Removed(index, layer) => self.insert(index, *layer),
                LayerEntry::Moved { from, to } if from.max(to) < self.list.len() => {
                    self.move_to(to, from)
                }
                LayerEntry::Added(_) | LayerEntry::Moved { .. } => {}
            }
        }
        for (id, entries) in log.cels {
            if let Some(layer) = self.list.iter_mut().find(|l| l.id() == id) {
                layer.play_back(entries);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::document::{ColorMode, Document};
    use crate::grid::Grid;
    use crate::layer::{Cel, CelEntry};

    struct Dice(u64);

    impl Dice {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n.max(1) as u64) as usize
        }
    }

    /// A cel whose table is far wider than its canvas.
    fn wide() -> Cel {
        let far = std::collections::HashMap::from([((0, 0), 7), ((3000, 2000), 9)]);
        Cel::at(crate::offset_tests::image_of(&far), 0, 0).expect("in range")
    }

    #[test]
    fn a_wide_cel_put_and_then_moved_along_by_an_insert_is_found() {
        let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("size");
        doc.add_frame(1, false).expect("room");
        doc.layers.start_log();
        doc.put_cel(0, 0, wide()).expect("fits");
        doc.add_frame(0, false).expect("room");
        let log = doc.layers.take_log();
        let table = doc.layers()[0].cel(1).table_bytes();
        assert!(table > 1000);
        assert_eq!(log.widest_put(&doc.layers), table);
    }

    #[test]
    fn wide_drawings_swapped_in_for_every_frame_are_found() {
        let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("size");
        doc.layers.start_log();
        let far = std::collections::HashMap::from([((0, 0), 7), ((3000, 2000), 9)]);
        let image = crate::offset_tests::image_of(&far);
        doc.layers_mut()[0].replace_images(vec![image]);
        let log = doc.layers.take_log();
        let table = doc.layers()[0].cel(0).table_bytes();
        assert!(table > 1000);
        assert_eq!(log.widest_put(&doc.layers), table);
    }

    #[test]
    fn changes_that_only_put_cels_back_compare_as_playing_them_back_does() {
        let mut dice = Dice(0x5eed);
        let drawings: Vec<Cel> = (1..4)
            .map(|v| Cel::new(Grid::from_cells(2, 1, &[v, 0])))
            .collect();
        for _ in 0..500 {
            let mut doc = Document::new(2, 1, ColorMode::Rgba).expect("size");
            (1..4).for_each(|at| doc.add_frame(at, false).expect("room"));
            for frame in 0..4 {
                doc.put_cel(0, frame, drawings[dice.below(3)].clone())
                    .expect("fits");
            }
            doc.layers.start_log();
            for _ in 0..dice.below(5) + 1 {
                let cel = drawings[dice.below(3)].clone();
                doc.put_cel(0, dice.below(4), cel).expect("fits");
                if dice.below(4) == 0 {
                    doc.add_frame(dice.below(4), false).expect("room");
                }
            }
            let log = doc.layers.take_log();
            let (_, entries) = &log.cels[0];
            let layer = &doc.layers()[0];
            let played = super::played_back_same(layer, entries);
            let shifts = entries.iter().any(|e| !matches!(e, CelEntry::Put(..)));
            let expected = (!shifts).then_some(played);
            assert_eq!(super::puts_same(layer.cels(), entries), expected);
            assert_eq!(super::cels_same(layer, entries), played);
        }
    }

    /// A layer's id, its frames with the first frame sharing each one's image, and its name.
    type LayerState = (u32, Vec<(Cel, usize)>, String);

    /// Each layer by id, with its frames, which frames share an image, and its settings.
    fn state(doc: &Document) -> Vec<LayerState> {
        doc.layers()
            .iter()
            .map(|l| {
                let cels = l.cels();
                let shared = |c: &Cel| cels.iter().position(|o| o.same_image(c));
                let frames = cels
                    .iter()
                    .map(|c| (c.clone(), shared(c).unwrap_or(0)))
                    .collect();
                (l.id(), frames, l.name.clone())
            })
            .collect()
    }

    fn doc() -> Document {
        let mut doc = Document::new(4, 2, ColorMode::Rgba).expect("size");
        doc.add_layer(1, ColorMode::Rgba).expect("room");
        doc.add_frame(1, false).expect("room");
        doc.add_frame(2, false).expect("room");
        doc
    }

    fn some_change(doc: &mut Document, dice: &mut Dice, value: u32) {
        let (layers, frames) = (doc.layers().len(), doc.frames.len());
        let (layer, frame) = (dice.below(layers), dice.below(frames));
        match dice.below(7) {
            0 => doc
                .add_layer(dice.below(layers + 1), ColorMode::Rgba)
                .expect("room"),
            1 if layers > 1 => doc.remove_layer(layer).expect("exists"),
            2 => doc.layers.move_to(layer, dice.below(layers)),
            3 => doc
                .add_frame(dice.below(frames + 1), dice.below(2) == 0)
                .expect("room"),
            4 if frames > 1 => doc.remove_frame(frame).expect("exists"),
            5 => doc.move_frame(frame, dice.below(frames)).expect("exists"),
            _ => {
                let grid = Grid::from_cells(4, 2, &[value; 8]);
                doc.set_cel(layer, frame, grid).expect("canvas size");
            }
        }
    }

    #[test]
    fn a_step_of_layer_and_frame_changes_plays_back_both_ways() {
        let mut dice = Dice(0x2545_f491_4f6c_dd1d);
        for round in 0..300 {
            let mut d = doc();
            for step in 0..dice.below(5) {
                some_change(&mut d, &mut dice, 500 + step as u32);
            }
            let before = state(&d);
            d.layers.start_log();
            for step in 0..1 + dice.below(8) {
                some_change(&mut d, &mut dice, (round * 10 + step) as u32);
            }
            let after = state(&d);
            let log = d.layers.take_log();
            d.layers.start_log();
            d.layers.play_back(log);
            assert_eq!(state(&d), before, "undo, round {round}");
            let redo = d.layers.take_log();
            d.layers.start_log();
            d.layers.play_back(redo);
            assert_eq!(state(&d), after, "redo, round {round}");
            let undo = d.layers.take_log();
            d.layers.play_back(undo);
            assert_eq!(state(&d), before, "undo again, round {round}");
        }
    }

    #[test]
    fn a_copy_of_the_layers_has_no_log() {
        let mut d = doc();
        d.layers.start_log();
        d.remove_layer(0).expect("exists");
        let mut copy = d.clone();
        assert!(copy.layers.take_log().layers.is_empty());
        assert!(!d.layers.take_log().layers.is_empty());
        assert_eq!(copy, d);
    }
}
