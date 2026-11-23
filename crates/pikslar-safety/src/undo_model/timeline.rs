//! The timeline's drags: blocks of cels moved, copied or linked, runs of frames moved or
//! duplicated, and a tag's ends dragged. Each must be one step, or nothing at all.

use super::{bad, Model};
use crate::deep_use::RATE_CHOICES;
use pikslar_core::{
    ticks, CelCmd, CelDrag, Command, Document, DragKind, FrameCmd, Landing, TagCmd, MAX_DURATION_MS,
};

/// A new frame rate keeps each frame's ticks, so far as a frame may last: the old ticks, or the
/// most the new rate fits in the longest time. Milliseconds keep every time as it was.
pub(super) fn check_rate_change(before: &Document, after: &Document) -> Result<(), String> {
    if after.frame_rate == before.frame_rate {
        return Ok(());
    }
    let Some(fps) = after.frame_rate else {
        let times = |d: &Document| d.frames.iter().map(|f| f.duration_ms).collect::<Vec<_>>();
        return match times(before) == times(after) {
            true => Ok(()),
            false => bad("milliseconds changed the times".to_owned()),
        };
    };
    let old = before.frame_rate.unwrap_or(fps);
    let most = ticks(MAX_DURATION_MS, fps);
    for (index, frame) in before.frames.iter().enumerate() {
        let expected = ticks(frame.duration_ms, old).min(most);
        if after.frame_units(index) != expected {
            return bad(format!(
                "a rate of {fps} turned frame {index}'s {expected} ticks into {}",
                after.frame_units(index)
            ));
        }
    }
    Ok(())
}

/// Cels in a link must really share one picture, or edits would make them differ, and every
/// link keeps the rules of the links.
pub(super) fn check_holds(doc: &Document) -> Result<(), String> {
    if let Err(fault) = doc.check_links() {
        return bad(format!("links: {fault}"));
    }
    for (index, layer) in doc.layers().iter().enumerate() {
        let held = layer.link(0) != 0 && layer.links().all(|id| id == layer.link(0));
        if layer.kind == pikslar_core::LayerKind::Reference && layer.frame_count() > 1 && !held {
            return bad(format!(
                "layer {index}: a reference is not held on every frame"
            ));
        }
    }
    Ok(())
}

impl Model {
    pub(super) fn timeline_drag(&mut self) {
        let frames = self.editor.doc().frames.len();
        let (from, to) = self.run_of(frames);
        let command: Command = match self.rng.below(5) {
            0 => CelCmd::Drag(self.cel_block(frames)).into(),
            4 => self.rate_command(),
            1 => FrameCmd::Duplicate { from, to }.into(),
            2 => {
                // Every place the run can start once it is taken out.
                let at = self.rng.below(frames - (to - from));
                FrameCmd::MoveRun { from, to, at }.into()
            }
            _ => match self.editor.doc().tags.len() {
                0 => TagCmd::Add {
                    name: "edge".into(),
                    from,
                    to,
                }
                .into(),
                tags => TagCmd::SetRange {
                    tag: self.rng.below(tags),
                    from,
                    to,
                }
                .into(),
            },
        };
        // A drag may be refused, as one over the frame limit is; then nothing changes.
        let _ = self.editor.apply(command);
    }

    /// Timing by milliseconds or by one of a few frame rates other than the one in use, as the
    /// app's menu offers: one step, rounding the times.
    fn rate_command(&mut self) -> Command {
        let current = self.editor.doc().frame_rate;
        let others: Vec<Option<u8>> = RATE_CHOICES
            .into_iter()
            .filter(|&fps| fps != current)
            .collect();
        let fps = others[self.rng.below(others.len())];
        FrameCmd::SetRate { fps }.into()
    }

    /// A run of frames, first and last.
    fn run_of(&mut self, frames: usize) -> (usize, usize) {
        let (a, b) = (self.rng.below(frames), self.rng.below(frames));
        (a.min(b), a.max(b))
    }

    /// A block of cels and somewhere for it to land, in or out of the document's bounds.
    fn cel_block(&mut self, frames: usize) -> CelDrag {
        let layers = self.editor.doc().layers().len();
        let (low, high) = (self.rng.below(layers), self.rng.below(layers));
        let (first, last) = self.run_of(frames);
        let kind = match self.rng.below(3) {
            0 => DragKind::Move,
            1 => DragKind::Copy,
            _ => DragKind::Link,
        };
        let landing = match self.rng.below(2) {
            0 => Landing::Overwrite,
            _ => Landing::Insert,
        };
        CelDrag {
            layers: (low.min(high), low.max(high)),
            frames: (first, last),
            to_layer: self.rng.below(layers + 1),
            to_frame: self.rng.below(frames + 1),
            kind,
            landing,
        }
    }
}
