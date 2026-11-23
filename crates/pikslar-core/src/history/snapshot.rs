//! A step of history for a change to the document's structure: what the change logged, and the
//! settings, frames, tags and colors from before it.

use super::held::Held;
use super::{Place, SavedSelection};
use crate::animation::Tag;
use crate::color::ColorTable;
use crate::document::{BlendMode, ColorMode, Document, FrameInfo};
use crate::layer::{Layer, LayerKind, PersistedLayerId};
use crate::layers::StepLog;
use crate::mask::Mask;
use crate::slice::Slice;
use crate::tileset::Tilesets;
use std::sync::Arc;

/// What a step can change about a layer, apart from its cels. Visibility is how the artist is
/// looking at the work, not part of the edit, so it is left as it is.
#[derive(Clone, Debug, PartialEq)]
struct Settings {
    id: u32,
    persisted: PersistedLayerId,
    name: String,
    opacity: u8,
    blend: BlendMode,
    mode: ColorMode,
    /// A tilemap layer's tileset can change, as when it is made independent.
    kind: LayerKind,
    depth: u8,
}

impl Settings {
    fn of(layer: &Layer) -> Self {
        Settings {
            id: layer.id(),
            persisted: layer.persisted_id(),
            name: layer.name.clone(),
            opacity: layer.opacity,
            blend: layer.blend,
            mode: layer.mode,
            kind: layer.kind,
            depth: layer.depth,
        }
    }

    fn swap(&mut self, layer: &mut Layer) {
        std::mem::swap(&mut self.name, &mut layer.name);
        std::mem::swap(&mut self.opacity, &mut layer.opacity);
        std::mem::swap(&mut self.blend, &mut layer.blend);
        std::mem::swap(&mut self.mode, &mut layer.mode);
        std::mem::swap(&mut self.kind, &mut layer.kind);
        std::mem::swap(&mut self.depth, &mut layer.depth);
        let persisted = layer.persisted_id();
        layer.set_persisted_id(self.persisted);
        self.persisted = persisted;
    }
}

/// The frame list from before a step: all of it until the step is finished, then only the run
/// of frames that differs, which `len` frames replaced.
#[derive(Clone, Debug)]
enum Frames {
    Whole(Vec<FrameInfo>),
    Span {
        at: usize,
        old: Vec<FrameInfo>,
        len: usize,
    },
}

impl Frames {
    fn swap(&mut self, frames: &mut Vec<FrameInfo>) {
        match self {
            Frames::Whole(old) => std::mem::swap(old, frames),
            Frames::Span { at, old, len } => {
                let end = (*at + *len).min(frames.len());
                let start = (*at).min(end);
                let back = std::mem::take(old);
                *len = back.len();
                *old = frames.splice(start..end, back).collect();
            }
        }
    }

    /// Keeps only what differs from the frames the step left, keys and all, so undo puts back
    /// the order of frames that look alike.
    fn finish(&mut self, now: &[FrameInfo]) {
        let Frames::Whole(old) = self else {
            return;
        };
        let front = old.iter().zip(now).take_while(|(a, b)| a.same(b)).count();
        let back = old[front..]
            .iter()
            .rev()
            .zip(now[front..].iter().rev())
            .take_while(|(a, b)| a.same(b))
            .count();
        *self = Frames::Span {
            at: front,
            old: old[front..old.len() - back].to_vec(),
            len: now.len() - front - back,
        };
    }

    /// All of the frame list again, from the frames the step left, which are as they were.
    fn reopen(&mut self, now: &[FrameInfo]) {
        if let Frames::Span { at, old, len } = self {
            let (at, end) = ((*at).min(now.len()), (*at + *len).min(now.len()));
            let whole = [&now[..at], &old[..], &now[end..]].concat();
            *self = Frames::Whole(whole);
        }
    }

    /// By timing only: a change that leaves nothing to see is put back whole, keys and all,
    /// so the frames are where they were.
    fn matches(&self, now: &[FrameInfo]) -> bool {
        match self {
            Frames::Whole(old) => old[..] == *now,
            Frames::Span { at, old, len } => now.get(*at..*at + *len) == Some(&old[..]),
        }
    }

    /// By timing and keys: the same frames, not only frames that play the same.
    fn same_keys(&self, now: &[FrameInfo]) -> bool {
        let same = |old: &[FrameInfo], now: &[FrameInfo]| {
            old.len() == now.len() && old.iter().zip(now).all(|(a, b)| a.same(b))
        };
        match self {
            Frames::Whole(old) => same(old, now),
            Frames::Span { at, old, len } => now.get(*at..*at + *len).is_some_and(|n| same(old, n)),
        }
    }

    fn bytes(&self) -> usize {
        let entries = match self {
            Frames::Whole(old) | Frames::Span { old, .. } => old.capacity(),
        };
        entries * size_of::<FrameInfo>()
    }
}

/// True if both tables are one, or hold the same tiles, so the document is as it was.
fn same_tilesets(a: &Arc<Tilesets>, b: &Arc<Tilesets>) -> bool {
    Arc::ptr_eq(a, b) || **a == **b
}

pub struct Snapshot {
    /// The canvas's width and height.
    size: (usize, usize),
    colors: Arc<ColorTable>,
    settings: Vec<Settings>,
    frames: Frames,
    tags: Vec<Tag>,
    frame_rate: Option<u8>,
    blending: crate::Blending,
    slices: Vec<Slice>,
    /// The tilesets from before the step. Kept whole while the step is open, and only if the
    /// step altered them once it is finished, so most steps keep none.
    tilesets: Option<Arc<Tilesets>>,
    place: Place,
    /// The selection to go back to, for a change that also applied floating pixels.
    selection: SavedSelection,
    /// What the change did to the layers and their cels.
    log: StepLog,
    /// True until the change is over and the step keeps only what it altered.
    open: bool,
    /// What the step is charged for the drawings its log keeps.
    charge: usize,
    /// What the step is charged for the tile images its saved tilesets keep.
    tile_charge: usize,
}

impl Snapshot {
    /// Takes what a change can alter apart from the cels, which the change logs as it goes.
    pub fn capture(doc: &Document, place: Place) -> Self {
        Snapshot {
            place,
            size: (doc.width, doc.height),
            colors: doc.colors.clone(),
            settings: doc.layers.iter().map(Settings::of).collect(),
            frames: Frames::Whole(doc.frames.clone()),
            tags: doc.tags.clone(),
            frame_rate: doc.frame_rate,
            blending: doc.blending,
            slices: doc.slices.clone(),
            tilesets: Some(doc.tilesets.clone()),
            selection: None,
            log: StepLog::default(),
            open: true,
            charge: 0,
            tile_charge: 0,
        }
    }

    /// Undo goes back to this selection too, unless a lift or paste the step took in already
    /// gave it an older one.
    pub(crate) fn keep_selection(&mut self, selection: Option<Arc<Mask>>) {
        self.selection.get_or_insert(selection);
    }

    /// Undo goes back past the lift or paste as well, to this selection and color table.
    pub(crate) fn include_floating(
        &mut self,
        selection: Option<Arc<Mask>>,
        colors: Option<Arc<ColorTable>>,
    ) {
        self.selection = Some(selection);
        if let Some(colors) = colors {
            self.colors = colors;
        }
    }

    /// The tilesets history keeps for this step: from before the change, once the step is
    /// finished and the change altered them.
    fn kept_tilesets(&self) -> Option<&Tilesets> {
        self.tilesets.as_deref().filter(|_| !self.open)
    }

    /// Counts the drawings the step keeps in history, and charges it for its share.
    pub(super) fn take_on(&mut self, held: &mut Held) {
        self.charge = held.take_on(&self.log);
        self.take_on_tiles(held);
    }

    /// Counts the tile images the step keeps, and charges it for its share. Called again when
    /// finishing the step settles what it keeps.
    pub(super) fn take_on_tiles(&mut self, held: &mut Held) {
        self.tile_charge = self
            .kept_tilesets()
            .map_or(0, |t| held.take_on_images(t.images()));
    }

    /// Lets go of the tile images the step keeps, before reopening it or playing it back.
    pub(super) fn let_go_tiles(&mut self, held: &mut Held) {
        if let Some(tilesets) = self.kept_tilesets() {
            held.let_go_images(tilesets.images());
        }
        self.tile_charge = 0;
    }

    /// Lets go of the drawings the step keeps, before it is played back or dropped.
    pub(super) fn let_go(&mut self, held: &mut Held) {
        held.let_go(&self.log);
        self.let_go_tiles(held);
        self.charge = 0;
    }

    /// What the step is charged for drawings, and the drawings its log keeps.
    #[cfg(test)]
    pub(super) fn charged_for(&self) -> (usize, impl Iterator<Item = &crate::layer::Cel>) {
        (
            self.charge + self.tile_charge,
            self.log.kept_cels().filter(|cel| !cel.is_blank()),
        )
    }

    /// What the step is charged for the tile images it keeps.
    #[cfg(test)]
    pub(super) fn tile_charge(&self) -> usize {
        self.tile_charge
    }

    /// The tile images the step keeps in history, which it may be charged for.
    #[cfg(test)]
    pub(super) fn kept_images(&self) -> impl Iterator<Item = &Arc<crate::image::Image>> {
        self.kept_tilesets().into_iter().flat_map(Tilesets::images)
    }

    /// The widest table among the cels the change put in the document.
    pub(crate) fn widest_put(&self, doc: &Document) -> usize {
        self.log.widest_put(&doc.layers)
    }

    /// Stops the document's log and keeps what it logged.
    pub(crate) fn take_log(&mut self, doc: &mut Document) {
        self.log = doc.layers.take_log();
    }

    /// Keeps only what the change altered, now that it is over.
    pub(crate) fn finish(&mut self, doc: &Document) {
        if !std::mem::replace(&mut self.open, false) {
            return;
        }
        self.frames.finish(&doc.frames);
        let log = &mut self.log;
        self.settings.retain(|saved| {
            match doc.layers.iter().find(|l| l.id() == saved.id) {
                Some(layer) => Settings::of(layer) != *saved,
                // A layer the step changed and then removed comes back by its log, so it comes
                // back with the settings from before the step.
                None => {
                    if let Some(layer) = log.removed_layer_mut(saved.id) {
                        saved.clone().swap(layer);
                    }
                    false
                }
            }
        });
        // The step is kept from now on, so the room for layers it left alone is let go.
        self.settings.shrink_to_fit();
        // A step that left the tilesets as they were keeps no reference to them, so the next
        // edit of a tile does not have to copy the table.
        if self
            .tilesets
            .as_ref()
            .is_some_and(|saved| same_tilesets(saved, &doc.tilesets))
        {
            self.tilesets = None;
        }
    }

    /// Undoes `finish`, which is exact while the document is as it was then: what finishing
    /// dropped is what the document still has.
    pub(crate) fn reopen(&mut self, doc: &Document) {
        if std::mem::replace(&mut self.open, true) {
            return;
        }
        self.frames.reopen(&doc.frames);
        let dropped: Vec<Settings> = doc
            .layers
            .iter()
            .filter(|l| self.settings.iter().all(|saved| saved.id != l.id()))
            .map(Settings::of)
            .collect();
        self.settings.extend(dropped);
        self.tilesets.get_or_insert_with(|| doc.tilesets.clone());
    }

    /// True if the document is still what it was before this step, so there is nothing to undo.
    /// As [`Self::matches`], and the frames keep their keys too.
    pub(crate) fn matches_keyed(&self, doc: &Document) -> bool {
        self.frames.same_keys(&doc.frames) && self.matches(doc)
    }

    pub(crate) fn matches(&self, doc: &Document) -> bool {
        let settings_same = self.settings.iter().all(|saved| {
            doc.layers
                .iter()
                .find(|l| l.id() == saved.id)
                .is_none_or(|l| Settings::of(l) == *saved)
        });
        // The cheap tests first: comparing the log may play it back on a copy of a layer.
        self.size == (doc.width, doc.height)
            && self.frames.matches(&doc.frames)
            && self.tags == doc.tags
            && self.frame_rate == doc.frame_rate
            && self.blending == doc.blending
            && self.slices == doc.slices
            && settings_same
            && self
                .tilesets
                .as_ref()
                .is_none_or(|saved| same_tilesets(saved, &doc.tilesets))
            && (Arc::ptr_eq(&self.colors, &doc.colors)
                || (self.colors.colors() == doc.colors.colors()
                    && self.colors.swatches() == doc.colors.swatches()
                    && self.colors.groups() == doc.colors.groups()))
            && self.log.changes_nothing(&doc.layers)
    }

    /// True until the change is over and the step keeps only what it altered.
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    /// False if undoing this step would still bring back a different selection.
    pub(super) fn keeps_selection(&self, current: Option<&Arc<Mask>>) -> bool {
        self.selection
            .as_ref()
            .is_none_or(|saved| saved.as_ref() == current)
    }

    /// Undoes or redoes the step, leaving what does the opposite.
    /// Puts back the cels a change that changed nothing to see left where they are, so that
    /// drawings that look the same and were swapped between frames go back to their own.
    pub(crate) fn put_back(mut self, doc: &mut Document) {
        let (mut place, mut selection) = (Place::default(), None);
        self.swap_with(doc, &mut place, &mut selection);
    }

    pub(super) fn swap_with(
        &mut self,
        doc: &mut Document,
        place: &mut Place,
        selection: &mut Option<Arc<Mask>>,
    ) {
        if let Some(saved) = &mut self.selection {
            std::mem::swap(saved, selection);
        }
        doc.layers.start_log();
        doc.layers.play_back(std::mem::take(&mut self.log));
        self.log = doc.layers.take_log();
        self.swap_rest(doc);
        std::mem::swap(&mut self.place, place);
    }

    fn swap_rest(&mut self, doc: &mut Document) {
        for saved in &mut self.settings {
            if let Some(layer) = doc.layers.iter_mut().find(|l| l.id() == saved.id) {
                saved.swap(layer);
            }
        }
        let size = (doc.width, doc.height);
        (doc.width, doc.height) = std::mem::replace(&mut self.size, size);
        std::mem::swap(&mut self.colors, &mut doc.colors);
        self.frames.swap(&mut doc.frames);
        std::mem::swap(&mut self.tags, &mut doc.tags);
        std::mem::swap(&mut self.frame_rate, &mut doc.frame_rate);
        std::mem::swap(&mut self.blending, &mut doc.blending);
        std::mem::swap(&mut self.slices, &mut doc.slices);
        if let Some(saved) = &mut self.tilesets {
            std::mem::swap(saved, &mut doc.tilesets);
        }
    }

    /// Puts the document back as it was, for a change that failed. Nothing is kept to redo.
    pub(crate) fn roll_back(mut self, doc: &mut Document) {
        doc.layers.play_back(std::mem::take(&mut self.log));
        self.swap_rest(doc);
    }

    /// What this step keeps: its log, its share of the drawings only history keeps, and the
    /// rest.
    pub(super) fn size(&self) -> usize {
        let shape: usize = self
            .selection
            .iter()
            .flatten()
            .map(|m| m.kept_bytes())
            .sum();
        let names: usize = self.tags.iter().map(|t| t.name.capacity()).sum();
        let tags = self.tags.capacity() * size_of::<Tag>() + names;
        let names: usize = self.slices.iter().map(|s| s.name.capacity()).sum();
        let slices = self.slices.capacity() * size_of::<Slice>() + names;
        let names: usize = self.settings.iter().map(|s| s.name.capacity()).sum();
        let settings = self.settings.capacity() * size_of::<Settings>() + names;
        // A color table the document no longer uses is the step's to keep.
        let colors = if Arc::strong_count(&self.colors) == 1 {
            self.colors.kept_bytes()
        } else {
            0
        };
        // The table the step keeps, besides the images charged for in `charge`.
        let tilesets = self
            .kept_tilesets()
            .map_or(0, |t| 2 * size_of::<usize>() + t.table_bytes());
        size_of::<Snapshot>()
            + tilesets
            + self.log.bytes()
            + self.charge
            + self.tile_charge
            + shape
            + tags
            + slices
            + settings
            + colors
            + self.frames.bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_step_keeps_room_only_for_the_settings_it_changed() {
        let mut doc = Document::new(4, 4, ColorMode::Rgba).expect("size");
        for at in 1..20 {
            doc.add_layer(at, ColorMode::Rgba).expect("room");
        }
        let mut step = Snapshot::capture(&doc, (0, 0));
        doc.layers_mut()[3].opacity = 9;
        step.finish(&doc);
        assert_eq!(step.settings.len(), 1);
        assert_eq!(
            step.settings.capacity(),
            1,
            "the other layers' room is let go"
        );
    }
}
