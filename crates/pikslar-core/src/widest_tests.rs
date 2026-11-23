//! The widest table any cel has had is remembered for the memory check. Structural steps find
//! it from the cels they put in the document, which must give what a look at every cel gives.

use crate::color::rgba;
use crate::command::{CelCmd, Command, DrawCmd, FrameCmd, LayerCmd, StepCmd};
use crate::document::{ColorMode, Document};
use crate::editor::{Editor, Tool};
use crate::layers::widest;
use crate::Turn;

struct Dice(u64);

impl Dice {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }

    fn offset(&mut self) -> i32 {
        self.below(4001) as i32 - 2000
    }
}

fn widest_now(e: &Editor) -> usize {
    e.doc()
        .layers()
        .iter()
        .map(|l| widest(l.cels()))
        .max()
        .unwrap_or(0)
}

fn structural(e: &Editor, dice: &mut Dice) -> Command {
    let frames = e.doc().frames.len();
    let layers = e.doc().layers().len();
    let (a, b) = (dice.below(frames), dice.below(frames));
    let layer = e.layer_id(dice.below(layers)).expect("layer");
    let frame = dice.below(frames);
    match dice.below(9) {
        0 => FrameCmd::Add {
            at: a,
            copy: dice.below(2) == 0,
        }
        .into(),
        1 => FrameCmd::Remove {
            from: a.min(b),
            to: a.max(b),
        }
        .into(),
        2 => FrameCmd::Move { from: a, to: b }.into(),
        3 => FrameCmd::HoldPrevious {
            layer,
            frame: a.max(1),
        }
        .into(),
        4 => LayerCmd::Add {
            at: dice.below(layers + 1),
            mode: ColorMode::Rgba,
        }
        .into(),
        5 => LayerCmd::Remove { layer }.into(),
        6 => LayerCmd::Move {
            layer,
            to: dice.below(layers),
        }
        .into(),
        7 => CelCmd::Turn {
            layer,
            frame,
            turn: Turn::Clockwise,
        }
        .into(),
        _ => FrameCmd::Release {
            layer,
            from: a.min(b),
            to: a.max(b),
        }
        .into(),
    }
}

/// One random command: drawing, moving a cel far, a structural change, undo or redo.
fn any(e: &mut Editor, dice: &mut Dice) {
    e.layer = dice.below(e.doc().layers().len());
    e.frame = dice.below(e.doc().frames.len());
    let layer = e.layer_id(e.layer).expect("layer");
    let (x, y) = (dice.below(64) as i32, dice.below(64) as i32);
    let command: Command = match dice.below(10) {
        0 | 1 => DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(x, y), (x + 3, y)],
        }
        .into(),
        2 => CelCmd::Move {
            layer,
            frame: e.frame,
            dx: dice.offset(),
            dy: dice.offset(),
        }
        .into(),
        3 => StepCmd::Undo.into(),
        4 => StepCmd::Redo.into(),
        _ => structural(e, dice),
    };
    e.color = rgba(x as u8 + 1, y as u8, 7, 255);
    let _ = e.apply(command);
    e.apply(StepCmd::EndDrag).expect("end");
}

#[test]
fn the_widest_table_found_from_each_step_is_the_widest_any_cel_has_had() {
    for seed in 1..=60u64 {
        let mut dice = Dice(seed * 0x9E37_79B9);
        let mut e = Editor::new(Document::new(64, 64, ColorMode::Rgba).expect("valid"));
        let mut seen = widest_now(&e);
        for turn in 0..120 {
            any(&mut e, &mut dice);
            seen = seen.max(widest_now(&e));
            assert_eq!(e.widest_table, seen, "seed {seed} turn {turn}");
        }
    }
}
