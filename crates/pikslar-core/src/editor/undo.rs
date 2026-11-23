//! Undo, redo and scrubbing through history.

use super::{Damage, Editor};
use crate::history::Place;
use crate::tilemap::TileEdit;
use crate::tileset::{TileId, TilesetId};
use std::cmp::Ordering;

/// What the steps a scrub played since the editor was last settled add up to.
#[derive(Default)]
struct Played {
    steps: u64,
    modifying: u64,
}

impl Editor {
    /// Walks the history until exactly `depth` steps remain undoable.
    ///
    /// The first step settles the editor in full, as undo does: it ends a stroke in progress,
    /// a value in progress and what was set aside to redo. Playing a step starts none of them
    /// again, so the steps after it settle only what each one changes, once at the end. Floating
    /// pixels are stepped through one at a time, as undo does.
    pub(crate) fn scrub_to(&mut self, depth: usize) {
        let first = match self.history.undo_depth().cmp(&depth) {
            Ordering::Greater => self.undo(),
            Ordering::Less => self.redo(),
            Ordering::Equal => false,
        };
        if !first {
            return;
        }
        if self.floating.is_some() {
            while self.history.undo_depth() > depth && self.undo() {}
            while self.history.undo_depth() < depth && self.redo() {}
            return;
        }
        self.play_settled(depth);
    }

    /// Plays the steps to `depth` on a settled editor. A step of tile pixels is played in full,
    /// as its damage depends on the frame shown when it is played.
    fn play_settled(&mut self, depth: usize) {
        let mut place = (self.layer, self.frame);
        let mut played = Played::default();
        loop {
            let redo = match self.history.undo_depth().cmp(&depth) {
                Ordering::Greater => false,
                Ordering::Less => true,
                Ordering::Equal => break,
            };
            let done = match self.history.next_tile_edit(redo) {
                Some(_) => {
                    let done = self.play_in_full(redo, place, std::mem::take(&mut played));
                    place = (self.layer, self.frame);
                    done
                }
                None => self.play_one(redo, &mut place, &mut played),
            };
            if !done {
                break;
            }
        }
        self.settle_played(place, played);
    }

    /// Plays one step without settling the editor, keeping `place` in the document as settling
    /// would. Says whether there was a step.
    #[inline(always)]
    fn play_one(&mut self, redo: bool, place: &mut Place, played: &mut Played) -> bool {
        let (doc, selection) = (&mut self.doc, &mut self.selection);
        let (modifies, done) = match redo {
            true => (
                self.history.redo_modifies(),
                self.history.redo(doc, selection, place),
            ),
            false => (
                self.history.undo_modifies(),
                self.history.undo(doc, selection, place),
            ),
        };
        if done {
            *place = (
                place.0.min(self.doc.layers.len() - 1),
                place.1.min(self.doc.frames.len() - 1),
            );
            played.steps += 1;
            played.modifying += u64::from(modifies);
            self.debug_check("undo or redo");
        }
        done
    }

    /// Settles what was played so far, then plays the next step as undo or redo does.
    #[cold]
    #[inline(never)]
    fn play_in_full(&mut self, redo: bool, place: Place, played: Played) -> bool {
        self.settle_played(place, played);
        match redo {
            true => self.redo(),
            false => self.undo(),
        }
    }

    /// Settles the editor once for steps played without it, as settling after each would have.
    fn settle_played(&mut self, (layer, frame): Place, played: Played) {
        if played.steps == 0 {
            return;
        }
        self.fit_overlay();
        self.selection_temporary = false;
        self.selection_version += played.steps;
        (self.layer, self.frame) = (layer, frame);
        self.damage = Damage::All;
        self.revision += played.modifying;
    }

    pub(crate) fn undo(&mut self) -> bool {
        if self.floating.is_some() {
            if !self.step_floating(true) {
                self.revert_floating();
            }
            return true;
        }
        self.cancel();
        let mut place = (self.layer, self.frame);
        let modifies = self.history.undo_modifies();
        let edit = self.history.next_tile_edit(false);
        let done = self
            .history
            .undo(&mut self.doc, &mut self.selection, &mut place);
        self.after_history(done, modifies, place, edit)
    }

    pub(crate) fn redo(&mut self) -> bool {
        if self.floating.is_some() {
            return self.step_floating(false);
        }
        self.cancel();
        let mut place = (self.layer, self.frame);
        let modifies = self.history.redo_modifies();
        let edit = self.history.next_tile_edit(true);
        let done = self
            .history
            .redo(&mut self.doc, &mut self.selection, &mut place);
        self.after_history(done, modifies, place, edit)
    }

    /// Settles the editor after a step was played. `edit` is the tile edit the step was, if it
    /// was one: that changes only where a map places the tiles, unless the step also brought
    /// another frame into view.
    #[inline(always)]
    pub(crate) fn after_history(
        &mut self,
        done: bool,
        modifies: bool,
        (layer, frame): Place,
        edit: Option<TileEdit>,
    ) -> bool {
        self.end_join();
        self.fit_overlay();
        if done {
            self.selection_temporary = false;
            self.selection_version += 1;
            self.layer = layer.min(self.doc.layers.len() - 1);
            let shown = self.frame;
            self.frame = frame.min(self.doc.frames.len() - 1);
            match edit {
                None => self.damage = Damage::All,
                Some(edit) => self.damage_after_tile_step(edit, self.frame == shown),
            }
            self.revision += modifies as u64;
            self.debug_check("undo or redo");
        }
        done
    }

    /// Notes that the pixels of these tiles of a tileset changed, so that only the canvas tiles
    /// where a shown map places them are drawn again.
    pub(crate) fn damage_tile_edit(&mut self, tileset: TilesetId, tiles: &[TileId]) {
        self.damage.add_tile_edit(TileEdit::new(tileset, tiles));
    }
}

impl Editor {
    /// Marks what a played step of tile pixels changed: only where a map places the tiles if the
    /// same frame is shown, everything otherwise. Out of line, as most steps are not one.
    #[cold]
    #[inline(never)]
    fn damage_after_tile_step(&mut self, edit: TileEdit, same_frame: bool) {
        if same_frame {
            self.damage.add_tile_edit(edit);
        } else {
            self.damage = Damage::All;
        }
    }
}
