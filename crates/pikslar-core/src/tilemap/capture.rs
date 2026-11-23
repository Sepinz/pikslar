//! Capturing a stamp from the map under the selection.

use super::cell::MapCell;
use super::stamp::{TileStamp, MAX_STAMP_SIDE};
use crate::document::LimitError;
use crate::editor::Editor;

impl Editor {
    /// Makes a stamp of the tiles of the active map whose cells' centres the selection holds,
    /// trimmed to the cells that hold a tile, and chooses it. A place inside the block the
    /// selection did not hold stays empty. Refused with `NothingToCapture` when nothing is
    /// selected or no selected cell holds a tile, and `StampTooLarge` past 64 by 64 cells.
    pub fn capture_tile_stamp(&mut self) -> Result<(), LimitError> {
        let tileset = self.active_map()?;
        let geo = self.map_geometry()?;
        let selection = self.selection.clone().ok_or(LimitError::NothingToCapture)?;
        let (x, y, w, h) = selection.bounds();
        let (a, b) = (geo.cell_of(x, y), geo.cell_of(x + w - 1, y + h - 1));
        let (left, top, right, bottom) = geo.window();
        let (x0, y0, x1, y1) = (a.0.max(left), a.1.max(top), b.0.min(right), b.1.min(bottom));
        // Rows and columns at the edge may hold no selected centre, so two more than the most.
        let most = MAX_STAMP_SIDE as i32 + 2;
        if x1 - x0 >= most || y1 - y0 >= most {
            return Err(LimitError::StampTooLarge);
        }
        let image = self.doc.layers[self.layer].cel(self.frame).image();
        let mut found: Vec<((i32, i32), MapCell)> = Vec::new();
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                let (px, py) = geo.centre((cx, cy));
                let cell = MapCell::from_bits(image.get(cx, cy));
                if !cell.is_empty() && selection.contains(px, py) {
                    found.push(((cx, cy), cell));
                }
            }
        }
        let xs = found.iter().map(|((cx, _), _)| *cx);
        let ys = found.iter().map(|((_, cy), _)| *cy);
        let (Some(left), Some(right)) = (xs.clone().min(), xs.max()) else {
            return Err(LimitError::NothingToCapture);
        };
        let (Some(top), Some(bottom)) = (ys.clone().min(), ys.max()) else {
            return Err(LimitError::NothingToCapture);
        };
        let (width, height) = ((right - left + 1) as usize, (bottom - top + 1) as usize);
        let mut cells = vec![MapCell::EMPTY; width * height];
        for ((cx, cy), cell) in found {
            cells[(cy - top) as usize * width + (cx - left) as usize] = cell;
        }
        let stamp = TileStamp::block(tileset, width, height, cells)?;
        self.tiles.set_stamp(Some(stamp));
        Ok(())
    }
}
