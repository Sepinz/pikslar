//! Replays fixed sequences with brush view events among the strokes, as `replay.rs` does for
//! edits. It has its own record, so the older one is not disturbed: the brush calls are view
//! state and draw nothing, so only the strokes after them show what they chose.

use crate::mutate::Rng;
use crate::replay::describe;
use pikslar_core::custom_brush::{BrushChoice, BrushPaint, Preset};
use pikslar_core::{
    rgba, ColorMode, Document, DrawCmd, Editor, Mask, SelectOp, SelectionCmd, StepCmd, Tool, Turn,
};
use std::fmt::Write;

const SIDE: usize = 24;
pub const SEQUENCES: u64 = 24;
pub const STEPS: usize = 50;

const TOOLS: [Tool; 4] = [Tool::Pencil, Tool::Eraser, Tool::Line, Tool::Rect];
const TURNS: [Turn; 4] = [
    Turn::FlipHorizontal,
    Turn::FlipVertical,
    Turn::Clockwise,
    Turn::CounterClockwise,
];

struct Player {
    editor: Editor,
    rng: Rng,
}

impl Player {
    fn point(&mut self) -> (i32, i32) {
        let span = SIDE as i32 + 3;
        (self.rng.between(-2, span), self.rng.between(-2, span))
    }

    fn stroke(&mut self, finish: DrawCmd) {
        let tool = TOOLS[self.rng.below(TOOLS.len())];
        self.editor.color = rgba(
            self.rng.next_u64() as u8 | 1,
            self.rng.next_u64() as u8,
            self.rng.next_u64() as u8,
            255,
        );
        let (a, b, c) = (self.point(), self.point(), self.point());
        let begin = DrawCmd::Begin {
            tool,
            x: a.0,
            y: a.1,
        };
        if self.editor.apply(begin).is_ok() && self.editor.is_stroking() {
            for p in [b, c] {
                let _ = self.editor.apply(DrawCmd::To { x: p.0, y: p.1 });
            }
            let _ = self.editor.apply(finish);
        }
    }

    /// Selects a patch of what is drawn and makes a brush of it.
    fn capture(&mut self) {
        let (x, y) = (self.rng.between(0, 14), self.rng.between(0, 14));
        let (w, h) = (self.rng.between(1, 9), self.rng.between(1, 9));
        let shape = Box::new(Mask::rect(x, y, w, h));
        let _ = self.editor.apply(SelectionCmd::Select {
            shape,
            op: SelectOp::Replace,
        });
        let paint = if self.rng.chance(2) {
            BrushPaint::OwnColors
        } else {
            BrushPaint::Shape
        };
        let _ = self.editor.capture_brush(paint);
    }

    /// The brush view events: choosing, turning and changing the paint of a brush.
    fn brush_event(&mut self) {
        match self.rng.below(5) {
            0 => self.capture(),
            1 => {
                self.editor.turn_brush(TURNS[self.rng.below(4)]);
            }
            2 => {
                let paint = [BrushPaint::OwnColors, BrushPaint::Shape][self.rng.below(2)];
                self.editor.set_brush_paint(paint);
            }
            3 => {
                self.editor.brush_size = self.rng.between(1, 6);
                let preset = Preset::ALL[self.rng.below(Preset::ALL.len())];
                self.editor.set_brush(BrushChoice::Preset(preset));
            }
            _ => self.editor.set_brush(BrushChoice::Round),
        }
    }

    fn step(&mut self) {
        match self.rng.below(12) {
            0..=3 => self.stroke(DrawCmd::Commit),
            4 => self.stroke(DrawCmd::Cancel),
            5 | 6 => self.brush_event(),
            7 | 8 => self.capture(),
            9 => {
                let _ = self.editor.apply(StepCmd::Undo);
            }
            10 => {
                let _ = self.editor.apply(StepCmd::Redo);
            }
            _ => self.brush_event(),
        }
    }
}

fn player(seed: u64) -> Result<Player, String> {
    let mode = if seed % 2 == 0 {
        ColorMode::Palette
    } else {
        ColorMode::Rgba
    };
    let doc = Document::new(SIDE, SIDE, mode).map_err(|e| e.to_string())?;
    Ok(Player {
        editor: Editor::new(doc),
        rng: Rng::new(seed.wrapping_mul(104_729) + 3),
    })
}

/// The record of one sequence, one line for each step.
pub fn play(seed: u64) -> Result<Vec<String>, String> {
    let mut player = player(seed)?;
    let mut lines = Vec::with_capacity(STEPS);
    for _ in 0..STEPS {
        player.step();
        lines.push(describe(&mut player.editor));
    }
    Ok(lines)
}

/// Plays a sequence, undoes everything, and returns how many cells are still drawn.
pub fn drawn_after_undoing_all(seed: u64) -> Result<usize, String> {
    let mut player = player(seed)?;
    for _ in 0..STEPS {
        player.step();
    }
    while player.editor.history().undo_depth() > 0 {
        player
            .editor
            .apply(StepCmd::Undo)
            .map_err(|e| e.to_string())?;
    }
    let side = SIDE as i32;
    let cells = (0..side).flat_map(|y| (0..side).map(move |x| (x, y)));
    Ok(cells
        .filter(|&(x, y)| player.editor.color_at(x, y) != 0)
        .count())
}

/// Every sequence, as text. Compared with the recorded file.
pub fn play_all() -> Result<String, String> {
    let mut text = String::new();
    for seed in 0..SEQUENCES {
        for (step, line) in play(seed)?.iter().enumerate() {
            let _ = writeln!(text, "{seed} {step} {line}");
        }
    }
    Ok(text)
}
