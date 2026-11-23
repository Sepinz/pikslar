//! Random documents for the remap property tests: palette layers with holds, links across
//! layers and copied frames, a layer that is not palette, floating pixels, tilesets of both
//! modes, duplicate colours, unused swatches, groups and session brushes. The same seed always
//! builds the same document, so two builds can differ in one thing only.

use crate::color::rgba;
use crate::command::{
    CelCmd, FloatingCmd, FrameCmd, LayerCmd, PaletteCmd, SelectionCmd, TileCmd, TileSource,
};
use crate::custom_brush::{Brush, BrushChoice, BrushPaint};
use crate::document::{ColorMode, Document};
use crate::editor::{Editor, Tool};
use crate::grid::Grid;
use crate::mask::Mask;
use crate::selection::SelectOp;
use crate::tileset::TilesetId;
use crate::{CelDrag, DragKind, DrawCmd, Landing};
use std::sync::Arc;

pub(super) struct Dice(pub u64);

impl Dice {
    pub fn new(seed: u64) -> Self {
        Dice(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

/// What the build leaves floating over layer 0, frame 0.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Float {
    /// Nothing is lifted.
    No,
    /// Pixels are lifted and moved, then put down.
    Anchored,
    /// Pixels are lifted and moved, and still floating.
    Pending,
}

fn color(dice: &mut Dice) -> u32 {
    let n = dice.below(9) as u8 * 10;
    rgba(n, 250 - n, 90, 255)
}

/// A random document, deterministic in `seed`.
pub(super) fn build(seed: u64, float: Float) -> Editor {
    let mut dice = Dice::new(seed);
    let mut e = Editor::new(Document::new(32, 32, ColorMode::Palette).expect("valid"));
    for _ in 0..4 + dice.below(5) {
        let color = color(&mut dice);
        e.apply(PaletteCmd::AddSwatch { color }).expect("room");
    }
    structure(&mut e, &mut dice);
    for _ in 0..12 + dice.below(20) {
        random_action(&mut e, &mut dice);
    }
    tilesets(&mut e, &mut dice);
    session_brushes(&mut e, &mut dice);
    lift(&mut e, float);
    e
}

fn structure(e: &mut Editor, dice: &mut Dice) {
    for _ in 0..1 + dice.below(2) {
        let at = e.doc().layers().len();
        e.apply(LayerCmd::Add {
            at,
            mode: ColorMode::Palette,
        })
        .expect("room");
    }
    if dice.below(2) == 0 {
        let at = e.doc().layers().len();
        e.apply(LayerCmd::Add {
            at,
            mode: ColorMode::Rgba,
        })
        .expect("room");
    }
    for _ in 0..1 + dice.below(3) {
        let at = dice.below(e.doc().frames.len() + 1);
        let copy = dice.below(2) == 0;
        e.apply(FrameCmd::Add { at, copy }).expect("room");
    }
    // Something drawn where the floating pixels are lifted from, in several colours.
    for (n, row) in (4..10).enumerate() {
        e.layer = 0;
        e.frame = 0;
        e.color = color(dice);
        let path = vec![(3 + n as i32, row), (12 + n as i32, row)];
        e.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path,
        })
        .expect("stroke");
    }
}

fn random_action(e: &mut Editor, dice: &mut Dice) {
    let (layers, frames) = (e.doc().layers().len(), e.doc().frames.len());
    e.layer = dice.below(layers);
    e.frame = dice.below(frames);
    let layer = e.layer_id(e.layer).expect("layer");
    let colors = e.doc().colors.len();
    let swatches = e.doc().colors.swatches().to_vec();
    let pick = |dice: &mut Dice| swatches.get(dice.below(swatches.len().max(1))).copied();
    let (x, y) = (dice.below(26) as i32, dice.below(26) as i32);
    let _ = match dice.below(11) {
        0..=3 => {
            e.color = color(dice);
            let path = vec![(x, y), (x + 4, y + dice.below(4) as i32)];
            e.apply(DrawCmd::Stroke {
                tool: Tool::Pencil,
                path,
            })
        }
        4 => e.apply(FrameCmd::HoldPrevious {
            layer,
            frame: e.frame.max(1).min(frames - 1),
        }),
        5 => e.apply(CelCmd::Drag(CelDrag {
            layers: (dice.below(layers), dice.below(layers)),
            frames: (dice.below(frames), dice.below(frames)),
            to_layer: dice.below(layers),
            to_frame: dice.below(frames),
            kind: DragKind::Link,
            landing: Landing::Overwrite,
        })),
        // A colour recoloured to one the table has: two ids with one value.
        6 | 7 => {
            let id = pick(dice).unwrap_or(1);
            let other = e
                .doc()
                .colors
                .color(1 + dice.below(colors.max(2) - 1) as u32);
            e.apply(PaletteCmd::Recolor { id, color: other })
        }
        8 => match pick(dice) {
            Some(id) => e.apply(PaletteCmd::RemoveSwatch { id }),
            None => return,
        },
        9 => e.apply(PaletteCmd::MoveSwatch {
            from: dice.below(swatches.len().max(1)),
            to: dice.below(swatches.len().max(1)),
        }),
        _ => {
            let ids: Arc<[u32]> = swatches.iter().copied().take(1 + dice.below(3)).collect();
            e.apply(PaletteCmd::AddGroup {
                name: "g".into(),
                ids,
            })
        }
    };
}

/// One tileset of palette tiles and one of RGBA tiles, with shared and distinct images.
fn tilesets(e: &mut Editor, dice: &mut Dice) {
    for mode in [ColorMode::Palette, ColorMode::Rgba] {
        e.apply(TileCmd::NewTileset {
            name: "T".into(),
            mode,
            tile_w: 8,
            tile_h: 8,
        })
        .expect("room");
        let set = e.doc().tilesets.iter().last().expect("added").id();
        for _ in 0..2 + dice.below(4) {
            add_tile(e, dice, set, mode);
        }
    }
}

fn add_tile(e: &mut Editor, dice: &mut Dice, set: TilesetId, mode: ColorMode) {
    let colors = e.doc().colors.len();
    let cells: Vec<u32> = (0..64)
        .map(|_| match mode {
            ColorMode::Palette => dice.below(colors) as u32,
            ColorMode::Rgba => color(dice),
        })
        .collect();
    let image = Arc::new(Grid::from_cells(8, 8, &cells).into_image());
    let outcome = e.apply(TileCmd::AddTile {
        tileset: set,
        from: TileSource::Image(image),
    });
    if dice.below(3) == 0 {
        let tile = crate::tileset::TileId::new(outcome.expect("added").created.expect("id"));
        let tile = tile.expect("valid id");
        e.apply(TileCmd::DuplicateTile { tileset: set, tile })
            .expect("duplicated");
    }
}

/// An active brush and a recent one, taken from ids and colours of the table.
fn session_brushes(e: &mut Editor, dice: &mut Dice) {
    let table = e.doc().colors.clone();
    for _ in 0..2 {
        let ids: Vec<u32> = (0..9)
            .map(|_| dice.below(table.len().min(8)) as u32)
            .collect();
        let colors: Vec<u32> = ids.iter().map(|&id| table.color(id)).collect();
        let made = Brush::from_parts("b", 3, 3, &colors, Some(&ids), BrushPaint::OwnColors);
        if let Ok(brush) = made {
            e.brushes.set_active(BrushChoice::Custom(Arc::new(brush)));
        }
    }
}

fn lift(e: &mut Editor, float: Float) {
    if float == Float::No {
        return;
    }
    e.layer = 0;
    e.frame = 0;
    let shape = Box::new(Mask::rect(2, 3, 14, 8));
    e.apply(SelectionCmd::Select {
        shape,
        op: SelectOp::Replace,
    })
    .expect("selected");
    e.apply(FloatingCmd::Lift { copy: false }).expect("lifted");
    e.apply(FloatingCmd::Move {
        dx: 5,
        dy: 4,
        join: false,
    })
    .expect("moved");
    if float == Float::Anchored {
        e.apply(FloatingCmd::Anchor).expect("anchored");
    }
}
