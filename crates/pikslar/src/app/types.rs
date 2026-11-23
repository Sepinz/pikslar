//! The small types the application state is made of: what the pointer is doing, which tool
//! the left button has, and the new-sprite panel's choices.

use crate::select_ui::Handle;
use pikslar_core::{ColorMode, SelectOp, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    None,
    Stroke,
    Pan {
        x: i32,
        y: i32,
    },
    Marquee {
        start: (i32, i32),
        op: SelectOp,
    },
    Lasso {
        op: SelectOp,
    },
    /// Pressed inside the selection, and not dragged yet.
    Grab {
        x: i32,
        y: i32,
        copy: bool,
        /// Nothing was selected: the press picked up the layer, and only a drag keeps that.
        auto: bool,
    },
    Move {
        x: i32,
        y: i32,
        /// This drag is what lifted the pixels, so cancelling it can put them back.
        lifted: bool,
    },
    Scale {
        handle: Handle,
        bounds: (i32, i32, i32, i32),
        lifted: bool,
    },
    /// The Move tool dragging the whole active cel from where the press found it.
    Cel {
        x: i32,
        y: i32,
        from: (i32, i32),
        /// Alt was held: only this cel moves, not the rest of its hold.
        alone: bool,
    },
}

/// What the left mouse button does on the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolChoice {
    Paint(Tool),
    Marquee,
    Lasso,
    Wand,
    Move,
    Text,
}

/// The new-sprite panel's choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewDialog {
    pub size: usize,
    pub mode: ColorMode,
}
