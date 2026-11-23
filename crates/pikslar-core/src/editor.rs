//! Editing state and tools: everything between pointer input and committed tiles.

use crate::color::ColorTable;
use crate::document::{ColorMode, Document, LimitError};
use crate::floating::Floating;
use crate::grid::Cell;
use crate::history::{Edit, History, SavedSelection, Snapshot};
use crate::image::TileKey;
use crate::layer::{CanvasWriter, Cel, Split};
use crate::mask::Mask;
use crate::overlay::Overlay;
use crate::rhythm::Trail;
use crate::tile::{TILE, TILE_AREA};
use std::borrow::Cow;
use std::sync::Arc;

#[cfg(test)]
mod brush_path_tests;
mod brush_stroke;
#[cfg(test)]
mod brush_stroke_tests;
mod draw;
mod rewind;
#[cfg(test)]
mod rewind_tests;
mod terrain_stroke;
mod tile_stroke;
mod transaction;
#[cfg(test)]
mod transaction_tests;
/// The tiles of `cel` the overlay changes, as they are with it, telling `changed` each one.
fn overlaid(overlay: &Overlay, cel: &Cel, mut changed: impl FnMut(u32)) -> Vec<(TileKey, Cell)> {
    let mut raw = [0u32; TILE_AREA];
    let tiles_wide = overlay.tiles_wide();
    let mut writer = CanvasWriter::new(cel);
    let mut order = Cow::Borrowed(overlay.touched());
    if writer.needs_rows() {
        order.to_mut().sort_unstable();
    }
    for &index in order.iter() {
        let Some(over) = overlay.tile(index as usize) else {
            continue;
        };
        let (tx, ty) = (index as usize % tiles_wide, index as usize / tiles_wide);
        cel.decode(tx, ty, &mut raw);
        let before = raw;
        over.apply(&mut raw);
        if raw == before {
            continue;
        }
        writer.put(tx, ty, &raw);
        changed(index);
    }
    writer.finish()
}

mod saved;
#[cfg(test)]
mod saved_tests;
#[cfg(test)]
mod scrub_tests;
#[cfg(test)]
mod tests;
mod undo;

pub use draw::for_line;
pub use rewind::{OlderStep, RewindRefusal, StepView};
pub use terrain_stroke::TerrainPreview;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pencil,
    Eraser,
    Fill,
    Picker,
    Line,
    Rect,
}

/// Which part of the canvas needs recompositing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Damage {
    #[default]
    None,
    Tiles(Vec<u32>),
    /// The pixels of some tiles of a tileset changed: only the canvas tiles where a shown map
    /// places one of them need drawing again. Which those are depends on the frames shown, so
    /// whoever draws finds them with [`crate::Document::placed_canvas_tiles`].
    Placed(crate::tilemap::TileEdit),
    All,
}

impl Damage {
    fn add(&mut self, index: u32) {
        match self {
            Damage::None => {
                // Room for a wide stamp's tiles, so a stroke event allocates this once.
                let mut tiles = Vec::with_capacity(16);
                tiles.push(index);
                *self = Damage::Tiles(tiles);
            }
            Damage::Tiles(v) => {
                if v.last() != Some(&index) {
                    v.push(index);
                }
            }
            // The two kinds of damage are not kept together: drawing everything is right for both.
            Damage::Placed(_) => *self = Damage::All,
            Damage::All => {}
        }
    }

    /// Adds a tile edit. Alone, or with another edit of the same tileset, it stays one; next
    /// to anything else everything is drawn again.
    pub(crate) fn add_tile_edit(&mut self, edit: crate::tilemap::TileEdit) {
        match self {
            Damage::None => *self = Damage::Placed(edit),
            Damage::Placed(own) => {
                if !own.join(&edit) {
                    *self = Damage::All;
                }
            }
            Damage::Tiles(_) | Damage::All => *self = Damage::All,
        }
    }
}

struct Stroke {
    tool: Tool,
    value: u32,
    start: (i32, i32),
    last: (i32, i32),
    /// Recent pencil points, for removing L-shaped corners.
    trail: Vec<(i32, i32)>,
    rhythm: Option<Box<Trail>>,
    /// The color table before this stroke added its color, to go back to if it comes to nothing.
    colors_before: Option<Arc<crate::color::ColorTable>>,
    /// The brush made ready for this stroke, when it is not the round one.
    stamp: Option<Box<crate::custom_brush::Stamp>>,
}

/// Everything outside this crate reads through accessors and changes the document by command.
///
/// ```
/// use pikslar_core::{ColorMode, Document, Editor, FrameCmd};
/// let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).unwrap());
/// e.apply(FrameCmd::Add { at: 1, copy: false }).unwrap();
/// assert_eq!(e.doc().frames.len(), 2);
/// ```
///
/// Reaching past a command does not compile:
///
/// ```compile_fail
/// use pikslar_core::{ColorMode, Document, Editor};
/// let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).unwrap());
/// e.doc.frames.clear();
/// ```
///
/// ```compile_fail
/// use pikslar_core::{ColorMode, Document, Editor};
/// let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).unwrap());
/// e.undo();
/// ```
pub struct Editor {
    pub(crate) doc: Document,
    pub(crate) history: History,
    pub layer: usize,
    pub frame: usize,
    /// Straight-alpha foreground color.
    pub color: u32,
    pub pixel_perfect: bool,
    /// Resolve the next pencil stroke into clean step patterns.
    pub rhythm: bool,
    /// Brush diameter in pixels.
    pub brush_size: i32,
    /// Bumped on every change that would need saving.
    pub(crate) revision: u64,
    pub(crate) overlay: Overlay,
    pub(crate) selection: Option<Arc<Mask>>,
    pub(crate) selection_version: u64,
    /// The selection was made just to pick up the layer, so it is no step of its own.
    pub(crate) selection_temporary: bool,
    /// How many of the selection's version changes only moved it.
    pub(crate) selection_moved: u64,
    pub(crate) floating_epoch: u64,
    pub(crate) floating: Option<Floating>,
    stroke: Option<Stroke>,
    damage: Damage,
    /// What is being dragged and how many steps had been recorded, so a drag is one undo step.
    /// How many times the revision has gone back to an earlier number.
    pub(crate) revision_rewinds: u64,
    /// What is being dragged, the step it joins, and the revision before that step.
    pub(crate) drag_mark: Option<(crate::steps::Dragged, u64, u64)>,
    pub(crate) memory_check: crate::command::MemoryCheck,
    /// The largest table any cel has had, so a rewind that regrows one can ask for it.
    pub(crate) widest_table: usize,
    /// While a transaction takes on its run: frames with new keys are a change even with
    /// the same timing, so the handles the run gave out keep naming them.
    pub(crate) keyed_frames: bool,
    /// The step being recorded goes under the oldest one, read back by Rewind.
    pub(crate) recording_older: bool,
    /// The brush in use and the recent ones: view state, never an undo step.
    pub(crate) brushes: crate::custom_brush::BrushState,
    /// The tile, stamp and mode chosen for tilemap layers: view state, never an undo step.
    pub(crate) tiles: crate::tilemap::TileState,
}

/// The largest tile table of any cel in the document, in bytes.
fn widest_table(doc: &Document) -> usize {
    doc.layers
        .iter()
        .flat_map(|layer| layer.cels())
        .map(Cel::table_bytes)
        .max()
        .unwrap_or(0)
}

impl Editor {
    /// An editor whose undo history may hold this many bytes.
    pub fn with_undo_budget(doc: Document, budget: usize) -> Self {
        Editor {
            history: History::with_budget(budget),
            ..Self::new(doc)
        }
    }

    pub fn new(doc: Document) -> Self {
        let overlay = Overlay::new(doc.width, doc.height);
        let widest_table = widest_table(&doc);
        Editor {
            doc,
            history: History::new(),
            layer: 0,
            frame: 0,
            color: crate::color::rgba(255, 255, 255, 255),
            pixel_perfect: true,
            rhythm: false,
            brush_size: 1,
            revision: 0,
            overlay,
            selection: None,
            selection_version: 0,
            selection_temporary: false,
            selection_moved: 0,
            floating_epoch: 0,
            floating: None,
            stroke: None,
            damage: Damage::All,
            drag_mark: None,
            revision_rewinds: 0,
            memory_check: crate::command::system_has_room,
            widest_table,
            keyed_frames: false,
            recording_older: false,
            brushes: crate::custom_brush::BrushState::default(),
            tiles: crate::tilemap::TileState::default(),
        }
    }

    /// The custom brushes, for reading.
    pub fn brushes(&self) -> &crate::custom_brush::BrushState {
        &self.brushes
    }

    /// The tilemap editing state, for reading.
    pub fn tiles(&self) -> &crate::tilemap::TileState {
        &self.tiles
    }

    /// The document, for reading.
    pub fn doc(&self) -> &Document {
        &self.doc
    }

    /// The document as it is shown: floating pixels written where they are, clipped at the
    /// canvas edge, into a copy that shares everything else. Nothing the document, history,
    /// selection or floating pixels hold changes; only the preview is brought up to date.
    pub fn shown_document(&mut self) -> Cow<'_, Document> {
        self.settle_floating();
        let shown = self.floating_stamp() != 0;
        if !shown || self.overlay.is_empty() || self.frame >= self.doc.frames.len() {
            return Cow::Borrowed(&self.doc);
        }
        let mut doc = self.doc.clone();
        let cel = doc.layers[self.layer].cel(self.frame);
        let mut changes = overlaid(&self.overlay, cel, |_| {});
        doc.swap_tiles(self.layer, self.frame, &mut changes, &mut None);
        Cow::Owned(doc)
    }

    /// With the revision, tells every change apart even when a step that came to nothing has
    /// given its revision number back.
    pub fn revision_rewinds(&self) -> u64 {
        self.revision_rewinds
    }

    /// Moves whenever the document changes, so anything derived from it knows to look again.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    /// Bounds undo history to `budget` bytes from the next step on. For measurement runs only
    /// (`--undo-budget-kib`); the program itself always keeps the default budget.
    pub fn set_undo_budget(&mut self, budget: usize) {
        self.history.set_budget(budget);
    }

    pub fn overlay(&self) -> Option<(usize, &Overlay)> {
        debug_assert!(
            !self.floating_is_stale(),
            "settle before reading the overlay"
        );
        (self.stroke.is_some() || self.floating.is_some()).then_some((self.layer, &self.overlay))
    }

    pub fn is_stroking(&self) -> bool {
        self.stroke.is_some() || self.tiles.stroke.is_some() || self.tiles.terrain.is_some()
    }

    /// Returns what changed since the last call.
    pub fn take_damage(&mut self) -> Damage {
        self.settle_floating();
        std::mem::take(&mut self.damage)
    }

    /// True if something other than a floating move changed since `take_damage`.
    pub fn has_damage(&self) -> bool {
        !matches!(self.damage, Damage::None)
    }

    pub fn damage_all(&mut self) {
        self.damage = Damage::All;
    }

    /// Marks for redrawing what looks different now that frame `from` gave way to the frame
    /// shown, by [`Document::frame_change_tiles`]: all of it while a stroke or floating pixels
    /// are in progress, or when most of the canvas changed. Whoever draws neighbouring frames
    /// beneath the shown one (an onion skin) must damage everything instead.
    pub fn damage_frame_change(&mut self, from: usize) {
        let frames = self.doc.frames.len();
        if from == self.frame || matches!(self.damage, Damage::All) {
            return;
        }
        if from >= frames || self.frame >= frames || self.is_floating() || self.is_stroking() {
            self.damage_all();
            return;
        }
        let total = self.doc.width.div_ceil(TILE) * self.doc.height.div_ceil(TILE);
        let mut drawn = Vec::new();
        self.doc.screen_layers_into(&mut drawn);
        let mut seen = vec![0u64; total.div_ceil(64)];
        let mut tiles = Vec::new();
        // Past half the canvas, drawing it all in order is as quick and lists nothing, so the
        // looking stops there.
        let most = total / 2;
        self.doc
            .frame_change_tiles((from, self.frame), &drawn, |index| {
                let (word, bit) = (index / 64, 1u64 << (index % 64));
                if let Some(w) = seen.get_mut(word).filter(|w| **w & bit == 0) {
                    *w |= bit;
                    tiles.push(index as u32);
                }
                match tiles.len() > most {
                    true => std::ops::ControlFlow::Break(()),
                    false => std::ops::ControlFlow::Continue(()),
                }
            });
        if tiles.len() > most {
            self.damage_all();
            return;
        }
        for index in tiles {
            self.damage.add(index);
        }
    }

    /// The active cel, read in canvas positions.
    pub(crate) fn cel(&self) -> &Cel {
        self.doc.layers[self.layer].cel(self.frame)
    }

    /// True while the active layer is a tilemap layer, whose cels hold cells, not pixels. The
    /// picker, the wand, Select Opaque, Copy and the code that shows a map read the pixels it
    /// draws with [`Document::layer_pixel`] and [`Document::layer_tile`].
    pub(crate) fn on_tilemap(&self) -> bool {
        self.doc.layers[self.layer].is_tilemap()
    }

    pub(crate) fn mode(&self) -> ColorMode {
        self.doc.layers[self.layer].mode
    }

    /// The color visible in the active layer's cell, as straight-alpha RGBA.
    pub fn color_at(&self, x: i32, y: i32) -> u32 {
        let outside =
            x < 0 || y < 0 || x as usize >= self.doc.width || y as usize >= self.doc.height;
        if outside {
            return 0;
        }
        let cell = self
            .doc
            .layer_pixel(&self.doc.layers[self.layer], self.frame, x, y);
        match self.mode() {
            ColorMode::Rgba => cell,
            ColorMode::Palette => self.doc.colors.color(cell),
        }
    }

    /// Discards the stroke in progress.
    pub(crate) fn cancel(&mut self) {
        if self.floating.is_some() {
            return;
        }
        self.cancel_tile_stroke();
        if let Some(stroke) = self.stroke.take() {
            for &t in self.overlay.touched() {
                self.damage.add(t);
            }
            if let Some(colors) = stroke.colors_before {
                self.doc.colors = colors;
            }
        }
        self.overlay.clear();
    }

    /// Moves the stroke into the document as one undo step.
    pub(crate) fn commit(&mut self) {
        // A stroke ended by another action keeps what it can; only its own Commit reports a
        // stroke that fell wholly off a map's tiles, and the count stays for the status line.
        if self.finish_stroke().is_err() {
            debug_assert!(self.tiles.off_tiles > 0);
        }
    }

    /// Writes the overlay into the active cel as one undo step, if that changed anything.
    pub(crate) fn commit_overlay(
        &mut self,
        selection: SavedSelection,
        colors: Option<Arc<ColorTable>>,
    ) {
        let (changes, split, swing) = self.write_overlay();
        let selection = selection.filter(|saved| *saved != self.selection);
        if changes.is_empty() && selection.is_none() {
            if let Some(colors) = colors {
                self.doc.colors = colors;
            }
            return;
        }
        self.record(Edit::Tiles {
            // Within a document's limits, which history checks fit 16 bits.
            layer: self.layer as u16,
            frame: self.frame as u16,
            left: 0,
            swing,
            changes: changes.into_boxed_slice(),
            selection,
            colors,
            split,
        });
    }

    /// Writes the overlay into the active cel and returns what it replaced, the drawing it split
    /// off if frames or history shared it, and how much more its tiles take than those replaced.
    pub(crate) fn write_overlay(&mut self) -> (Vec<(TileKey, Cell)>, Split, i32) {
        let (layer, frame) = (self.layer, self.frame);
        if frame >= self.doc.layers[layer].frame_count() {
            self.overlay.clear();
            return (Vec::new(), None, 0);
        }
        let damage = &mut self.damage;
        let cel = self.doc.layers[layer].cel(frame);
        let mut changes = overlaid(&self.overlay, cel, |i| damage.add(i));
        self.overlay.clear();
        let (mut split, mut swing) = (None, 0);
        if !changes.is_empty() {
            let written = crate::history::tile_bytes(&changes);
            if self.doc.swap_tiles(layer, frame, &mut changes, &mut split) {
                // Other layers show the change too.
                self.damage = Damage::All;
            }
            let replaced = crate::history::tile_bytes(&changes);
            // Clamped so that its sign can always turn. No canvas comes near.
            let most = i64::from(i32::MAX);
            swing = (written as i64 - replaced as i64).clamp(-most, most) as i32;

            let table = self.doc.layers[layer].cel(frame).table_bytes();
            self.widest_table = self.widest_table.max(table);
        }
        (changes, split, swing)
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot::capture(&self.doc, (self.layer, self.frame))
    }

    /// Adds one step to the undo history. Every step passes through here.
    pub(crate) fn record(&mut self, edit: Edit) {
        self.record_with(edit, false);
    }

    /// Records the step of a value in progress, which leaves redo as it was if it comes to
    /// nothing.
    pub(crate) fn record_joined(&mut self, edit: Edit) {
        self.record_with(edit, true);
    }

    fn record_with(&mut self, edit: Edit, set_redo_aside: bool) {
        let modifies = edit.modifies();
        // A step on top ends a value still open below it. Pixels, cel positions and selections
        // leave the frames and settings that finishing looks at, so finishing now is exact. A
        // structural step finished the value before its change began.
        if !matches!(edit, Edit::Snapshot(_)) {
            self.history.finish_last(&self.doc);
        }
        if self.recording_older {
            // Beneath everything, so the document as it is now is unchanged by it.
            self.history.push_older(edit);
            return;
        }
        if set_redo_aside {
            self.history.push_setting_redo_aside(edit);
        } else {
            self.history.push(edit);
        }
        self.revision += u64::from(modifies);
    }

    /// Makes the overlay again if the canvas changed size, as resizing or undoing it does. It
    /// is empty whenever the canvas can change.
    pub(crate) fn fit_overlay(&mut self) {
        let size = (self.doc.width, self.doc.height);
        if self.overlay.size() != size {
            self.overlay = Overlay::new(size.0, size.1);
        }
    }

    /// Runs a change to the document's structure as one step, or not at all.
    pub(crate) fn transact<R>(
        &mut self,
        change: impl FnOnce(&mut Document) -> Result<R, LimitError>,
    ) -> Result<R, LimitError> {
        self.transact_with(change, false, false)
    }

    /// Runs a change to the canvas itself as one step, or not at all. A selection is a mask of
    /// the old canvas, so a change that is kept clears it, and undo brings it back.
    pub(crate) fn transact_canvas<R>(
        &mut self,
        change: impl FnOnce(&mut Document) -> Result<R, LimitError>,
    ) -> Result<R, LimitError> {
        self.transact_with(change, false, true)
    }

    /// Runs a change to the document's structure as the step of a value in progress, which
    /// leaves redo as it was if the value comes to nothing.
    pub(crate) fn transact_joined<R>(
        &mut self,
        change: impl FnOnce(&mut Document) -> Result<R, LimitError>,
    ) -> Result<R, LimitError> {
        self.transact_with(change, true, false)
    }

    /// Hands the change to one shared body, so each command's change does not make a copy of
    /// the whole step machinery.
    fn transact_with<R>(
        &mut self,
        change: impl FnOnce(&mut Document) -> Result<R, LimitError>,
        set_redo_aside: bool,
        clear_selection: bool,
    ) -> Result<R, LimitError> {
        let (mut change, mut result) = (Some(change), None);
        let mut run = |doc: &mut Document| {
            let change = change.take().ok_or(LimitError::NoSuchItem)?;
            result = Some(change(doc)?);
            Ok(())
        };
        self.transact_dyn(&mut run, set_redo_aside, clear_selection)?;
        result.ok_or(LimitError::NoSuchItem)
    }

    #[inline(never)]
    fn transact_dyn(
        &mut self,
        change: &mut dyn FnMut(&mut Document) -> Result<(), LimitError>,
        set_redo_aside: bool,
        clear_selection: bool,
    ) -> Result<(), LimitError> {
        self.with_open_step_finished(|e| e.transact_now(change, set_redo_aside, clear_selection))
    }

    /// In a debug build, stops at once when the document breaks a rule of the links, naming
    /// what broke it. Costs one link table, so release builds skip it.
    pub(crate) fn debug_check_links(&self, after: &str) {
        if cfg!(debug_assertions) {
            if let Err(fault) = self.doc.check_links() {
                panic!("{after} broke a rule of the links: {fault}");
            }
        }
    }

    /// In a debug build, stops at once when the document breaks a rule of its tilemap layers,
    /// naming what broke it. One pass over each distinct map drawing, and nothing at all for a
    /// document without tilemap layers, so release builds skip it.
    pub(crate) fn debug_check_tilemaps(&self, after: &str) {
        if cfg!(debug_assertions) {
            if let Err(fault) = self.doc.check_tilemaps() {
                panic!("{after} broke a rule of the tilemap layers: {fault}");
            }
        }
    }

    /// Every check of the document's own rules that debug builds make after a change.
    pub(crate) fn debug_check(&self, after: &str) {
        self.debug_check_links(after);
        self.debug_check_tilemaps(after);
    }

    /// A change to the canvas clears the selection, and undo brings it back. A stored step goes
    /// beneath instead: the selection is of the canvas as it is now and stays, and the older
    /// canvas had none that is known.
    fn clear_selection_into(&mut self, snapshot: &mut Snapshot) {
        if self.recording_older {
            snapshot.keep_selection(None);
        } else {
            snapshot.keep_selection(self.selection.clone());
            self.set_selection(None);
        }
    }

    fn transact_now(
        &mut self,
        change: &mut dyn FnMut(&mut Document) -> Result<(), LimitError>,
        set_redo_aside: bool,
        clear_selection: bool,
    ) -> Result<(), LimitError> {
        self.cancel();
        let mut snapshot = self.snapshot();
        self.doc.layers.start_log();
        let held = self.absorb_floating(&mut snapshot);
        let outcome = change(&mut self.doc);
        self.fit_overlay();
        snapshot.take_log(&mut self.doc);
        match outcome {
            Ok(()) => {
                snapshot.finish(&self.doc);
                let wrote = held.as_ref().is_some_and(|h| h.wrote_pixels());
                // A turn or a move with floating pixels can make a table wider than any before.
                let widest = match wrote {
                    true => widest_table(&self.doc),
                    false => snapshot.widest_put(&self.doc),
                };
                self.widest_table = self.widest_table.max(widest);
                let same = match self.keyed_frames {
                    true => snapshot.matches_keyed(&self.doc),
                    false => snapshot.matches(&self.doc),
                };
                if wrote || !same {
                    drop(held); // first, so a table it shared is charged to the step
                    if clear_selection {
                        self.clear_selection_into(&mut snapshot);
                    }
                    self.record_with(Edit::Snapshot(Box::new(snapshot)), set_redo_aside);
                } else {
                    // Changed nothing to see: what it moved goes back to its own place, and
                    // floating pixels it took in are given back, as when a change fails.
                    snapshot.put_back(&mut self.doc);
                    self.give_back_floating(held);
                }
                self.layer = self.layer.min(self.doc.layers.len() - 1);
                self.frame = self.frame.min(self.doc.frames.len() - 1);
                self.damage = Damage::All;
                self.debug_check("a command");
                Ok(())
            }
            Err(error) => {
                snapshot.roll_back(&mut self.doc);
                self.give_back_floating(held);
                self.debug_check("a refused command");
                Err(error)
            }
        }
    }
}
