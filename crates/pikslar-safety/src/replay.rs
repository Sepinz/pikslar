//! Replays fixed sequences of edits and records the state after each one.
//!
//! The record was made on the code before the command system existed, so comparing against it
//! shows that moving an operation changed nothing. A change that is meant to differ updates the
//! record in its own commit and says why.

use crate::deep_use::RATE_CHOICES;
use crate::go;
use crate::mutate::Rng;
use pikslar_core::{
    rgba, BlendMode, Clip, ColorMode, Command, Document, Editor, FrameCmd, LayerCmd, LimitError,
    Mask, PaletteCmd, SelectOp, Slice, SliceCmd, SliceRect, StepCmd, TagCmd, Tool,
};
use pikslar_core::{DrawCmd, FloatingCmd, SelectionCmd};
use std::fmt::Write;

const SIDE: usize = 24;
pub const SEQUENCES: u64 = 30;
pub const STEPS: usize = 60;

fn mix(hash: &mut u64, value: u64) {
    for byte in value.to_le_bytes() {
        *hash = (*hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn mix_text(hash: &mut u64, text: &str) {
    mix(hash, text.len() as u64);
    text.bytes().for_each(|b| mix(hash, b as u64));
}

fn hash_slice(hash: &mut u64, slice: &Slice) {
    mix_text(hash, &slice.name);
    mix(hash, u64::from(slice.color));
    let b = slice.bounds;
    [b.x, b.y, b.width as i32, b.height as i32]
        .iter()
        .for_each(|&v| mix(hash, v as u64));
    let (px, py) = slice.pivot.unwrap_or((i32::MIN, i32::MIN));
    [px, py].iter().for_each(|&v| mix(hash, v as u64));
    let c = slice.center.unwrap_or(SliceRect {
        x: -1,
        y: -1,
        width: 0,
        height: 0,
    });
    [c.x, c.y, c.width as i32, c.height as i32]
        .iter()
        .for_each(|&v| mix(hash, v as u64));
}

fn hash_document(hash: &mut u64, doc: &Document) {
    doc.colors
        .colors()
        .iter()
        .for_each(|&c| mix(hash, c as u64));
    doc.colors
        .swatches()
        .iter()
        .for_each(|&s| mix(hash, s as u64));
    doc.frames
        .iter()
        .for_each(|f| mix(hash, f.duration_ms as u64));
    for tag in &doc.tags {
        mix_text(hash, &tag.name);
        mix(hash, tag.from as u64);
        mix(hash, tag.to as u64);
        mix(hash, tag.direction as u64);
    }
    for slice in &doc.slices {
        hash_slice(hash, slice);
    }
    for layer in doc.layers() {
        mix_text(hash, &layer.name);
        mix(hash, layer.visible as u64);
        mix(hash, layer.opacity as u64);
        mix(hash, layer.blend as u64);
        mix(hash, layer.mode as u64);
        layer.links().for_each(|l| mix(hash, l as u64));
        for cel in layer.cels() {
            cel.to_cells(doc.width, doc.height)
                .iter()
                .for_each(|&c| mix(hash, c as u64));
        }
    }
}

fn hash_selection(hash: &mut u64, editor: &Editor) {
    match editor.selection() {
        None => mix(hash, u64::MAX),
        Some(mask) => {
            let (x, y, w, h) = mask.bounds();
            [x, y, w, h].iter().for_each(|&v| mix(hash, v as u64));
            mask.for_each(|px, py| mix(hash, ((px as u64) << 32) | py as u64 & 0xffff_ffff));
        }
    }
}

/// Everything a replay compares after a step.
pub(crate) fn describe(editor: &mut Editor) -> String {
    editor.settle_floating();
    let mut hash = 0xcbf2_9ce4_8422_2325;
    hash_document(&mut hash, editor.doc());
    hash_selection(&mut hash, editor);
    for y in -2..SIDE as i32 + 2 {
        for x in -2..SIDE as i32 + 2 {
            mix(&mut hash, editor.color_at(x, y) as u64);
        }
    }
    let mut line = String::new();
    let _ = write!(
        line,
        "{hash:016x} {} {} {} {} {}",
        editor.history().undo_depth(),
        editor.history().redo_depth(),
        editor.layer,
        editor.frame,
        editor.is_floating() as u8,
    );
    line
}

/// Runs a command but stays where the record stayed, since the interface sets the place itself.
fn stay(e: &mut Editor, command: impl Into<Command>) -> Result<(), LimitError> {
    let (layer, frame) = (e.layer, e.frame);
    e.apply(command)?;
    e.layer = layer.min(e.doc().layers().len() - 1);
    e.frame = frame.min(e.doc().frames.len() - 1);
    Ok(())
}

struct Player {
    editor: Editor,
    rng: Rng,
}

impl Player {
    fn point(&mut self) -> (i32, i32) {
        (
            self.rng.between(-2, SIDE as i32 + 1),
            self.rng.between(-2, SIDE as i32 + 1),
        )
    }

    fn frames(&self) -> usize {
        self.editor.doc().frames.len()
    }

    fn layers(&self) -> usize {
        self.editor.doc().layers().len()
    }

    fn range(&mut self) -> (usize, usize) {
        let (a, b) = (self.rng.below(self.frames()), self.rng.below(self.frames()));
        (a.min(b), a.max(b))
    }

    fn focus(&mut self) {
        self.editor.layer = self.rng.below(self.layers());
        self.editor.frame = self.rng.below(self.frames());
        self.editor.color = rgba(
            self.rng.next_u64() as u8 | 1,
            self.rng.next_u64() as u8,
            self.rng.next_u64() as u8,
            255,
        );
    }

    fn stroke(&mut self, tool: Tool) {
        let (a, b, c) = (self.point(), self.point(), self.point());
        if self
            .editor
            .apply(DrawCmd::Begin {
                tool,
                x: a.0,
                y: a.1,
            })
            .is_ok()
            && self.editor.is_stroking()
        {
            go(&mut self.editor, DrawCmd::To { x: b.0, y: b.1 });
            go(&mut self.editor, DrawCmd::To { x: c.0, y: c.1 });
            go(&mut self.editor, DrawCmd::Commit);
        }
    }

    fn cancelled_stroke(&mut self) {
        let (a, b) = (self.point(), self.point());
        if self
            .editor
            .apply(DrawCmd::Begin {
                tool: Tool::Pencil,
                x: a.0,
                y: a.1,
            })
            .is_ok()
        {
            go(&mut self.editor, DrawCmd::To { x: b.0, y: b.1 });
            go(&mut self.editor, DrawCmd::Cancel);
        }
    }

    fn structure(&mut self) {
        let (layer, frame) = (self.editor.layer, self.editor.frame);
        let (from, to) = self.range();
        let e = &mut self.editor;
        let Some(id) = e.layer_id(layer) else {
            return;
        };
        let choice = self.rng.below(16);
        let _ = match choice {
            0 => stay(
                e,
                FrameCmd::Add {
                    at: frame + 1,
                    copy: true,
                },
            ),
            1 => stay(
                e,
                FrameCmd::Add {
                    at: frame + 1,
                    copy: false,
                },
            ),
            2 => stay(
                e,
                FrameCmd::Remove {
                    from: frame,
                    to: frame,
                },
            ),
            3 => stay(e, FrameCmd::Remove { from, to }),
            4 => stay(e, FrameCmd::Move { from, to }),
            5 => stay(
                e,
                LayerCmd::Add {
                    at: layer + 1,
                    mode: ColorMode::Rgba,
                },
            ),
            6 => stay(
                e,
                LayerCmd::Add {
                    at: layer + 1,
                    mode: ColorMode::Palette,
                },
            ),
            7 => stay(e, LayerCmd::Remove { layer: id }),
            8 => {
                let to = (layer + 1).min(e.doc().layers().len() - 1);
                stay(e, LayerCmd::Move { layer: id, to })
            }
            9 => stay(e, FrameCmd::HoldPrevious { layer: id, frame }),
            10 => stay(
                e,
                FrameCmd::HoldAcross {
                    layer: id,
                    source: from,
                    from,
                    to,
                },
            ),
            11 => stay(
                e,
                FrameCmd::Release {
                    layer: id,
                    from: frame,
                    to: frame,
                },
            ),
            12 => stay(
                e,
                TagCmd::Add {
                    name: "tag".to_owned(),
                    from,
                    to,
                },
            ),
            13 => stay(
                e,
                LayerCmd::SetBlend {
                    layer: id,
                    blend: BlendMode::Multiply,
                },
            ),
            14 => {
                let mode = match e.doc().layers()[layer].mode {
                    ColorMode::Rgba => ColorMode::Palette,
                    ColorMode::Palette => ColorMode::Rgba,
                };
                stay(e, LayerCmd::SetMode { layer: id, mode })
            }
            _ => match e.doc().tags.is_empty() {
                true => {
                    let visible = e.doc().layers()[layer].visible;
                    stay(e, LayerCmd::SetVisible { layer: id, visible })
                }
                false => stay(e, TagCmd::CycleDirection { tag: 0 }),
            },
        };
    }

    /// Adds a slice, sets the first one whole or removes it: the cases the app makes.
    fn slice_edit(&mut self) {
        let (x, y) = self.point();
        let (width, height) = (1 + self.rng.below(12) as u32, 1 + self.rng.below(12) as u32);
        let slice = Box::new(Slice {
            name: format!("Slice {}", self.rng.below(10)),
            color: self.rng.next_u64() as u32,
            bounds: SliceRect {
                x,
                y,
                width,
                height,
            },
            pivot: self.rng.chance(2).then_some((0, 0)),
            center: None,
        });
        let _ = match self.rng.below(4) {
            0 => self.editor.apply(SliceCmd::Remove { index: 0 }),
            1 => self.editor.apply(SliceCmd::Set { index: 0, slice }),
            _ => self.editor.apply(SliceCmd::Add { slice }),
        };
    }

    fn values(&mut self) {
        let (layer, frame) = (self.editor.layer, self.editor.frame);
        let color = self.editor.color;
        match self.rng.below(10) {
            9 => self.slice_edit(),
            8 => {
                let fps = RATE_CHOICES[self.rng.below(RATE_CHOICES.len())];
                let _ = self.editor.apply(FrameCmd::SetRate { fps });
            }
            0 | 1 => {
                let opacity = self.rng.next_u64() as u8;
                if let Some(id) = self.editor.layer_id(layer) {
                    let _ = self
                        .editor
                        .apply(LayerCmd::SetOpacity { layer: id, opacity });
                }
            }
            2 => {
                let _ = self.editor.apply(StepCmd::EndDrag);
            }
            3 => {
                let durations = vec![10 + self.rng.below(900) as u32];
                let _ = self.editor.apply(FrameCmd::Retime {
                    from: frame,
                    durations,
                });
            }
            4 => {
                let _ = self.editor.apply(PaletteCmd::AddSwatch { color });
            }
            5 => {
                let id = 1 + self.rng.below(self.editor.doc().colors.len());
                let _ = self.rng.chance(2);
                let _ = self.editor.apply(StepCmd::EndDrag);
                let _ = self.editor.apply(PaletteCmd::Recolor {
                    id: id as u32,
                    color,
                });
            }
            6 => {
                let swatches = self.editor.doc().colors.swatches().to_vec();
                if let Some(&id) = swatches.first() {
                    let _ = self.editor.apply(PaletteCmd::RemoveSwatch { id });
                }
            }
            _ => {
                let shown = self.editor.doc().layers()[layer].visible;
                if let Some(id) = self.editor.layer_id(layer) {
                    let visible = !shown;
                    let _ = self
                        .editor
                        .apply(LayerCmd::SetVisible { layer: id, visible });
                }
            }
        }
    }

    fn selection(&mut self) {
        let (x, y) = self.point();
        let (w, h) = (1 + self.rng.below(12) as i32, 1 + self.rng.below(12) as i32);
        let shape = Mask::rect(x, y, w, h);
        match self.rng.below(6) {
            0 | 1 => go(
                &mut self.editor,
                SelectionCmd::Select {
                    shape: Box::new(shape),
                    op: SelectOp::Replace,
                },
            ),
            2 => go(
                &mut self.editor,
                SelectionCmd::Select {
                    shape: Box::new(shape),
                    op: SelectOp::Add,
                },
            ),
            3 => go(
                &mut self.editor,
                SelectionCmd::Select {
                    shape: Box::new(shape),
                    op: SelectOp::Subtract,
                },
            ),
            4 => go(&mut self.editor, SelectionCmd::Invert),
            _ => go(&mut self.editor, SelectionCmd::None),
        }
    }

    /// A drag of the floating pixels, marked as the canvas marks it.
    fn gesture(&mut self) {
        if !self
            .editor
            .apply(FloatingCmd::Lift {
                copy: self.rng.chance(4),
            })
            .expect("lift")
            .changed
        {
            return;
        }
        go(&mut self.editor, FloatingCmd::GestureBegin);
        if self.rng.chance(2) {
            for _ in 0..1 + self.rng.below(3) {
                let (dx, dy) = (self.rng.between(-4, 4), self.rng.between(-4, 4));
                go(&mut self.editor, FloatingCmd::Move { dx, dy, join: true });
            }
        } else {
            let (x, y) = (self.rng.between(0, 16), self.rng.between(0, 16));
            let (w, h) = (1 + self.rng.below(20) as i32, 1 + self.rng.below(20) as i32);
            go(&mut self.editor, FloatingCmd::Scale { x, y, w, h });
        }
        go(&mut self.editor, FloatingCmd::GestureEnd);
    }

    fn floating(&mut self) {
        match self.rng.below(9) {
            0..=2 => self.gesture(),
            3 => go(&mut self.editor, FloatingCmd::FlipHorizontal),
            4 => go(&mut self.editor, FloatingCmd::FlipVertical),
            5 => go(
                &mut self.editor,
                FloatingCmd::Rotate {
                    clockwise: self.rng.chance(2),
                },
            ),
            6 => go(&mut self.editor, FloatingCmd::Anchor),
            7 => go(&mut self.editor, FloatingCmd::Revert),
            _ => {
                let (x, y) = self.point();
                let (w, h) = (1 + self.rng.below(6), 1 + self.rng.below(6));
                let color = rgba(9, self.rng.next_u64() as u8, 200, 255);
                go(
                    &mut self.editor,
                    FloatingCmd::Paste {
                        clip: std::sync::Arc::new(Clip::from_image(w, h, vec![color; w * h])),
                        x,
                        y,
                    },
                );
            }
        }
    }

    fn pixels(&mut self) {
        let (x, y) = self.point();
        match self.rng.below(2) {
            0 => go(&mut self.editor, DrawCmd::DeleteSelected),
            _ => go(&mut self.editor, DrawCmd::EraseCell { x, y }),
        }
    }

    fn step(&mut self) {
        if self.rng.chance(2) && !self.editor.is_floating() {
            self.focus();
        }
        match self.rng.below(16) {
            0 => self.stroke(Tool::Pencil),
            1 => self.stroke(Tool::Eraser),
            2 => self.stroke(Tool::Line),
            3 => self.stroke(Tool::Rect),
            4 => self.stroke(Tool::Fill),
            5 => self.cancelled_stroke(),
            6 | 7 => self.structure(),
            8 => self.values(),
            9 => self.selection(),
            10 | 11 => self.floating(),
            12 => self.pixels(),
            13 | 14 => {
                self.editor.apply(StepCmd::Undo).expect("undo");
            }
            _ => {
                self.editor.apply(StepCmd::Redo).expect("redo");
            }
        }
    }
}

/// The record of one sequence, one line for each step.
pub fn play(seed: u64) -> Result<Vec<String>, String> {
    let mode = if seed % 2 == 0 {
        ColorMode::Palette
    } else {
        ColorMode::Rgba
    };
    let doc = Document::new(SIDE, SIDE, mode).map_err(|e| e.to_string())?;
    let mut player = Player {
        editor: Editor::new(doc),
        rng: Rng::new(seed.wrapping_mul(7919) + 1),
    };
    let mut lines = Vec::with_capacity(STEPS);
    for _ in 0..STEPS {
        player.step();
        lines.push(describe(&mut player.editor));
    }
    Ok(lines)
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
