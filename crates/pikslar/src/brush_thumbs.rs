//! Pictures of the library's brushes, shown beside their names in the library menus.
//!
//! Each brush is drawn once into a picture of 16 by 16 cells, when the library is read or
//! changes, and kept: drawing the menu again only copies cells to the screen. A brush is shown
//! as it paints (its turn applied), grown by whole cells when it is small and sampled nearest
//! when it is large, centred, with its clear pixels on the canvas's checker. Library brushes
//! hold colours, not palette ids, so a picture never depends on a document's palette.

use crate::app::App;
use crate::brush_store::BrushStore;
use crate::brush_ui::PAGE;
use crate::frame_menu::MenuKind;
use pikslar_core::orient::Orientation;
use pikslar_format::brush_lib::{LibraryBrush, Paint};
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{menu_row, MENU_PAD};

/// Cells on a side of a picture.
pub(crate) const SIDE: usize = 16;
/// Cells in a picture.
pub(crate) const CELLS: usize = SIDE * SIDE;
/// Interface units a library menu leaves before each name: the picture and a gap.
pub(crate) const ROOM: i32 = SIDE as i32 + 6;
/// The canvas's checker (`pikslar_render::current_backdrop`), in squares of this many cells.
const SQUARE: usize = 4;

/// The cells a `w` by `h` brush takes in a picture: grown by the largest whole number of
/// cells that fits when it is small, shrunk to fit when it is large, never thinner than a cell.
pub(crate) fn shown_size(w: usize, h: usize) -> (usize, usize) {
    let longest = w.max(h);
    if longest == 0 {
        return (0, 0);
    }
    if longest <= SIDE {
        let k = SIDE / longest;
        return (w * k, h * k);
    }
    ((w * SIDE / longest).max(1), (h * SIDE / longest).max(1))
}

fn checker(x: usize, y: usize) -> u32 {
    let back = pikslar_render::current_backdrop();
    if (x / SQUARE + y / SQUARE) % 2 == 0 {
        back.checker_a
    } else {
        back.checker_b
    }
}

/// One cell: the pixel's colour mixed over the checker by its alpha, or the interface ink for
/// a shape brush. In the surface's format, never 0.
fn cell(paint: Paint, px: [u8; 4], back: u32) -> u32 {
    let a = u32::from(px[3]);
    if a == 0 {
        return back;
    }
    if paint == Paint::Shape {
        return theme::text();
    }
    let mix = |src: u8, shift: u32| {
        let under = (back >> shift) & 0xff;
        ((u32::from(src) * a + under * (255 - a) + 127) / 255) << shift
    };
    0xff00_0000 | mix(px[0], 16) | mix(px[1], 8) | mix(px[2], 0)
}

/// Draws `entry` into `out`, [`CELLS`] cells row by row; 0 where the picture shows nothing. A
/// damaged entry draws nothing. Allocates nothing.
pub(crate) fn draw_thumb(entry: &LibraryBrush, out: &mut [u32]) {
    out.fill(0);
    let (sw, sh) = (usize::from(entry.width), usize::from(entry.height));
    let Some(turn) = Orientation::from_index(entry.orientation) else {
        return;
    };
    if out.len() < CELLS || entry.rgba.len() != sw * sh * 4 {
        return;
    }
    let (w, h) = turn.turned_size(sw, sh);
    let (tw, th) = shown_size(w, h);
    let (left, top) = ((SIDE - tw) / 2, (SIDE - th) / 2);
    let back = turn.inverse();
    for ty in 0..th {
        for tx in 0..tw {
            let (sx, sy) = back.apply(tx * w / tw, ty * h / th, w, h);
            let i = (sy * sw + sx) * 4;
            let Some(&[r, g, b, a]) = entry.rgba.get(i..i + 4) else {
                continue;
            };
            let (cx, cy) = (left + tx, top + ty);
            out[cy * SIDE + cx] = cell(entry.paint, [r, g, b, a], checker(cx, cy));
        }
    }
}

/// The pictures of a library, made again only when its [`BrushStore::version`] changes. One
/// buffer holds them all and is reused, so a full library of 64 takes 64 KiB.
#[derive(Default)]
pub(crate) struct Thumbs {
    version: u64,
    cells: Vec<u32>,
}

impl Thumbs {
    /// Makes the pictures again if the library changed. Returns whether it did.
    pub fn update(&mut self, store: &BrushStore) -> bool {
        if self.version == store.version() {
            return false;
        }
        let brushes = store.brushes();
        self.cells.clear();
        self.cells.resize(brushes.len() * CELLS, 0);
        for (brush, out) in brushes.iter().zip(self.cells.chunks_exact_mut(CELLS)) {
            draw_thumb(brush, out);
        }
        self.version = store.version();
        true
    }

    /// The picture of the brush at `index`.
    pub fn of(&self, index: usize) -> Option<&[u32]> {
        self.cells.get(index * CELLS..(index + 1) * CELLS)
    }

    /// Bytes the pictures hold.
    #[cfg(test)]
    pub fn bytes(&self) -> usize {
        self.cells.capacity() * std::mem::size_of::<u32>()
    }
}

/// Copies a picture to the screen with its top left at `(x, y)`, each cell `s` pixels; a run of
/// one colour along a row is one fill.
pub(crate) fn paint(sf: &mut Surface<'_>, cells: &[u32], x: i32, y: i32, s: i32) {
    for (row, line) in cells.chunks_exact(SIDE).enumerate() {
        let mut start = 0;
        while start < SIDE {
            let value = line[start];
            let run = line[start..].iter().take_while(|&&c| c == value).count();
            if value != 0 {
                let at = Rect::new(x + start as i32 * s, y + row as i32 * s, run as i32 * s, s);
                sf.fill(at, value);
            }
            start += run;
        }
    }
}

/// The page of the library a menu lists, if it shows pictures.
pub(crate) fn library_page(kind: MenuKind) -> Option<usize> {
    match kind {
        MenuKind::BrushLibrary(page)
        | MenuKind::BrushRemove(page)
        | MenuKind::BrushRename(page) => Some(page),
        _ => None,
    }
}

/// The row of a menu drawn in `rect` that shows item `n` when the list starts at item `first`, or
/// none when that item is scrolled out of the menu.
pub(crate) fn shown_row(rect: Rect, n: usize, first: usize, s: i32) -> Option<Rect> {
    let row = menu_row(rect, n.checked_sub(first)?, s);
    (row.bottom() <= rect.bottom()).then_some(row)
}

impl App {
    /// Draws the pictures in the rows of a library menu drawn in `rect`, after the menu itself.
    pub(crate) fn draw_library_thumbs(&mut self, sf: &mut Surface<'_>, rect: Rect, page: usize) {
        let s = self.ui.scale;
        self.brush_ui.thumbs.update(&self.brush_ui.store);
        let first = self.ui.menu_first;
        let side = SIDE as i32 * s;
        for n in 0..PAGE {
            let index = page * PAGE + n;
            let Some(cells) = self.brush_ui.thumbs.of(index) else {
                break;
            };
            let Some(row) = shown_row(rect, n, first, s) else {
                continue;
            };
            let (x, y) = (row.x + MENU_PAD * s, row.y + (row.h - side) / 2);
            paint(sf, cells, x, y, s);
            if self.ui.naming() {
                if let Some(brush) = self.brush_ui.store.brushes().get(index) {
                    let name = format!("Picture of {}", brush.name);
                    self.ui.name_rect(Rect::new(x, y, side, side), &name);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "brush_thumbs_tests.rs"]
mod tests;
