//! Drawing: turning pointer input into stamps, lines, rectangles and fills on the overlay.

use super::{Damage, Editor, Stroke, Tool};
use crate::brush::{brush_origin, in_brush};
use crate::color::{ColorTable, TableFull, MAX_COLORS};
use crate::custom_brush::Stamp;
use crate::document::{ColorMode, LimitError};
use crate::history::Edit;
use crate::layer::Cel;
use crate::overlay::Overlay;
use crate::rhythm::Trail;
use crate::span_fill::scan_fill;
use crate::tile::{TILE, TILE_SHIFT};
use crate::tilemap::{write_through, TileEdit};
use crate::tileset::TileId;
use std::sync::Arc;

/// A line from one point to another.
type Line = ((i32, i32), (i32, i32));

impl Editor {
    /// Converts the current color to a cell value for the active layer, and returns the color
    /// table as it was if that value needed adding to it.
    /// Refuses before anything has happened, so floating pixels stay floating.
    fn check_color_fits(&self) -> Result<(), TableFull> {
        let colors = &self.doc.colors;
        let new = colors.id_for_existing(self.color).is_none();
        if self.mode() == ColorMode::Palette && new && colors.len() >= MAX_COLORS {
            return Err(TableFull);
        }
        Ok(())
    }

    fn paint_value(&mut self) -> Result<(u32, Option<Arc<ColorTable>>), TableFull> {
        match self.mode() {
            ColorMode::Rgba => Ok((self.color, None)),
            ColorMode::Palette => match self.doc.colors.id_for_existing(self.color) {
                Some(id) => Ok((id, None)),
                None => {
                    let before = self.doc.colors.clone();
                    let id = Arc::make_mut(&mut self.doc.colors).id_for(self.color)?;
                    Ok((id, Some(before)))
                }
            },
        }
    }

    /// Paints one brush stamp centered on a point.
    fn stamp(&mut self, x: i32, y: i32, value: u32, shape: Option<&Stamp>) {
        if let Some(shape) = shape {
            self.stamp_cells(shape, x, y);
            return;
        }
        let n = self.brush_size.max(1);
        if n == 1 {
            self.plot(x, y, value);
            return;
        }
        let (left, top) = brush_origin(x, y, n);
        for dy in 0..n {
            for dx in (0..n).filter(|&dx| in_brush(n, dx, dy)) {
                self.plot(left + dx, top + dy, value);
            }
        }
    }

    pub(crate) fn begin(&mut self, tool: Tool, x: i32, y: i32) -> Result<(), LimitError> {
        let own = self.stamp_has_own_colors(tool);
        if !matches!(tool, Tool::Eraser | Tool::Picker) && !own {
            self.check_color_fits()?;
        }
        self.check_stamp_fits(tool)?;
        self.anchor();
        self.cancel();
        if tool == Tool::Picker {
            let picked = self.color_at(x, y);
            if picked != 0 {
                self.color = picked;
            }
            return Ok(());
        }
        let (value, colors_before) = if tool == Tool::Eraser || own {
            (0, None)
        } else {
            self.paint_value()?
        };
        let (stamp, colors_before) = self.begin_stamp(tool, value, colors_before)?;
        let rhythm = self.rhythm && tool == Tool::Pencil && self.brush_size <= 1 && stamp.is_none();
        if tool == Tool::Fill {
            self.flood(x, y, value, colors_before.clone());
            if self.stroke.is_none() {
                if let Some(colors) = colors_before {
                    self.doc.colors = colors;
                }
            }
            self.commit();
            return Ok(());
        }
        self.stroke = Some(Stroke {
            tool,
            value,
            start: (x, y),
            last: (x, y),
            trail: vec![(x, y)],
            rhythm: rhythm.then(|| Box::new(Trail::new((x, y)))),
            colors_before,
            stamp: None,
        });
        self.stamp(x, y, value, stamp.as_deref());
        if let Some(stroke) = self.stroke.as_mut() {
            stroke.stamp = stamp;
        }
        Ok(())
    }

    /// Continues the stroke to a new position.
    pub(crate) fn drag(&mut self, x: i32, y: i32) {
        let Some(mut stroke) = self.stroke.take() else {
            return;
        };
        match stroke.tool {
            Tool::Pencil | Tool::Eraser => self.freehand(&mut stroke, x, y),
            Tool::Line => {
                self.reset_preview();
                let (sx, sy) = stroke.start;
                self.draw_line((sx, sy), (x, y), stroke.value, stroke.stamp.as_deref());
            }
            Tool::Rect => {
                self.reset_preview();
                self.draw_rect(stroke.start, (x, y), stroke.value, stroke.stamp.as_deref());
            }
            Tool::Fill | Tool::Picker => {}
        }
        stroke.last = (x, y);
        self.stroke = Some(stroke);
    }

    fn freehand(&mut self, stroke: &mut Stroke, x: i32, y: i32) {
        if let Some(trail) = stroke.rhythm.as_deref_mut() {
            trail.extend_to(x, y);
            self.apply_rhythm(trail, stroke.value);
            return;
        }
        let (lx, ly) = stroke.last;
        let value = stroke.value;
        let perfect = self.pixel_perfect
            && stroke.tool == Tool::Pencil
            && self.brush_size <= 1
            && stroke.stamp.is_none();
        let mut prev = None;
        for_line(lx, ly, x, y, |px, py| {
            if stroke.trail.last() == Some(&(px, py)) {
                return;
            }
            stroke.trail.push((px, py));
            match stroke.stamp.as_deref() {
                Some(shape) => self.stamp_path_point(&mut prev, shape, (px, py)),
                None => self.stamp(px, py, value, None),
            }
            if perfect {
                remove_corner(
                    &mut stroke.trail,
                    &mut self.overlay,
                    &mut self.damage,
                    self.doc.width,
                );
            }
        });
        if let Some(shape) = stroke.stamp.as_deref() {
            self.stamp_path_end(prev, shape);
        }
    }

    /// Replaces the live end of a resolved stroke with its newest form.
    fn apply_rhythm(&mut self, trail: &mut Trail, value: u32) {
        let old = trail.tail().to_vec();
        trail.update();
        for &(x, y) in old.iter().filter(|c| !trail.is_frozen(**c)) {
            self.overlay.remove(x, y);
            self.mark(x, y);
        }
        for &(x, y) in trail.settled().iter().chain(trail.tail()) {
            self.plot(x, y, value);
        }
    }

    /// Moves the stroke into the document as one undo step: into the active cel, or on a
    /// tilemap layer into the tiles its cells place. Refused with `NoTileHere`, leaving the
    /// document as it was, only when every pixel of a stroke on a map fell on empty cells.
    pub(crate) fn finish_stroke(&mut self) -> Result<(), LimitError> {
        self.commit_tile_stroke();
        let Some(stroke) = self.stroke.take() else {
            return Ok(());
        };
        if self.on_tilemap() {
            return self.commit_through_tiles(stroke.colors_before);
        }
        self.commit_overlay(None, stroke.colors_before);
        Ok(())
    }

    /// Writes the overlay into the tiles the active map places under it, as one step of tile
    /// images. Only where the overlay was shown and where the edited tiles are placed is drawn
    /// again.
    fn commit_through_tiles(&mut self, colors: Option<Arc<ColorTable>>) -> Result<(), LimitError> {
        let through = write_through(&self.doc, self.layer, self.frame, &self.overlay);
        let shown = self.overlay.touched().to_vec();
        self.overlay.clear();
        self.tiles.off_tiles = through.dropped;
        let set = self.doc.layers[self.layer].tileset();
        let refusal = match set {
            Some(set) => self.doc.check_tile_swap(set, &through.images).err(),
            None => None,
        };
        let (Some(set), false, None) = (set, through.images.is_empty(), refusal) else {
            if let Some(colors) = colors {
                self.doc.colors = colors;
            }
            shown.iter().for_each(|&t| self.damage.add(t));
            return match (refusal, through.landed, through.dropped) {
                (Some(error), _, _) => Err(error),
                (None, 0, 1..) => Err(LimitError::NoTileHere),
                (None, _, _) => Ok(()),
            };
        };
        let ids: Vec<TileId> = through.images.iter().map(|(id, _)| *id).collect();
        let mut before = through.images;
        self.doc.swap_tile_images(set, &mut before);
        self.record(Edit::tile_images(set, before.into(), colors));
        self.damage_written(TileEdit::new(set, &ids).with_canvas_tiles(&shown));
        Ok(())
    }

    /// Joins a tile edit to the damage so far, which is the stroke's own while it was drawn.
    fn damage_written(&mut self, edit: TileEdit) {
        self.damage = match std::mem::take(&mut self.damage) {
            Damage::None => Damage::Placed(edit),
            Damage::Tiles(tiles) => Damage::Placed(edit.with_canvas_tiles(&tiles)),
            Damage::Placed(mut own) => match own.join(&edit) {
                true => Damage::Placed(own),
                false => Damage::All,
            },
            Damage::All => Damage::All,
        };
    }

    /// Removes one cell as its own undo step, for fixes offered by Pixel Lint.
    pub(crate) fn erase_cell(&mut self, x: i32, y: i32) {
        self.anchor();
        self.cancel();
        self.plot_unclipped(x, y, 0);
        self.commit_overlay(None, None);
    }

    pub(crate) fn reset_preview(&mut self) {
        for &t in self.overlay.touched() {
            self.damage.add(t);
        }
        self.overlay.clear();
    }

    fn draw_line(&mut self, from: (i32, i32), to: (i32, i32), value: u32, shape: Option<&Stamp>) {
        self.draw_path(&[(from, to)], value, shape);
    }

    /// The stamp (or the round brush) along each line in turn, as one path.
    fn draw_path(&mut self, lines: &[Line], value: u32, shape: Option<&Stamp>) {
        let Some(shape) = shape else {
            for &(a, b) in lines {
                for_line(a.0, a.1, b.0, b.1, |x, y| self.stamp(x, y, value, None));
            }
            return;
        };
        let mut prev = None;
        for &(a, b) in lines {
            for_line(a.0, a.1, b.0, b.1, |x, y| {
                self.stamp_path_point(&mut prev, shape, (x, y));
            });
        }
        self.stamp_path_end(prev, shape);
    }

    fn draw_rect(&mut self, a: (i32, i32), b: (i32, i32), value: u32, shape: Option<&Stamp>) {
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        let sides = [
            ((x0, y0), (x1, y0)),
            ((x0, y1), (x1, y1)),
            ((x0, y0), (x0, y1)),
            ((x1, y0), (x1, y1)),
        ];
        self.draw_path(&sides, value, shape);
    }

    #[inline]
    pub(super) fn plot(&mut self, x: i32, y: i32, value: u32) {
        if self.selection.as_ref().is_none_or(|s| s.contains(x, y)) {
            self.plot_unclipped(x, y, value);
        }
    }

    /// Like `plot`, but leaves the damage to the caller, which marks a whole stamp at once.
    #[inline]
    pub(super) fn plot_unmarked(&mut self, x: i32, y: i32, value: u32) {
        if self.selection.as_ref().is_none_or(|s| s.contains(x, y)) {
            self.overlay.put(x, y, value);
        }
    }

    #[inline]
    pub(crate) fn plot_unclipped(&mut self, x: i32, y: i32, value: u32) {
        self.overlay.put(x, y, value);
        self.mark(x, y);
    }

    /// Marks every tile a `w` by `h` area at (`x`, `y`) touches, in order and each once.
    pub(super) fn mark_area(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let (width, height) = (self.doc.width as i32, self.doc.height as i32);
        let (x1, y1) = (
            x.saturating_add(w).min(width),
            y.saturating_add(h).min(height),
        );
        let (x0, y0) = (x.max(0), y.max(0));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let tiles = (x0 as usize >> TILE_SHIFT)..=((x1 - 1) as usize >> TILE_SHIFT);
        for ty in (y0 as usize >> TILE_SHIFT)..=((y1 - 1) as usize >> TILE_SHIFT) {
            for tx in tiles.clone() {
                let index = self.overlay.tile_index(tx << TILE_SHIFT, ty << TILE_SHIFT);
                self.damage.add(index as u32);
            }
        }
    }

    #[inline]
    fn mark(&mut self, x: i32, y: i32) {
        if x >= 0 && y >= 0 && (x as usize) < self.doc.width && (y as usize) < self.doc.height {
            let index = self.overlay.tile_index(x as usize, y as usize);
            self.damage.add(index as u32);
        }
    }

    /// Fills the 4-connected region of matching cells under the point.
    fn flood(&mut self, x: i32, y: i32, value: u32, colors_before: Option<Arc<ColorTable>>) {
        let (w, h) = (self.doc.width as i32, self.doc.height as i32);
        if x < 0 || y < 0 || x >= w || y >= h {
            return;
        }
        let cel = self.doc.layers[self.layer].cel(self.frame).clone();
        let selection = self.selection.clone();
        if selection.as_ref().is_some_and(|s| !s.contains(x, y)) {
            return;
        }
        let target = cel.get(x, y);
        if target == value {
            return;
        }
        // Compiled twice on purpose: most fills have no selection and must not pay for one.
        match selection {
            Some(s) => self.flood_within(&cel, (x, y), target, value, |px, py| s.contains(px, py)),
            None => self.flood_within(&cel, (x, y), target, value, |_, _| true),
        }
        self.stroke = Some(Stroke {
            tool: Tool::Fill,
            value,
            start: (x, y),
            last: (x, y),
            trail: Vec::new(),
            rhythm: None,
            colors_before,
            stamp: None,
        });
    }

    fn flood_within(
        &mut self,
        cel: &Cel,
        start: (i32, i32),
        target: u32,
        value: u32,
        inside: impl Fn(i32, i32) -> bool,
    ) {
        let (w, h) = (self.doc.width as i32, self.doc.height as i32);
        let read = cel.sampler();
        let open = |px: i32, py: i32| inside(px, py) && read(px, py) == target;
        scan_fill((w, h), start, open, |px, py| {
            self.overlay.put(px, py, value);
            self.mark(px, py);
        });
    }
}

/// Removes the middle of an L-shaped step so diagonal lines stay one pixel thin.
fn remove_corner(
    trail: &mut Vec<(i32, i32)>,
    overlay: &mut Overlay,
    damage: &mut Damage,
    width: usize,
) {
    let n = trail.len();
    if n < 3 {
        return;
    }
    let (a, b, c) = (trail[n - 3], trail[n - 2], trail[n - 1]);
    let diagonal = (a.0 - c.0).abs() == 1 && (a.1 - c.1).abs() == 1;
    let adjacent = |p: (i32, i32), q: (i32, i32)| (p.0 - q.0).abs() + (p.1 - q.1).abs() == 1;
    if diagonal && adjacent(a, b) && adjacent(b, c) && !trail[..n - 2].contains(&b) {
        overlay.remove(b.0, b.1);
        let tiles_wide = width.div_ceil(TILE) as i32;
        if b.0 >= 0 && b.1 >= 0 {
            damage.add(((b.1 >> 4) * tiles_wide + (b.0 >> 4)) as u32);
        }
        trail.remove(n - 2);
    }
}

/// Bresenham line from one point to another, inclusive.
pub fn for_line(x0: i32, y0: i32, x1: i32, y1: i32, mut plot: impl FnMut(i32, i32)) {
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    loop {
        plot(x, y);
        if x == x1 && y == y1 {
            return;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}
