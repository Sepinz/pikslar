//! Custom brushes in the random runs: capturing, turning and choosing a brush (view state, never a
//! step, never a change to the file), and strokes with a brush in either colour mode, each of
//! which is one step or nothing and undoes and redoes to the very document it made.

use super::{bad, Model};
use crate::go;
use pikslar_core::custom_brush::{BrushChoice, BrushPaint, Handle, Preset};
use pikslar_core::{DrawCmd, LayerKind, LimitError, Tool, Turn};

const TURNS: [Turn; 4] = [
    Turn::FlipHorizontal,
    Turn::FlipVertical,
    Turn::Clockwise,
    Turn::CounterClockwise,
];
const TOOLS: [Tool; 4] = [Tool::Pencil, Tool::Eraser, Tool::Line, Tool::Rect];

impl Model {
    /// One brush action, chosen at random: mostly strokes, some captures and view changes.
    pub(super) fn brush_step(&mut self) -> Result<(), String> {
        self.mostly_off_maps();
        match self.rng.below(8) {
            0 | 1 => self.capture(),
            2 | 3 => self.change_brush(),
            _ => self.brush_stroke(),
        }
    }

    /// Brushes paint pixels, so a map layer refuses them. Three times in four a brush step on
    /// one moves to the first layer that is not a map, so the runs keep painting with brushes.
    fn mostly_off_maps(&mut self) {
        let layers = self.editor.doc().layers();
        let on_map = |i: usize| matches!(layers[i].kind, LayerKind::Tilemap(_));
        if !on_map(self.editor.layer) || self.rng.chance(4) {
            return;
        }
        if let Some(paint) = (0..layers.len()).find(|&i| !on_map(i)) {
            self.editor.layer = paint;
        }
    }

    /// Runs a view-state change and holds it to what view state is: no step, no change of the
    /// document or of the file's revision.
    fn view_only(&mut self, name: &str, change: impl FnOnce(&mut Self)) -> Result<(), String> {
        let (pushes, revision) = (self.editor.history().pushes(), self.editor.revision());
        change(self);
        if self.editor.history().pushes() != pushes {
            return bad(format!("{name} became an undo step"));
        }
        if self.editor.revision() != revision {
            return bad(format!("{name} marked the file changed"));
        }
        self.compare(name)
    }

    /// Selects a patch and makes a brush of it, in the paint mode chosen at random.
    fn capture(&mut self) -> Result<(), String> {
        self.edit("selecting", Self::select_something)?;
        let paint = [BrushPaint::OwnColors, BrushPaint::Shape][self.rng.below(2)];
        let mut result = Ok(());
        self.view_only("capturing a brush", |m| {
            result = m.editor.capture_brush(paint)
        })?;
        match result {
            Ok(()) => self.counts.brushes += 1,
            Err(LimitError::NothingToCapture) => {}
            Err(other) => return bad(format!("capturing a brush gave {other:?}")),
        }
        Ok(())
    }

    fn change_brush(&mut self) -> Result<(), String> {
        match self.rng.below(6) {
            0 | 1 => self.turn_brush(),
            2 => self.view_only("changing a brush's paint", |m| {
                let paint = [BrushPaint::OwnColors, BrushPaint::Shape][m.rng.below(2)];
                m.editor.set_brush_paint(paint);
            }),
            3 => self.view_only("moving a brush's handle", |m| {
                let corner = m.rng.below(5);
                let handle = match corner {
                    0 => Handle::Centre,
                    n => Handle::Corner(n as u8 - 1),
                };
                m.editor.set_brush_handle(handle);
            }),
            4 => self.view_only("choosing a preset", |m| {
                m.editor.brush_size = m.rng.between(1, 7);
                let preset = Preset::ALL[m.rng.below(Preset::ALL.len())];
                m.editor.set_brush(BrushChoice::Preset(preset));
            }),
            _ => self.view_only("choosing the round brush", |m| {
                m.editor.set_brush(BrushChoice::Round)
            }),
        }
    }

    /// Turns the brush once; now and then four times over, which must give the brush back.
    fn turn_brush(&mut self) -> Result<(), String> {
        let turn = TURNS[self.rng.below(TURNS.len())];
        let around = matches!(turn, Turn::Clockwise | Turn::CounterClockwise);
        let full_circle = around && self.rng.chance(3);
        let before = self.editor.brushes().active_custom().cloned();
        self.view_only("turning a brush", |m| {
            let times = if full_circle { 4 } else { 1 };
            for _ in 0..times {
                m.editor.turn_brush(turn);
            }
        })?;
        let after = self.editor.brushes().active_custom().cloned();
        if before.is_some() {
            self.counts.brush_turns += 1;
        }
        if full_circle && before != after {
            return bad("four quarter turns did not give the brush back".to_owned());
        }
        Ok(())
    }

    /// Makes sure a custom brush is the one chosen, capturing one if need be.
    fn have_custom_brush(&mut self) -> Result<(), String> {
        if self.editor.brushes().active_custom().is_none() {
            self.capture()?;
        }
        Ok(())
    }

    /// A stroke with the chosen brush, finished or cancelled. A finished one is one step or
    /// nothing; a cancelled one is nothing, and gives back any colours it added.
    fn brush_stroke(&mut self) -> Result<(), String> {
        self.have_custom_brush()?;
        let custom = self.editor.brushes().active_custom().is_some();
        let before = self.editor.history().pushes();
        self.edit("brush stroke", Self::paint_with_brush)?;
        let stepped = self.editor.history().pushes() != before;
        self.counts.brush_strokes += u64::from(custom && stepped);
        Ok(())
    }

    fn paint_with_brush(&mut self) {
        let tool = TOOLS[self.rng.below(TOOLS.len())];
        let points: Vec<(i32, i32)> = (0..2 + self.rng.below(3)).map(|_| self.point()).collect();
        let begin = DrawCmd::Begin {
            tool,
            x: points[0].0,
            y: points[0].1,
        };
        if self.editor.apply(begin).is_err() {
            return;
        }
        for &(x, y) in &points[1..] {
            go(&mut self.editor, DrawCmd::To { x, y });
        }
        if self.rng.chance(6) {
            go(&mut self.editor, DrawCmd::Cancel);
        } else {
            go(&mut self.editor, DrawCmd::Commit);
        }
    }
}
