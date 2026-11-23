//! Interface icons, hand-drawn on three pixel grids and drawn in theme colours.
//!
//! Tools are 16 by 16 ([`ToolIcon`]), actions 12 by 12 ([`ActionIcon`]) and marks 8 by 8
//! ([`MarkIcon`]). Every pixel is one of four tones (see [`Tone`]); [`Tones`] turns them into
//! colours for a control's state, and [`draw_art`] draws an icon at a whole number of pixels a
//! unit, one fill per horizontal run.
//!
//! [`Icon`] is the set the interface used before the three grids; each of its variants draws
//! one of the new icons, so its callers did not change.

mod actions;
mod marks;
mod pack;
mod tools;

#[cfg(test)]
mod tests;

use std::cell::Cell;

use pikslar_render::{Rect, Surface};

use crate::theme;

pub use actions::ActionIcon;
pub use marks::MarkIcon;
pub use pack::{Art, Tone};
pub use tools::ToolIcon;

/// The side, in units of `scale`, of the box [`draw_icon`] centres an icon in.
pub const ICON_SIZE: i32 = 9;

/// The icons the interface asks for by name. Each draws one icon of the three grids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Pencil,
    Eraser,
    Fill,
    Picker,
    Line,
    Rect,
    Eye,
    Plus,
    Minus,
    Up,
    Down,
    Play,
    Pause,
    Copy,
    Onion,
    Prev,
    Next,
    Marquee,
    Lasso,
    Wand,
    Move,
    Lock,
    Merge,
    Group,
    Text,
}

impl Icon {
    pub const ALL: [Icon; 25] = {
        use Icon::*;
        [
            Pencil, Eraser, Fill, Picker, Line, Rect, Eye, Plus, Minus, Up, Down, Play, Pause,
            Copy, Onion, Prev, Next, Marquee, Lasso, Wand, Move, Lock, Merge, Group, Text,
        ]
    };

    /// The art this icon draws.
    pub fn art(self) -> Art {
        match self {
            Icon::Pencil => ToolIcon::Pencil.art(),
            Icon::Eraser => ToolIcon::Eraser.art(),
            Icon::Fill => ToolIcon::Fill.art(),
            Icon::Picker => ToolIcon::Picker.art(),
            Icon::Line => ToolIcon::Line.art(),
            Icon::Rect => ToolIcon::Rectangle.art(),
            Icon::Text => ToolIcon::Text.art(),
            Icon::Marquee => ToolIcon::SelectBox.art(),
            Icon::Lasso => ToolIcon::Lasso.art(),
            Icon::Wand => ToolIcon::Wand.art(),
            Icon::Move => ToolIcon::Move.art(),
            Icon::Plus => ActionIcon::Plus.art(),
            Icon::Minus => ActionIcon::Minus.art(),
            Icon::Up => ActionIcon::MoveUp.art(),
            Icon::Down => ActionIcon::MoveDown.art(),
            Icon::Play => ActionIcon::Play.art(),
            Icon::Pause => ActionIcon::Pause.art(),
            Icon::Copy => ActionIcon::Duplicate.art(),
            Icon::Onion => ActionIcon::OnionSkin.art(),
            Icon::Prev => ActionIcon::PreviousFrame.art(),
            Icon::Next => ActionIcon::NextFrame.art(),
            Icon::Merge => ActionIcon::MergeDown.art(),
            Icon::Group => ActionIcon::NewGroup.art(),
            Icon::Eye => MarkIcon::Eye.art(),
            Icon::Lock => MarkIcon::Lock.art(),
        }
    }
}

/// The state of the control an icon sits on, for its colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// Ink in Text, shade in Muted, signal in Accent.
    Normal,
    /// On an Accent face: every tone in OnAccent.
    Chosen,
    /// Unavailable: every tone in Muted.
    Disabled,
}

/// The colour of each tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tones {
    pub ink: u32,
    pub shade: u32,
    pub signal: u32,
}

impl Tones {
    /// Every tone in `color`.
    pub const fn one(color: u32) -> Tones {
        Tones {
            ink: color,
            shade: color,
            signal: color,
        }
    }

    /// The tones for `look` in the current theme. In high contrast shade and signal draw as
    /// ink, so the icon is one system colour.
    pub fn for_look(look: Look, high_contrast: bool) -> Tones {
        match look {
            Look::Chosen => Tones::one(theme::on_accent()),
            Look::Disabled => Tones::one(theme::disabled()),
            Look::Normal if high_contrast => Tones::one(theme::text()),
            Look::Normal => Tones {
                ink: theme::text(),
                shade: theme::muted(),
                signal: theme::accent(),
            },
        }
    }

    /// The colour of `tone`; `None` for clear.
    pub const fn get(self, tone: Tone) -> Option<u32> {
        match tone {
            Tone::Clear => None,
            Tone::Ink => Some(self.ink),
            Tone::Shade => Some(self.shade),
            Tone::Signal => Some(self.signal),
        }
    }
}

thread_local! {
    static HIGH_CONTRAST: Cell<bool> = const { Cell::new(false) };
}

/// Draws icons from now on for high contrast (one colour) or not, on this thread.
pub fn set_high_contrast(on: bool) {
    HIGH_CONTRAST.with(|c| c.set(on));
}

/// Whether icons draw for high contrast, on this thread.
pub fn high_contrast() -> bool {
    HIGH_CONTRAST.with(Cell::get)
}

/// Draws `art` with its top left at (`x`, `y`), `unit` pixels a pixel of art.
pub fn draw_art(target: &mut Surface<'_>, x: i32, y: i32, art: Art, tones: Tones, unit: i32) {
    let size = art.size();
    for row in 0..size {
        let mut col = 0;
        while col < size {
            let tone = art.tone(col, row);
            let mut end = col + 1;
            while end < size && art.tone(end, row) == tone {
                end += 1;
            }
            if let Some(color) = tones.get(tone) {
                let run = Rect::new(x + col * unit, y + row * unit, (end - col) * unit, unit);
                target.fill(run, color);
            }
            col = end;
        }
    }
}

/// The look [`draw_icon`] reads from the colour its caller passes.
fn look_of(color: u32) -> Look {
    if color == theme::text() {
        Look::Normal
    } else if color == theme::on_accent() {
        Look::Chosen
    } else {
        Look::Disabled
    }
}

/// Draws `icon` centred in the `ICON_SIZE * scale` square at (`x`, `y`), at `scale / 2` pixels a
/// pixel of art (the interface's unit), or twice that for an 8 by 8 mark. `color` is the ink: Text draws the normal tones,
/// OnAccent the chosen ones, and any other colour draws every tone in that colour.
pub fn draw_icon(target: &mut Surface<'_>, x: i32, y: i32, icon: Icon, color: u32, scale: i32) {
    let art = icon.art();
    // Marks are drawn two pixels a unit here: the old icons filled the box, and an 8 by 8 eye
    // or lock at one pixel a unit read too small in a layer row.
    let doubled = if art.size() <= 8 { 2 } else { 1 };
    let unit = ((scale / 2).max(1) * doubled)
        .min(ICON_SIZE * scale / art.size())
        .max(1);
    let inset = (ICON_SIZE * scale - art.size() * unit) / 2;
    let tones = match look_of(color) {
        Look::Disabled => Tones::one(color),
        look => Tones::for_look(look, high_contrast()),
    };
    draw_art(target, x + inset, y + inset, art, tones, unit);
}
