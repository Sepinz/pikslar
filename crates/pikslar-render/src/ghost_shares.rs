//! Ghosts that come or go whole after an edit.
//!
//! A ghost leaves out a layer whose cel in the ghost's frame looks the same as in the frame
//! shown (`Compositor::blend_ghost`), as a hold does. When two frames show one drawing without
//! a hold, a stroke on the shown frame gives it a drawing of its own. That layer's whole ghost
//! then appears, beneath translucent and clear pixels everywhere, though the stroke damages
//! only what it touched. Undoing the stroke shares the drawing again, and the whole ghost goes.
//!
//! So the compositor remembers, for each ghost and layer, whether the ghost left the layer out.
//! When that changes and nothing else moved, it composites every canvas tile the ghost
//! frame's cel draws on. That cel did not change, so its area covers every pixel where the
//! ghost appeared or went. A frame change, onion skin turned on or a layer added or removed
//! damages the whole canvas anyway, so those only start the record afresh.

use super::Compositor;
use pikslar_core::{Document, Ghost, TILE};

/// What the last update's ghosts left out.
#[derive(Default)]
pub(super) struct GhostShares {
    /// The frame shown, the layer count and each ghost's frame: the record holds for these.
    key: Vec<usize>,
    /// For each ghost in turn, one entry per layer: true where the ghost left the layer out.
    left_out: Vec<bool>,
}

fn key_of<'a>(doc: &Document, now: usize, ghosts: &'a [Ghost]) -> impl Iterator<Item = usize> + 'a {
    let head = [now, doc.layers().len()];
    head.into_iter().chain(ghosts.iter().map(|g| g.frame))
}

/// Calls `f` with each ghost's frame, each layer and whether the ghost leaves it out.
fn each_left_out(
    doc: &Document,
    drawn: &[bool],
    (now, ghosts): (usize, &[Ghost]),
    mut f: impl FnMut(usize, usize, bool),
) {
    for ghost in ghosts {
        for (index, layer) in doc.layers().iter().enumerate() {
            let shown = drawn.get(index).copied().unwrap_or(false);
            let left = shown && layer.cel(ghost.frame).looks_the_same(layer.cel(now));
            f(ghost.frame, index, left);
        }
    }
}

impl Compositor {
    /// Composites the tiles of every ghost that came or went whole since the last update,
    /// skipping tiles this update already composited, and appends those it composited to
    /// `changed`. `redrawn_all` says this update composited every tile.
    pub(super) fn redraw_changed_ghosts(
        &mut self,
        scene: (&Document, &[bool]),
        (now, ghosts): (usize, &[Ghost]),
        redrawn_all: bool,
        changed: &mut Vec<crate::view::Rect>,
    ) {
        let (doc, drawn) = scene;
        let same = !redrawn_all && key_of(doc, now, ghosts).eq(self.shares.key.iter().copied());
        let mut areas = Vec::new();
        let mut at = 0;
        let mut left_out = std::mem::take(&mut self.shares.left_out);
        left_out.resize(ghosts.len() * doc.layers().len(), false);
        each_left_out(doc, drawn, (now, ghosts), |frame, layer, left| {
            if same && left_out[at] != left {
                areas.extend(doc.cel_area(layer, frame));
            }
            left_out[at] = left;
            at += 1;
        });
        self.shares.key.clear();
        self.shares.key.extend(key_of(doc, now, ghosts));
        self.shares.left_out = left_out;
        for area in areas {
            self.compose_area(area, changed);
        }
    }

    /// Collects the canvas tiles `area` touches that this update has not collected.
    fn compose_area(
        &mut self,
        (x, y, w, h): (i32, i32, i32, i32),
        changed: &mut Vec<crate::view::Rect>,
    ) {
        let (x0, y0) = (x.max(0), y.max(0));
        let (x1, y1) = (
            (x + w).min(self.width as i32),
            (y + h).min(self.height as i32),
        );
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let side = TILE as i32;
        for ty in y0 / side..=(y1 - 1) / side {
            for tx in x0 / side..=(x1 - 1) / side {
                let index = ty as usize * self.tiles_wide + tx as usize;
                if self.seen[index] == self.stamp {
                    continue;
                }
                self.seen[index] = self.stamp;
                if self.compose_or_defer(index) {
                    changed.push(self.tile_rect(index));
                }
            }
        }
    }
}
