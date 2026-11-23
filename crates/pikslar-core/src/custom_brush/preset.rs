//! Generated brushes: never stored, sized by the brush size control.

use super::brush::{Handle, MAX_BRUSH_SIDE};
use super::stamp::Stamp;
use crate::brush::in_brush;
use crate::orient::Orientation;

/// A generated brush shape. The round brush is [`BrushChoice::Round`](super::BrushChoice).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preset {
    #[default]
    Square,
    Diamond,
    HorizontalLine,
    VerticalLine,
}

impl Preset {
    pub const ALL: [Preset; 4] = [
        Preset::Square,
        Preset::Diamond,
        Preset::HorizontalLine,
        Preset::VerticalLine,
    ];

    /// The stamp of a `size` wide brush painting `fill`, centred by `brush_origin`'s rule.
    pub fn stamp(self, size: i32, fill: u32) -> Stamp {
        let n = size.clamp(1, MAX_BRUSH_SIDE as i32) as usize;
        let dims = match self {
            Preset::Square | Preset::Diamond => (n, n),
            Preset::HorizontalLine => (n, 1),
            Preset::VerticalLine => (1, n),
        };
        Stamp::build(dims, Orientation::IDENTITY, Handle::Centre, |x, y| {
            self.covers(n as i32, x as i32, y as i32).then_some(fill)
        })
    }

    fn covers(self, n: i32, dx: i32, dy: i32) -> bool {
        match self {
            Preset::Diamond => {
                let (ex, ey) = (dx * 2 - (n - 1), dy * 2 - (n - 1));
                ex.abs() + ey.abs() <= n
            }
            _ => true,
        }
    }
}

/// The round brush as a stamp, by the same shape the pencil paints.
pub(super) fn round_stamp(size: i32, fill: u32) -> Stamp {
    let n = size.clamp(1, MAX_BRUSH_SIDE as i32);
    Stamp::build(
        (n as usize, n as usize),
        Orientation::IDENTITY,
        Handle::Centre,
        |x, y| in_brush(n, x as i32, y as i32).then_some(fill),
    )
}
