//! A tilemap layer on the canvas: in Tiles mode the paint tools place, clear, fill and pick
//! cells; in Pixels mode they draw on the tiles placed (the 10.6 edit-through), and pixels over
//! empty cells are dropped with a word on the status line. While a tilemap layer is active the
//! canvas shows its cell grid, and the cells of a stroke not yet let go.

use crate::app::{App, Gesture};
use crate::panel_tiles::shown_color;
use pikslar_core::tilemap::PendingCell;
use pikslar_core::{DrawCmd, TileCmd, TileTool, Tool, Turn};
use pikslar_platform::{Button, Mods};
use pikslar_render::{draw_frame, Rect, Surface};
use pikslar_ui::theme;

/// Said after a pixel stroke on a map that dropped pixels over empty cells.
pub(crate) const PLACE_TILES_FIRST: &str = "Place tiles here first";
/// The grid is left out when its cells would be closer than this on screen, in pixels.
const LEAST_GRID_STEP: i32 = 4;

/// The tile tool a paint tool is in Tiles mode.
pub(crate) fn map_tool(tool: Tool) -> TileTool {
    match tool {
        Tool::Pencil => TileTool::Place,
        Tool::Eraser => TileTool::Erase,
        Tool::Fill => TileTool::Fill,
        Tool::Picker => TileTool::Pick,
        Tool::Line => TileTool::Line,
        Tool::Rect => TileTool::Rect,
    }
}

impl App {
    /// Begins a map stroke if the active layer is a tilemap layer in Tiles mode. Returns false
    /// if the press is for the pixel tools.
    pub(crate) fn tile_canvas_down(
        &mut self,
        (x, y): (i32, i32),
        chosen: Tool,
        button: Button,
        mods: Mods,
    ) -> bool {
        if !self.placing_tiles() {
            return false;
        }
        let tool = match (self.terrain_tool(chosen, button, mods), button) {
            (Some(terrain), _) => terrain,
            (None, Button::Right) => TileTool::Erase,
            _ if mods.alt => TileTool::Pick,
            _ => map_tool(chosen),
        };
        if tool == TileTool::Pick {
            // A picked tile is what the tools place next.
            self.tile_ui.terrain.painting = false;
        }
        let (dx, dy) = self.view.to_doc(x, y);
        match self.editor.apply(TileCmd::Begin { tool, x: dx, y: dy }) {
            Ok(_) if self.editor.is_stroking() => {
                self.gesture = Gesture::Stroke;
                self.tile_ui.map_stroke = true;
            }
            Ok(_) => self.tile_ui.block = None,
            Err(e) => self.notify_error(e.to_string()),
        }
        self.full_canvas = true;
        self.ui_dirty = true;
        true
    }

    /// Continues a map stroke. The canvas is drawn again whole, to show its cells.
    pub(crate) fn tile_canvas_move(&mut self, x: i32, y: i32) {
        let (dx, dy) = self.view.to_doc(x, y);
        self.run_on_canvas(TileCmd::To { x: dx, y: dy });
        self.full_canvas = true;
    }

    /// Ends the stroke in progress, a map stroke or a pixel one.
    pub(crate) fn commit_stroke(&mut self) {
        if std::mem::take(&mut self.tile_ui.map_stroke) {
            self.run(TileCmd::Commit);
            self.full_canvas = true;
            return;
        }
        self.run(DrawCmd::Commit);
        if self.active_tileset().is_some() && self.editor.pixels_off_tiles() > 0 {
            self.notify_error(PLACE_TILES_FIRST.to_string());
        }
    }

    /// Abandons the stroke in progress.
    pub(crate) fn cancel_stroke(&mut self) {
        if std::mem::take(&mut self.tile_ui.map_stroke) {
            self.run(TileCmd::Cancel);
        } else {
            self.run(DrawCmd::Cancel);
        }
    }

    /// The flip and rotate keys turn the stamp in Tiles mode. Returns false if there is none.
    pub(crate) fn turn_tile_key(&mut self, turn: Turn) -> bool {
        if !self.placing_tiles() || self.editor.is_floating() {
            return false;
        }
        match self.editor.turn_tile_stamp(turn) {
            Ok(turned) => {
                self.ui_dirty |= turned;
                turned
            }
            Err(e) => {
                self.notify(e.to_string());
                true
            }
        }
    }

    /// Delete in Tiles mode empties the selected cells.
    pub(crate) fn delete_selected(&mut self) {
        if self.placing_tiles() {
            self.select_change(TileCmd::DeleteSelected);
        } else {
            self.select_change(DrawCmd::DeleteSelected);
        }
    }

    /// The grid shown: the active tilemap layer, its tile size and its cel's place.
    fn grid_now(&self) -> Option<crate::panel_tiles::GridKey> {
        let set = self.active_tileset()?;
        let size = self.editor.doc().tileset(set).ok()?.tile_size();
        let layer = &self.editor.doc().layers()[self.editor.layer];
        let frame = self.editor.frame.min(layer.frame_count().saturating_sub(1));
        Some((self.editor.layer, size, layer.cel(frame).position()))
    }

    /// Draws the canvas again whole when the grid it shows changes.
    pub(crate) fn follow_tile_grid(&mut self) {
        let now = self.grid_now();
        if self.tile_ui.drawn_grid != now {
            self.tile_ui.drawn_grid = now;
            self.full_canvas = true;
        }
    }

    /// The grid of the active tilemap layer and the cells of the stroke in progress, within
    /// `area` of the screen.
    pub(crate) fn draw_tile_marks(&self, sf: &mut Surface<'_>, area: Rect) {
        let Some((_, (tw, th), (ox, oy))) = self.tile_ui.drawn_grid else {
            return;
        };
        self.draw_grid(sf, area, (tw as i32, th as i32), (ox.into(), oy.into()));
        self.draw_pending(sf, area);
    }

    fn draw_grid(
        &self,
        sf: &mut Surface<'_>,
        area: Rect,
        (tw, th): (i32, i32),
        (ox, oy): (i32, i32),
    ) {
        let (view, doc) = (self.view, self.editor.doc());
        let sprite = Rect::new(0, 0, doc.width as i32, doc.height as i32);
        let clip = view.to_screen(sprite).intersect(area);
        if clip.is_empty() || tw * view.num / view.den < LEAST_GRID_STEP {
            return;
        }
        if th * view.num / view.den < LEAST_GRID_STEP {
            return;
        }
        let mut x = ox.rem_euclid(tw);
        while x <= sprite.w {
            let at = view.to_screen(Rect::new(x, 0, 1, 1)).x;
            sf.fill(
                Rect::new(at, clip.y, 1, clip.h).intersect(clip),
                theme::muted(),
            );
            x += tw;
        }
        let mut y = oy.rem_euclid(th);
        while y <= sprite.h {
            let at = view.to_screen(Rect::new(0, y, 1, 1)).y;
            sf.fill(
                Rect::new(clip.x, at, clip.w, 1).intersect(clip),
                theme::muted(),
            );
            y += th;
        }
    }

    /// The cells a map stroke has placed, or a terrain stroke would change, and not yet put
    /// into the map.
    pub(crate) fn pending_now(&self) -> Vec<PendingCell> {
        let mut cells = self.editor.pending_cells();
        let (Some(preview), Some((_, (tw, th), (ox, oy)))) =
            (self.editor.terrain_preview(), self.grid_now())
        else {
            return cells;
        };
        let (tw, th) = (tw as i32, th as i32);
        cells.extend(preview.cells.into_iter().map(|((x, y), cell)| PendingCell {
            rect: (i32::from(ox) + x * tw, i32::from(oy) + y * th, tw, th),
            cell,
        }));
        cells
    }

    /// The cells a map stroke has placed and not yet put into the map: what they will show,
    /// each framed; a cleared cell only framed.
    fn draw_pending(&self, sf: &mut Surface<'_>, area: Rect) {
        let (Some(set), true) = (self.active_tileset(), self.tile_ui.map_stroke) else {
            return;
        };
        let doc = self.editor.doc();
        let Ok(mode) = doc.tileset(set).map(|s| s.mode()) else {
            return;
        };
        for pending in self.pending_now() {
            let (x, y, w, h) = pending.rect;
            if !pending.cell.is_empty() {
                for py in 0..h {
                    for px in 0..w {
                        let raw = doc.cell_pixel(set, pending.cell, px as usize, py as usize);
                        if let Some(color) = shown_color(doc, mode, raw) {
                            let at = self.view.to_screen(Rect::new(x + px, y + py, 1, 1));
                            sf.fill(at.intersect(area), crate::color_ui::to_screen(color));
                        }
                    }
                }
            }
            draw_frame(sf, &self.view, area, Rect::new(x, y, w, h), theme::accent());
        }
    }
}

#[cfg(test)]
#[path = "tile_canvas_tests.rs"]
mod tests;
