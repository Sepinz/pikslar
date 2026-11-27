//! What Rewind (`docs/rewind.md`) asks of the editor: to show the steps it holds one at a time,
//! oldest last, without changing anything, and to take a step read back from disk under the
//! oldest one it holds.

use super::Editor;
use crate::document::Document;
use crate::grid::Cell;
use crate::history::{Edit, RewindMarks};
use crate::ids::DocumentId;
use crate::image::TileKey;
use crate::layer::Split;
use crate::tile::TILE;
use std::sync::Arc;

/// One step, as the walk passes it.
pub enum StepView<'a> {
    /// The step changed nothing in the file, such as a selection.
    Unchanged,
    /// The step drew on one cel of a pixel layer, and undoing it puts these tiles back.
    Pixels {
        layer: usize,
        frame: usize,
        before: &'a [(TileKey, Cell)],
        /// Another frame of the layer showing the drawing this cel showed before the step,
        /// which the step split off from it: undo shows the two as one drawing again.
        shares: Option<usize>,
    },
    /// The whole document as it was before the step.
    Whole(&'a Document),
}

/// A step read back from disk, to go under the oldest step in memory.
pub enum OlderStep {
    Unchanged,
    Pixels {
        layer: usize,
        frame: usize,
        before: Vec<(TileKey, Cell)>,
        shares: Option<usize>,
    },
    Whole(Box<Document>),
}

/// Why the editor did not take a step or walk its history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewindRefusal {
    /// A stroke is in progress or pixels are floating.
    Busy,
    /// There is a step left to undo in memory, or none is missing below.
    NotAtBottom,
    /// The step does not fit this document: another layer kind, or out of range.
    DoesNotFit,
    /// History in memory is already over its budget: steps taken back would go past it.
    Full,
}

/// How far past the canvas, in canvases, a stored pixel step may reach.
const REACH: i32 = 2;

impl Editor {
    pub fn rewind_marks(&self) -> RewindMarks {
        self.history.rewind_marks()
    }

    /// The document had `by` steps before this session's: every position moves up by that.
    pub fn rebase_rewind(&mut self, by: u64) {
        self.history.rebase(by);
    }

    /// Every step in memory is now as written to disk.
    pub fn clear_rewind_rewrites(&mut self) {
        self.history.clear_rewrites();
    }

    /// A write of the steps from position `at` up failed: the next one writes them again.
    pub fn rewind_not_written_from(&mut self, at: u64) {
        self.history.not_written_from(at);
    }

    /// Gives the document its id. No step, and no change to save.
    pub fn set_document_id(&mut self, id: DocumentId) {
        self.doc.document_id = id;
    }

    /// The most tiles a stored pixel step on this document can name: every key within
    /// [`REACH`] canvases of the canvas, as [`Editor::push_older`] takes them. A reader refuses
    /// a step naming more before it holds them.
    pub fn rewind_tile_room(&self) -> usize {
        let span = |size: usize| (2 * REACH as usize + 1) * size / TILE + 2;
        span(self.doc.width) * span(self.doc.height)
    }

    fn rewind_busy(&self) -> bool {
        self.is_stroking() || self.floating.is_some()
    }

    /// Shows `visit` each step from the newest down to the one at position `to + 1`, with its
    /// position, then puts everything back. Steps are played on the editor's own document, as a
    /// scrub plays them, so nothing holds a copy of a drawing while steps play and every step's
    /// charge comes back as it was. The document, selection, place and history are as before.
    pub fn walk_back(
        &mut self,
        to: u64,
        visit: &mut dyn FnMut(u64, StepView<'_>),
    ) -> Result<(), RewindRefusal> {
        let marks = self.history.rewind_marks();
        if self.rewind_busy() {
            return Err(RewindRefusal::Busy);
        }
        if to < marks.below || to > marks.depth {
            return Err(RewindRefusal::DoesNotFit);
        }
        if to == marks.depth {
            return Ok(());
        }
        let room = self.history.redo_room();
        let mut place = (self.layer, self.frame);
        let mut played = 0;
        for at in (to + 1..=marks.depth).rev() {
            let whole = match self.history.newest_undo() {
                Some(edit) => match pixels_of(edit, &self.doc) {
                    Some(view) => {
                        visit(at, view);
                        false
                    }
                    None => edit.modifies(),
                },
                None => break,
            };
            let (doc, selection) = (&mut self.doc, &mut self.selection);
            if !self.history.undo(doc, selection, &mut place) {
                break;
            }
            played += 1;
            if whole {
                visit(at, StepView::Whole(&self.doc));
            } else if !self.history.redo_modifies() {
                visit(at, StepView::Unchanged);
            }
        }
        for _ in 0..played {
            self.history
                .redo(&mut self.doc, &mut self.selection, &mut place);
        }
        self.history.give_back_redo_room(room);
        Ok(())
    }

    /// Puts a step read back from disk under the oldest step in memory, so undo plays it next.
    /// Only while nothing is left to undo in memory and steps are missing below. Redo, the
    /// document and the revision are left as they are.
    pub fn push_older(&mut self, step: OlderStep) -> Result<(), RewindRefusal> {
        let marks = self.history.rewind_marks();
        if self.rewind_busy() {
            return Err(RewindRefusal::Busy);
        }
        if marks.depth != marks.below || marks.below == 0 {
            return Err(RewindRefusal::NotAtBottom);
        }
        // Redo is never trimmed (it is the only way back to the newest work), so the steps
        // taken back stop once memory is over budget: at most one step past it.
        if self.history.is_over_budget() {
            return Err(RewindRefusal::Full);
        }
        let edit = match step {
            OlderStep::Unchanged => Edit::Selection(self.selection.clone()),
            OlderStep::Pixels {
                layer,
                frame,
                before,
                shares,
            } => self.older_pixels((layer, frame), before, shares)?,
            OlderStep::Whole(doc) => return self.push_older_whole(*doc),
        };
        match self.history.push_older(edit) {
            true => Ok(()),
            false => Err(RewindRefusal::NotAtBottom),
        }
    }

    /// A pixel step, if it fits the cel it names.
    fn older_pixels(
        &self,
        (layer, frame): (usize, usize),
        before: Vec<(TileKey, Cell)>,
        shares: Option<usize>,
    ) -> Result<Edit, RewindRefusal> {
        let target = self.doc.layers.get(layer).filter(|l| !l.is_tilemap());
        let Some(target) = target.filter(|l| frame < l.frame_count()) else {
            return Err(RewindRefusal::DoesNotFit);
        };
        let cel = target.cel(frame);
        if before.is_empty() || !keys_fit(&self.doc, cel.position(), &before) {
            return Err(RewindRefusal::DoesNotFit);
        }
        // What the tiles it will keep once played take, less what it keeps now.
        let image = cel.image();
        let now: usize = before
            .iter()
            .filter_map(|(key, _)| image.tile(key.x.into(), key.y.into()))
            .map(crate::tile::Tile::heap_size)
            .sum();
        let kept = crate::history::tile_bytes(&before) as i64;
        let most = i64::from(i32::MAX);
        let swing = (now as i64 - kept).clamp(-most, most) as i32;
        Ok(Edit::Tiles {
            // Checked against the document, whose limits fit 16 bits.
            layer: layer as u16,
            frame: frame as u16,
            left: 0,
            swing,
            changes: before.into_boxed_slice(),
            selection: None,
            colors: None,
            split: shares
                .filter(|&g| g != frame)
                .and_then(|g| target.drawing(g))
                .map(Arc::downgrade),
        })
    }

    /// Takes `older` as the state beneath the oldest step: the document becomes `older`, then
    /// takes on what it is now as one structural step, recorded under the others.
    fn push_older_whole(&mut self, older: Document) -> Result<(), RewindRefusal> {
        let place = (self.layer, self.frame);
        let mut current = std::mem::replace(&mut self.doc, older);
        // Ids, carried sections and the file's id belong to the document as it is now.
        self.doc.take_on_floor(&current);
        keep_views(&mut self.doc, &current);
        let (id, foreign) = (current.document_id, std::mem::take(&mut current.foreign));
        let resized = (current.width, current.height) != (self.doc.width, self.doc.height);
        let before = self.history.undo_depth();
        let change = |doc: &mut Document| {
            super::transaction::take_on_document(doc, current);
            Ok(())
        };
        self.recording_older = true;
        self.keyed_frames = true;
        let took = match resized {
            true => self.transact_canvas(change),
            false => self.transact(change),
        };
        (self.recording_older, self.keyed_frames) = (false, false);
        (self.doc.document_id, self.doc.foreign) = (id, foreign);
        self.layer = place.0.min(self.doc.layers.len() - 1);
        self.frame = place.1.min(self.doc.frames.len() - 1);
        if took.is_err() {
            return Err(RewindRefusal::DoesNotFit);
        }
        if self.history.undo_depth() == before {
            // The two are the same work: the step changed nothing to see.
            return self.push_older(OlderStep::Unchanged);
        }
        Ok(())
    }
}

/// Shows, locks and folds the layers of a stored document as their namesakes are now: how the
/// artist looks at a layer is never undone, from memory or from disk. Layers are matched by the
/// number the file keeps for them.
fn keep_views(older: &mut Document, now: &Document) {
    for layer in older.layers.iter_mut() {
        let id = layer.persisted_id();
        let same = now.layers.iter().find(|l| l.persisted_id() == id);
        if let Some(same) = same.filter(|_| id.get() != 0) {
            (layer.visible, layer.locked, layer.folded) = (same.visible, same.locked, same.folded);
        }
    }
}

/// The step as pixels on one cel of a pixel layer, if it is only that.
fn pixels_of<'a>(edit: &'a Edit, doc: &Document) -> Option<StepView<'a>> {
    let Edit::Tiles {
        layer,
        frame,
        changes,
        colors: None,
        split,
        ..
    } = edit
    else {
        return None;
    };
    let (layer, frame) = (*layer as usize, *frame as usize);
    let target = doc.layers.get(layer).filter(|l| !l.is_tilemap())?;
    (!changes.is_empty()).then(|| StepView::Pixels {
        layer,
        frame,
        before: changes,
        shares: sharing_frame(target, frame, split),
    })
}

/// Another frame showing the drawing a pixel step split off, if one does.
fn sharing_frame(layer: &crate::layer::Layer, frame: usize, split: &Split) -> Option<usize> {
    let old = split.as_ref()?.upgrade()?;
    (0..layer.frame_count())
        .filter(|&g| g != frame)
        .find(|&g| layer.drawing(g).is_some_and(|d| Arc::ptr_eq(d, &old)))
}

/// True if every key is distinct and within [`REACH`] canvases of the canvas, in the cel's
/// own coordinates, so a stored step can never build a table much larger than the canvas.
fn keys_fit(doc: &Document, (cx, cy): (i16, i16), changes: &[(TileKey, Cell)]) -> bool {
    let tile = TILE as i32;
    let (w, h) = (doc.width as i32, doc.height as i32);
    let span = |origin: i16, size: i32| {
        let first = (-i32::from(origin) - REACH * size).div_euclid(tile);
        let last = (-i32::from(origin) + (REACH + 1) * size).div_euclid(tile);
        first..=last
    };
    let (xs, ys) = (span(cx, w), span(cy, h));
    let inside = changes
        .iter()
        .all(|(k, _)| xs.contains(&i32::from(k.x)) && ys.contains(&i32::from(k.y)));
    let mut keys: Vec<(i16, i16)> = changes.iter().map(|(k, _)| (k.x, k.y)).collect();
    keys.sort_unstable();
    inside && keys.windows(2).all(|pair| pair[0] != pair[1])
}
