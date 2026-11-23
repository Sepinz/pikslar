//! The tool strip's brush button: it shows the brush in use as a small picture and opens the
//! brush menu.

use crate::app::App;
use crate::frame_menu::MenuKind;
use crate::panel_side::ID_BRUSH;
use pikslar_core::custom_brush::BrushChoice;
use pikslar_core::{rgba, ColorMode};
use pikslar_render::{Rect, Surface};
use pikslar_ui::chrome::{self, Look};
use pikslar_ui::role::{Role, State};
use pikslar_ui::theme;
use std::cell::RefCell;

/// The picture is this many cells on a side, whatever the brush.
const CELLS: usize = 16;
/// The room right of the picture that holds the menu caret, in units.
pub(crate) const CARET_ROOM: i32 = 12;

/// A caret pointing down, five units wide, its top row's left end at `(x, y)` less a unit.
fn caret(sf: &mut Surface<'_>, (x, y): (i32, i32), s: i32, ink: u32) {
    for row in 0..3 {
        let r = Rect::new(x + row * s, y - s + row * s, (5 - 2 * row) * s, s);
        sf.fill(r, ink);
    }
}

/// The picture of a brush, remembered until the brush or its size changes.
#[derive(Default)]
pub(crate) struct Thumb(RefCell<Option<(BrushChoice, i32, Vec<u32>)>>);

impl App {
    /// Draws the button in `rect`.
    pub(crate) fn draw_brush_button(&mut self, sf: &mut Surface<'_>, rect: Rect) {
        let s = self.ui.scale;
        let hint = format!(
            "Brush: shapes, captured brushes, the library  {} captures",
            self.keymap.chord(crate::shortcuts::Action::CaptureBrush)
        );
        self.note_hint(rect, hint);
        let open = self
            .menu
            .is_some_and(|m| matches!(m.kind, MenuKind::Brush | MenuKind::BrushHandle));
        let id = ID_BRUSH + 4;
        let state = self.ui.interact(id, rect);
        self.ui.describe(Role::Button, State::chosen(open));
        self.ui.name(id, "Brush");
        let look = Look::of(state, open, false);
        chrome::face(sf, rect, s, theme::panel(), look);
        // The picture on the left and a caret on the right, so it reads as a menu of brushes
        // and not as a colour swatch (final review, defect 14).
        let picture = Rect::new(rect.x, rect.y, rect.w - CARET_ROOM * s, rect.h);
        self.paint_thumb(sf, picture);
        let tip = (picture.right() + s, rect.y + rect.h / 2 + look.drop() * s);
        caret(sf, tip, s, look.ink());
        self.ui.ring(sf, id, rect);
        if state.clicked {
            let at = (rect.x, rect.bottom());
            self.defer(move |a| a.open_brush_menu(at));
        }
    }

    fn paint_thumb(&self, sf: &mut Surface<'_>, rect: Rect) {
        let s = self.ui.scale;
        let side = CELLS as i32 * s;
        let (left, top) = (rect.x + (rect.w - side) / 2, rect.y + (rect.h - side) / 2);
        let cells = self.thumb_cells();
        for (i, &argb) in cells.iter().enumerate().filter(|(_, &c)| c != 0) {
            let (tx, ty) = ((i % CELLS) as i32, (i / CELLS) as i32);
            sf.fill(Rect::new(left + tx * s, top + ty * s, s, s), argb);
        }
    }

    /// The picture, row by row, 0 where nothing is drawn.
    fn thumb_cells(&self) -> Vec<u32> {
        let choice = self.editor.brushes().active();
        let size = self.editor.brush_size;
        let mut cache = self.brush_ui.thumb.0.borrow_mut();
        if let Some((c, n, cells)) = cache.as_ref() {
            if c == choice && *n == size {
                return cells.clone();
            }
        }
        let cells = self.build_thumb(choice, size);
        *cache = Some((choice.clone(), size, cells.clone()));
        cells
    }

    fn build_thumb(&self, choice: &BrushChoice, size: i32) -> Vec<u32> {
        let table = &self.editor.doc().colors;
        let fill = rgba(0xde, 0xdb, 0xd2, 255);
        let Ok(prepared) = choice.prepare(size, ColorMode::Rgba, table, fill) else {
            return vec![0; CELLS * CELLS];
        };
        let stamp = prepared.stamp;
        let (w, h) = stamp.size();
        let (left, top) = stamp.offset();
        let (w, h) = (w.max(1) as usize, h.max(1) as usize);
        let mut dense = vec![0u32; w * h];
        for run in stamp.runs() {
            let (row, first) = ((run.dy - top) as usize, (run.dx - left) as usize);
            for (i, &value) in run.values.iter().enumerate() {
                if let Some(cell) = dense.get_mut(row * w + first + i) {
                    *cell = value;
                }
            }
        }
        let longest = w.max(h);
        let mut out = vec![0u32; CELLS * CELLS];
        for ty in 0..CELLS {
            for tx in 0..CELLS {
                let (sx, sy) = (tx * longest / CELLS, ty * longest / CELLS);
                if sx < w && sy < h {
                    out[ty * CELLS + tx] = to_surface(dense[sy * w + sx]);
                }
            }
        }
        out
    }
}

/// A cell in the surface's pixel format; 0 stays "nothing".
fn to_surface(cell: u32) -> u32 {
    if cell == 0 {
        return 0;
    }
    let [r, g, b, _] = cell.to_le_bytes();
    0xff00_0000 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_are_shown_with_red_and_blue_in_the_surface_order() {
        let red = rgba(255, 0, 0, 255);
        assert_eq!(to_surface(red), 0xffff_0000);
        assert_eq!(to_surface(0), 0);
    }
}
