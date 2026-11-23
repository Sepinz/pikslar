//! Pixels set one at a time, waiting to be written in blocks.
//!
//! A command per pixel would cost a stroke each. Instead `set_pixel` keeps its colors in
//! blocks of [`BLOCK_SIDE`] square, for one cel, and the blocks are written by one
//! `PutPixels` each before any other operation, and at the end of the run. A block written
//! this way leaves the same document as its pixels set one by one (`pikslar-api`'s
//! `contract_bulk.rs`). At most [`MOST_BLOCKS`] wait at once, so the memory this takes outside
//! the script's own cap is bounded: 256 blocks of 16 KiB, 4 MiB.

use std::collections::BTreeMap;

pub const BLOCK_SIDE: usize = 64;
pub const BLOCK_AREA: usize = BLOCK_SIDE * BLOCK_SIDE;
pub const MOST_BLOCKS: usize = 256;

/// The cel waiting pixels belong to, checked when the first of them was set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cel {
    pub layer: i64,
    pub frame: i64,
    pub width: usize,
    pub height: usize,
}

/// One block ready to write: its corner, width and colors row by row, 0 for untouched.
pub struct Block {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub colors: Vec<i64>,
}

#[derive(Default)]
pub struct Pending {
    cel: Option<Cel>,
    blocks: BTreeMap<(usize, usize), Box<[u32]>>,
}

impl Pending {
    /// The cel pixels are waiting for, if it is this one.
    pub fn is_for(&self, layer: i64, frame: i64) -> Option<Cel> {
        self.cel.filter(|c| (c.layer, c.frame) == (layer, frame))
    }

    /// Starts waiting for a cel that has been checked. Any blocks must have been taken.
    pub fn start(&mut self, cel: Cel) {
        debug_assert!(self.blocks.is_empty());
        self.cel = Some(cel);
    }

    /// Forgets the cel, so the next pixel checks it again.
    pub fn forget(&mut self) {
        self.cel = None;
    }

    /// True when setting this pixel needs a block that is not waiting yet.
    pub fn needs_block(&self, x: usize, y: usize) -> bool {
        !self.blocks.contains_key(&(x / BLOCK_SIDE, y / BLOCK_SIDE))
    }

    pub fn is_full(&self) -> bool {
        self.blocks.len() >= MOST_BLOCKS
    }

    /// Keeps a color that is not 0 for a pixel of the cel.
    pub fn put(&mut self, x: usize, y: usize, color: u32) {
        let key = (x / BLOCK_SIDE, y / BLOCK_SIDE);
        let block = self
            .blocks
            .entry(key)
            .or_insert_with(|| vec![0; BLOCK_AREA].into_boxed_slice());
        block[(y % BLOCK_SIDE) * BLOCK_SIDE + x % BLOCK_SIDE] = color;
    }

    /// The waiting blocks, cut to the canvas, ready to write; none wait afterwards.
    pub fn take(&mut self) -> Option<(Cel, Vec<Block>)> {
        let cel = self.cel?;
        if self.blocks.is_empty() {
            return None;
        }
        let blocks = std::mem::take(&mut self.blocks);
        let cut = blocks.into_iter().map(|((bx, by), colors)| {
            let (x, y) = (bx * BLOCK_SIDE, by * BLOCK_SIDE);
            let width = BLOCK_SIDE.min(cel.width - x);
            let height = BLOCK_SIDE.min(cel.height - y);
            let rows = colors.chunks(BLOCK_SIDE).take(height);
            let colors = rows
                .flat_map(|row| row[..width].iter().map(|&c| i64::from(c)))
                .collect();
            Block {
                x,
                y,
                width,
                colors,
            }
        });
        Some((cel, cut.collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cel() -> Cel {
        Cel {
            layer: 1,
            frame: 2,
            width: 70,
            height: 65,
        }
    }

    #[test]
    fn blocks_are_cut_to_the_canvas() {
        let mut p = Pending::default();
        p.start(cel());
        assert!(p.needs_block(69, 64));
        p.put(69, 64, 7);
        p.put(0, 0, 9);
        assert!(!p.needs_block(1, 1));
        let (c, blocks) = p.take().expect("waiting");
        assert_eq!(c, cel());
        assert_eq!(blocks.len(), 2);
        assert_eq!((blocks[0].x, blocks[0].y, blocks[0].width), (0, 0, 64));
        assert_eq!(blocks[0].colors.len(), 64 * 64);
        assert_eq!(blocks[0].colors[0], 9);
        assert_eq!((blocks[1].x, blocks[1].y, blocks[1].width), (64, 64, 6));
        assert_eq!(blocks[1].colors, vec![0, 0, 0, 0, 0, 7]);
        assert!(p.take().is_none(), "nothing waits after taking");
        assert_eq!(p.is_for(1, 2), Some(cel()), "the cel is still known");
    }

    #[test]
    fn a_later_color_replaces_an_earlier_one() {
        let mut p = Pending::default();
        p.start(cel());
        p.put(3, 0, 1);
        p.put(3, 0, 2);
        let (_, blocks) = p.take().expect("waiting");
        assert_eq!(blocks[0].colors[3], 2);
    }

    #[test]
    fn forgetting_the_cel_means_checking_it_again() {
        let mut p = Pending::default();
        p.start(cel());
        p.forget();
        assert_eq!(p.is_for(1, 2), None);
    }
}
