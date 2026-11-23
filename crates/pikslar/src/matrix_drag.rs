//! Choosing a block of cels in the matrix and dropping it somewhere else. Everything here is
//! view state until the drop, which is one command. Provisional: see docs/cel-matrix.md.
//!
//! A press picks the cel; Shift-click picks every cel between the current one and it. A drag
//! from a picked cel carries the whole block: on its own it moves, with Ctrl it copies, with
//! Alt it links, and with Shift it lands in new frames instead of over the cels there.

use pikslar_core::cel_drag::{CelDrag, DragKind, Landing};
use pikslar_platform::Mods;

/// A cel by layer, counted from the bottom, and frame.
pub(crate) type At = (usize, usize);

/// A rectangle of cels: some layers next to each other, across some frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub layers: (usize, usize),
    pub frames: (usize, usize),
}

impl Block {
    pub fn single((layer, frame): At) -> Block {
        Block {
            layers: (layer, layer),
            frames: (frame, frame),
        }
    }

    /// Every cel between two, both included.
    pub fn spanning(a: At, b: At) -> Block {
        Block {
            layers: (a.0.min(b.0), a.0.max(b.0)),
            frames: (a.1.min(b.1), a.1.max(b.1)),
        }
    }

    pub fn contains(&self, (layer, frame): At) -> bool {
        (self.layers.0..=self.layers.1).contains(&layer)
            && (self.frames.0..=self.frames.1).contains(&frame)
    }

    /// True while every cel of the block is still in a document of this size.
    pub fn fits(&self, layers: usize, frames: usize) -> bool {
        self.layers.1 < layers && self.frames.1 < frames
    }

    /// Where a drag puts the block.
    pub fn landed(drag: &CelDrag) -> Block {
        let (height, width) = (drag.layers.1 - drag.layers.0, drag.frames.1 - drag.frames.0);
        Block {
            layers: (drag.to_layer, drag.to_layer + height),
            frames: (drag.to_frame, drag.to_frame + width),
        }
    }
}

/// A press on the matrix, until the button comes up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Press {
    /// The cel pressed.
    pub at: At,
    /// The cel under the pointer now. Its frame may be one past the last, for an insert there.
    pub over: At,
    /// The keys held at the press, which choose what a drag does.
    pub mods: Mods,
}

impl Press {
    pub fn new(at: At, mods: Mods) -> Press {
        Press { at, over: at, mods }
    }

    pub fn moved(&self) -> bool {
        self.over != self.at
    }
}

/// What the drop of a dragged block does: it keeps its place against the grabbed cel, and stays
/// inside the document. `None` while the pointer is still on the cel it pressed.
pub(crate) fn drop_drag(
    block: Block,
    press: &Press,
    layers: usize,
    frames: usize,
) -> Option<CelDrag> {
    if !press.moved() || !block.fits(layers, frames) {
        return None;
    }
    let kind = match (press.mods.alt, press.mods.ctrl) {
        (true, _) => DragKind::Link,
        (false, true) => DragKind::Copy,
        (false, false) => DragKind::Move,
    };
    let landing = match press.mods.shift {
        true => Landing::Insert,
        false => Landing::Overwrite,
    };
    let height = block.layers.1 - block.layers.0 + 1;
    let width = block.frames.1 - block.frames.0 + 1;
    let last_frame = match landing {
        Landing::Overwrite => frames - width,
        Landing::Insert => frames,
    };
    let along = |start: usize, from: usize, to: usize, last: usize| {
        (start as i64 + to as i64 - from as i64).clamp(0, last as i64) as usize
    };
    Some(CelDrag {
        layers: block.layers,
        frames: block.frames,
        to_layer: along(block.layers.0, press.at.0, press.over.0, layers - height),
        to_frame: along(block.frames.0, press.at.1, press.over.1, last_frame),
        kind,
        landing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(at: At, over: At, mods: Mods) -> Press {
        Press { at, over, mods }
    }

    const PLAIN: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
    };

    #[test]
    fn a_block_spans_two_cels_in_any_order() {
        let b = Block::spanning((3, 1), (1, 4));
        assert_eq!(b.layers, (1, 3));
        assert_eq!(b.frames, (1, 4));
        assert!(b.contains((2, 2)) && !b.contains((0, 2)) && !b.contains((2, 5)));
        assert!(b.fits(4, 5) && !b.fits(3, 5) && !b.fits(4, 4));
    }

    #[test]
    fn the_keys_at_the_press_choose_the_kind_of_drag() {
        let block = Block::single((0, 0));
        let kinds = [
            (PLAIN, DragKind::Move, Landing::Overwrite),
            (
                Mods {
                    ctrl: true,
                    ..PLAIN
                },
                DragKind::Copy,
                Landing::Overwrite,
            ),
            (
                Mods { alt: true, ..PLAIN },
                DragKind::Link,
                Landing::Overwrite,
            ),
            (
                Mods {
                    alt: true,
                    ctrl: true,
                    ..PLAIN
                },
                DragKind::Link,
                Landing::Overwrite,
            ),
            (
                Mods {
                    shift: true,
                    ..PLAIN
                },
                DragKind::Move,
                Landing::Insert,
            ),
        ];
        for (mods, kind, landing) in kinds {
            let d = drop_drag(block, &press((0, 0), (0, 2), mods), 1, 4).expect("moved");
            assert_eq!((d.kind, d.landing), (kind, landing), "{mods:?}");
        }
    }

    #[test]
    fn the_block_follows_the_grabbed_cel_and_stays_inside() {
        let block = Block::spanning((1, 1), (2, 2));
        let d = drop_drag(block, &press((2, 2), (2, 3), PLAIN), 4, 6).expect("moved");
        assert_eq!((d.to_layer, d.to_frame), (1, 2));
        assert_eq!(Block::landed(&d), Block::spanning((1, 2), (2, 3)));
        let far = drop_drag(block, &press((1, 1), (9, 40), PLAIN), 4, 6).expect("moved");
        assert_eq!((far.to_layer, far.to_frame), (2, 4), "kept inside");
        let insert = Mods {
            shift: true,
            ..PLAIN
        };
        let end = drop_drag(block, &press((1, 1), (1, 40), insert), 4, 6).expect("moved");
        assert_eq!(end.to_frame, 6, "an insert may land after the last frame");
        let left = drop_drag(block, &press((2, 2), (0, 0), PLAIN), 4, 6).expect("moved");
        assert_eq!((left.to_layer, left.to_frame), (0, 0));
    }

    #[test]
    fn no_drag_without_moving_or_with_a_block_gone_stale() {
        let block = Block::single((0, 0));
        assert_eq!(drop_drag(block, &Press::new((0, 0), PLAIN), 1, 3), None);
        let gone = Block::single((2, 0));
        assert_eq!(drop_drag(gone, &press((2, 0), (2, 1), PLAIN), 2, 3), None);
    }
}
