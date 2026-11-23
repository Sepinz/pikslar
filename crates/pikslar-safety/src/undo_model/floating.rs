//! Floating pixels in the random runs: lifted or pasted, flipped, turned, moved and scaled,
//! their own undo steps walked, then applied as one step or cancelled.

use super::{bad, Model, SIDE};
use crate::go;
use pikslar_core::{rgba, Clip, Editor, FloatingCmd, FloatingSnapshot, Mask, SelectOp};
use pikslar_core::{SelectionCmd, StepCmd};

impl Model {
    /// Lifts or pastes, works on the floating pixels, walks their own undo steps, then applies
    /// or cancels. Applying is one step of the document's history.
    pub(super) fn floating(&mut self) -> Result<(), String> {
        self.edit("selecting", Self::select_something)?;
        let start = self.editor.doc().clone();
        if !self.start_floating() {
            return Ok(());
        }
        self.counts.floating += 1;
        // Freshly lifted pixels are undone all at once, so that state is not a stop of its own.
        let skip = usize::from(!self.editor.is_pasting());
        let mut states = vec![floating_state(&self.editor)];
        for _ in 0..1 + self.rng.below(6) {
            if self.change_floating() {
                states.push(floating_state(&self.editor));
            }
        }
        if states.len() > skip {
            self.walk_floating(&mut states[skip..])?;
        }
        if self.rng.chance(3) {
            go(&mut self.editor, FloatingCmd::Revert);
            self.compare_with(&start, "cancelling floating pixels")?;
            self.compare_picture("cancelling floating pixels")?;
            return self.edit("clearing the selection", |m| {
                go(&mut m.editor, SelectionCmd::None)
            });
        }
        self.edit("applying floating pixels", |m| {
            go(&mut m.editor, FloatingCmd::Anchor)
        })?;
        self.edit("clearing the selection", |m| {
            go(&mut m.editor, SelectionCmd::None)
        })
    }

    pub(super) fn select_something(&mut self) {
        let (x, y) = self.point();
        let shape = Mask::rect(
            x,
            y,
            1 + self.rng.below(16) as i32,
            1 + self.rng.below(16) as i32,
        );
        go(
            &mut self.editor,
            SelectionCmd::Select {
                shape: Box::new(shape),
                op: SelectOp::Replace,
            },
        );
    }

    fn start_floating(&mut self) -> bool {
        let (x, y) = self.point();
        if self.rng.chance(3) {
            let color = rgba(9, self.rng.next_u64() as u8, 200, 255);
            let (w, h) = (1 + self.rng.below(8), 1 + self.rng.below(8));
            let clip = Clip::from_image(w, h, vec![color; w * h]);
            self.editor
                .apply(FloatingCmd::Paste {
                    clip: std::sync::Arc::new(clip.clone()),
                    x,
                    y,
                })
                .is_ok()
        } else {
            let copy = self.rng.chance(2);
            let (layer, frame) = (self.editor.layer, self.editor.frame);
            let shared = self.editor.shared_refusal(layer, frame);
            let refusal = self.editor.paint_refusal(layer).or(shared);
            match self.editor.apply(FloatingCmd::Lift { copy }) {
                Err(e) if Some(e) == refusal => false,
                result => result.expect("lift").changed,
            }
        }
    }

    /// Changes the floating pixels as a gesture would. Returns true if that made a step.
    fn change_floating(&mut self) -> bool {
        let before = floating_state(&self.editor);
        let choice = self.rng.below(5);
        // Flips and turns are a step each. Moves and scales are one for the gesture.
        if choice >= 3 {
            go(&mut self.editor, FloatingCmd::GestureBegin);
        }
        match choice {
            0 => go(&mut self.editor, FloatingCmd::FlipHorizontal),
            1 => go(&mut self.editor, FloatingCmd::FlipVertical),
            2 => go(
                &mut self.editor,
                FloatingCmd::Rotate {
                    clockwise: self.rng.chance(2),
                },
            ),
            3 => {
                let (x, y) = (self.rng.between(0, 20), self.rng.between(0, 20));
                let (w, h) = (1 + self.rng.below(24) as i32, 1 + self.rng.below(24) as i32);
                go(&mut self.editor, FloatingCmd::Scale { x, y, w, h });
            }
            _ => {
                let (dx, dy) = (self.rng.between(-5, 5), self.rng.between(-5, 5));
                go(&mut self.editor, FloatingCmd::Move { dx, dy, join: true });
            }
        }
        self.editor.settle_floating();
        let changed = floating_state(&self.editor) != before;
        if choice >= 3 {
            go(&mut self.editor, FloatingCmd::GestureEnd);
        }
        changed || choice < 3
    }

    /// Undoes partway through the floating steps and redoes back, comparing each stop.
    fn walk_floating(&mut self, states: &mut [FloatingState]) -> Result<(), String> {
        let back = self.rng.below(states.len());
        for offset in 1..=back {
            self.editor.apply(StepCmd::Undo).expect("undo");
            self.editor.settle_floating();
            let expected = &states[states.len() - 1 - offset];
            if floating_state(&self.editor) != *expected {
                return bad(format!("floating undo {offset} of {back} landed elsewhere"));
            }
        }
        for _ in 0..back {
            self.editor.apply(StepCmd::Redo).expect("redo");
            self.editor.settle_floating();
        }
        match states.last() {
            Some(last) if floating_state(&self.editor) != *last => {
                bad(format!("redoing {back} floating steps landed elsewhere"))
            }
            _ => Ok(()),
        }
    }
}

type FloatingState = (
    Option<FloatingSnapshot>,
    Option<(i32, i32, i32, i32)>,
    Vec<u32>,
);

fn floating_state(editor: &Editor) -> FloatingState {
    let without_table = editor.floating_snapshot().map(|s| FloatingSnapshot {
        colors_before: None,
        ..s
    });
    let shown = (-4..SIDE as i32 + 4)
        .flat_map(|y| (-4..SIDE as i32 + 4).map(move |x| (x, y)))
        .map(|(x, y)| editor.color_at(x, y))
        .collect();
    (without_table, editor.selection().map(|m| m.bounds()), shown)
}
