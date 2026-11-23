//! Position tweens: runs of a hold given offsets between their keys, as one step or nothing.
//!
//! Until one cel of a hold can be moved alone, no command leaves a hold's frames apart, so a
//! tween would only ever line up frames already in line. Half the runs therefore start from a
//! document that holds one drawing across five frames with the last one moved, as a file can.

use super::Model;
use crate::go;
use pikslar_core::{rgba, Cel, CelCmd, ColorMode, Document, DrawCmd, Ease, Editor, Tool};

/// Frames in the held run the starting document has.
const HELD: usize = 5;

/// The document a run starts from: plain for even seeds, with a hold apart for odd ones.
pub(super) fn starting_document(
    side: usize,
    mode: ColorMode,
    seed: u64,
) -> Result<Document, String> {
    let doc = Document::new(side, side, mode).map_err(|e| e.to_string())?;
    if seed % 2 == 0 {
        return Ok(doc);
    }
    let mut e = Editor::new(doc);
    e.color = rgba(40, 160, 90, 255);
    let tool = Tool::Rect;
    go(&mut e, DrawCmd::Begin { tool, x: 3, y: 4 });
    go(&mut e, DrawCmd::To { x: 9, y: 12 });
    go(&mut e, DrawCmd::Commit);
    let mut doc = e.doc().clone();
    let fail = |e: pikslar_core::LimitError| e.to_string();
    for at in 1..HELD {
        doc.add_frame(at, true).map_err(fail)?;
    }
    doc.set_holds(0, &[1; HELD]).map_err(fail)?;
    let apart = (seed % 23) as i32 - 11;
    let last: Option<Cel> = doc.layers()[0].cel(HELD - 1).moved_to(apart, 7 - apart);
    doc.put_cel(0, HELD - 1, last.ok_or("past the range")?)
        .map_err(fail)?;
    Ok(doc)
}

impl Model {
    /// Tweens a run of the current layer: mostly the stretch of the current frame's hold around
    /// it, now and then any run at all, which may be refused.
    pub(super) fn tween(&mut self) {
        let Some(layer) = self.editor.layer_id(self.editor.layer) else {
            return;
        };
        let (from, to) = match self.rng.below(4) {
            0 => {
                let frames = self.editor.doc().frames.len();
                let from = self.rng.below(frames);
                (from, from + self.rng.below(frames - from))
            }
            _ => self.held_stretch(),
        };
        let ease = Ease::ALL[self.rng.below(Ease::ALL.len())];
        let tweened = self.editor.apply(CelCmd::Tween {
            layer,
            from,
            to,
            ease,
        });
        if tweened.is_ok_and(|done| done.changed) {
            self.counts.tweens += 1;
        }
    }

    /// The frames on either side of the current one that are in its hold, without a gap.
    fn held_stretch(&self) -> (usize, usize) {
        let (index, frame) = (self.editor.layer, self.editor.frame);
        let layer = &self.editor.doc().layers()[index];
        let id = layer.link(frame);
        if id == 0 {
            return (frame, frame);
        }
        let mut from = frame;
        while from > 0 && layer.link(from - 1) == id {
            from -= 1;
        }
        let mut to = frame;
        while layer.link(to + 1) == id {
            to += 1;
        }
        (from, to)
    }
}
