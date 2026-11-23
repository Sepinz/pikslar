//! The cel matrix: every layer against the frames of the strip, under it. See
//! docs/cel-matrix.md for the conventions it follows.

use crate::app::App;
use crate::layout::{matrix_row, Layout};
use crate::matrix_drag::{drop_drag, At, Block, Press};
use crate::timeline_geom::Geom;
use crate::workspace::model::Panel;
use pikslar_core::links::Across;
use pikslar_core::CelCmd;
use pikslar_render::{Rect, Surface};
use pikslar_ui::chrome;
use pikslar_ui::icons::ActionIcon;
use pikslar_ui::icons::{self, draw_art, Look as IconLook, MarkIcon, Tones};
use pikslar_ui::role::{Role, State};
use pikslar_ui::theme;
use pikslar_ui::{cap_height, draw_text, scroll_bar};

pub(crate) const ID_MATRIX: u32 = 230_000;
pub(crate) const ID_MATRIX_BAR: u32 = ID_MATRIX + 1;
const ID_FOLD: u32 = ID_MATRIX + 2;
pub(crate) const ID_NAMES: u32 = ID_MATRIX + 3;
pub(crate) const ID_NAMES_SPLIT: u32 = ID_MATRIX + 4;
/// Width of one frame column, as in the strip.
const CELL: i32 = 34;
/// In the names column: where the eye, the lock mark and the name start, in units. The eye
/// keeps a gap from the active row's accent bar.
const MARK_EYE: i32 = 3;
const MARK_LOCK: i32 = 13;
const NAME_X: i32 = 24;
const HINT: &str = "Click a cel to draw there. Shift-click picks cels; drag them, Ctrl copies, \
Alt links, Shift inserts. A notch marks a drawing shared with another layer";

/// Whether the matrix is open, and which rows are in view. View state: never undone or saved.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Matrix {
    pub open: bool,
    first: usize,
    /// The active row and list shape last brought into view.
    followed: Option<(usize, usize, usize)>,
    /// The cels picked to drag.
    pub selection: Option<Block>,
    /// The document's revision, and its count of revisions given back, when they were picked:
    /// any edit since may have moved them.
    picked_at: (u64, u64),
    /// A press on the cels, until the button comes up.
    pub press: Option<Press>,
}

impl Matrix {
    /// The first row to show; the active row is brought into view when it or the list changes.
    pub fn window(&mut self, active: usize, rows: usize, count: usize) -> usize {
        if self.followed != Some((active, rows, count)) {
            let before = self.followed.replace((active, rows, count));
            // A window that only changed size keeps what the wheel chose, unless it hides the
            // row that was in view.
            let seen = |first: usize, rows: usize| (first..first + rows).contains(&active);
            let resized = before.is_some_and(|(a, r, c)| {
                a == active && c == count && (!seen(self.first, r) || seen(self.first, rows))
            });
            if !resized {
                self.first = self
                    .first
                    .min(active)
                    .max((active + 1).saturating_sub(rows));
            }
        }
        self.first = self.first.min(count.saturating_sub(rows));
        self.first
    }

    /// A new document starts from its first rows.
    pub fn reset_view(&mut self) {
        self.first = 0;
        self.followed = None;
    }

    /// A key moved focus along the rows: the current row is brought into view again, even if
    /// the wheel or the bar had scrolled it away.
    pub fn follow_again(&mut self) {
        self.followed = None;
    }

    pub fn scroll(&mut self, ticks: i32) {
        self.first = self.first.saturating_add_signed(-(ticks as isize));
    }
}

/// Which row and column of the cells a point is on, counting on past their edges.
fn cell_at(cells: Rect, (x, y): (i32, i32), s: i32) -> (i64, i64) {
    let col = (x - cells.x).div_euclid(CELL * s);
    let row = (y - cells.y).div_euclid(matrix_row() * s);
    (row as i64, col as i64)
}

impl App {
    /// The layout the window should have now, with matrix rows while the matrix is open. On a
    /// tilemap layer the Color panel compacts before Tiles folds for room.
    pub(crate) fn wanted_layout(&self) -> Layout {
        let rows = match (self.matrix.open, self.asked_rows()) {
            (false, _) => 0,
            (true, Some(rows)) => rows,
            (true, None) => self.editor.doc().layers().len() as i32,
        };
        let (w, h) = self.size;
        let size = (w, h, self.ui.scale);
        let view = self.view_layout();
        let layout = Layout::arranged(&view, size, rows);
        let tiles_folded = |l: &Layout| {
            let effective = l.solved.effective;
            effective.find(Panel::Tiles).is_some_and(|(side, i, _)| {
                let shown = l.solved.area(side).slots().get(i).copied();
                let asked = view.area(side).slots().get(i).copied();
                let tiles = shown.is_some_and(|r| r.panel == Some(Panel::Tiles) && !r.open);
                tiles && asked.is_some_and(|sl| sl.open)
            })
        };
        if self.active_tileset().is_some() && !view.color_compact && tiles_folded(&layout) {
            return Layout::arranged(&view.with_compact(true), size, rows);
        }
        layout
    }

    /// Brings the layout up to date when the matrix opens, folds or gains or loses rows.
    pub(crate) fn sync_matrix_layout(&mut self) {
        let wanted = self.wanted_layout();
        if wanted != self.layout {
            let before = std::mem::replace(&mut self.layout, wanted).canvas;
            self.follow_canvas(before);
            self.full_canvas = true;
            self.ui_dirty = true;
        }
    }

    /// Opens the matrix, its rows following the layers, or folds it to the single line.
    pub(crate) fn toggle_matrix(&mut self) {
        if self.matrix.open && self.asked_rows().is_some() {
            self.size_timeline(crate::dock_keys::LINE);
            return;
        }
        self.matrix.open = !self.matrix.open;
        self.sync_matrix_layout();
    }

    /// The unfold chevron at the right end of the line: shows every layer's cels, or folds
    /// them away. Named "Cels" for scripts.
    pub(crate) fn draw_fold(&mut self, sf: &mut Surface<'_>, r: Rect) {
        let open = self.matrix.open;
        self.note_hint(r, "Show every layer's cels, or fold to one line");
        // Open, a single chevron pointing down to fold the rows away: the two inward chevrons
        // read as a close cross at 1x (final review, defect 15).
        let art = if open {
            pikslar_ui::icons::MarkIcon::ChevronDown.art()
        } else {
            ActionIcon::Expand.art()
        };
        let state = State {
            expanded: Some(open),
            ..State::NONE
        };
        let parent = theme::panel();
        if chrome::art_button(&mut self.ui, sf, (ID_FOLD, r), art, parent, state) {
            self.defer(|a| a.toggle_matrix());
        }
        if self.ui.naming() {
            self.ui.name(ID_FOLD, "Cels");
        }
    }

    /// The wheel over the matrix scrolls its rows. Returns false elsewhere.
    pub(crate) fn matrix_wheel(&mut self, ticks: i32, x: i32, y: i32) -> bool {
        if !self.layout.matrix.contains(x, y) {
            return false;
        }
        self.matrix.scroll(ticks);
        self.ui_dirty = true;
        true
    }

    /// The rows under the strip, whose columns line up with its frames.
    pub(crate) fn draw_matrix(&mut self, sf: &mut Surface<'_>, g: &Geom) {
        let strip = g.strip;
        let area = self.layout.matrix;
        let s = self.ui.scale;
        let rows = self.layout.matrix_rows(s) as usize;
        // A press the matrix can no longer see end must not land later.
        if rows == 0 || !(self.ui.down || self.ui.released) {
            self.matrix.press = None;
        }
        if rows == 0 {
            return;
        }
        let count = self.editor.doc().layers().len();
        if self.matrix.picked_at != self.picked_now() {
            self.matrix.selection = None;
        }
        let first_row = self
            .matrix
            .window(count - 1 - self.editor.layer, rows, count);
        let first = self.timeline.first();
        let visible = (strip.w / (CELL * s)).max(1) as usize;
        let shown = visible.min(self.editor.doc().frames.len().saturating_sub(first));
        let rows_shown = rows.min(count - first_row);
        let cells = Rect::new(
            strip.x,
            area.y,
            shown as i32 * CELL * s,
            rows_shown as i32 * matrix_row() * s,
        );
        self.matrix_input(cells, first_row, first);
        let view = (cells, first_row, first);
        let names = Rect::new(
            g.names.x,
            area.y,
            g.names.w,
            rows_shown as i32 * matrix_row() * s,
        );
        self.names_column(sf, names, first_row);
        // Asked once per repaint, so each cell's notch is a binary search.
        let across = self.editor.doc().links_reaching_across();
        for row in 0..rows_shown {
            self.draw_matrix_row(sf, view, row, names, &across);
        }
        self.draw_matrix_tail(sf, view, (shown, rows, count), g);
    }

    /// What goes over the rows: the picked and landing blocks, the focus ring, the names
    /// splitter and the scroll bar.
    fn draw_matrix_tail(
        &mut self,
        sf: &mut Surface<'_>,
        view: (Rect, usize, usize),
        (shown, rows, count): (usize, usize, usize),
        g: &Geom,
    ) {
        let s = self.ui.scale;
        let area = self.layout.matrix;
        let first_row = view.1;
        if let Some(block) = self.matrix.selection {
            self.outline_block(sf, view, block, theme::accent());
        }
        if let Some(block) = self.landing() {
            self.outline_block(sf, view, block, theme::text());
        }
        self.ring_current_cel(sf, view, shown);
        let gutter = Rect::new(g.gutter.x, area.y, g.gutter.w, area.h);
        self.names_splitter(sf, gutter, (g.names_units, g.names_most));
        // Beside the last column, where the eye is, not at the far end of a wide strip.
        let bar = Rect::new(view.0.right() + 2 * s, area.y, 12 * s, area.h);
        if self.ui.naming() && count > rows {
            self.ui.name(ID_MATRIX_BAR, "Scroll layers");
        }
        if let Some(wanted) =
            scroll_bar(&mut self.ui, sf, ID_MATRIX_BAR, bar, first_row, rows, count)
        {
            self.matrix.first = wanted;
            self.ui_dirty = true;
        }
    }

    /// One row of the matrix in view: the layer's name, its cels and its holds.
    fn draw_matrix_row(
        &mut self,
        sf: &mut Surface<'_>,
        (cells, first_row, first): (Rect, usize, usize),
        row: usize,
        names: Rect,
        across: &Across,
    ) {
        let s = self.ui.scale;
        let index = self.editor.doc().layers().len() - 1 - (first_row + row);
        let y = cells.y + row as i32 * matrix_row() * s;
        let label = Rect::new(names.x, y, names.w, matrix_row() * s);
        self.draw_row_label(sf, label, index);
        let shown = (cells.w / (CELL * s)) as usize;
        for col in 0..shown {
            let x = cells.x + col as i32 * CELL * s;
            let r = Rect::new(x, y, CELL * s - 2 * s, matrix_row() * s - 2 * s);
            let shared = across.contains(self.editor.doc().layers()[index].link(first + col));
            self.draw_matrix_cell(sf, r, (index, first + col), shared);
        }
        let line = Rect::new(cells.x, y, cells.w, matrix_row() * s);
        self.draw_row_holds(sf, line, index, first, shown);
    }

    /// A press picks a cel and a drag from the picked cels carries them.
    fn matrix_input(&mut self, cells: Rect, first_row: usize, first: usize) {
        let state = self.ui.interact(ID_MATRIX, cells);
        self.ui.focus.register_keys(ID_MATRIX);
        // The value is the current frame, counted from 1, for scripts and a screen reader.
        let frame = self.editor.frame as i32 + 1;
        self.ui.describe(Role::ListItem, State::value(frame));
        self.ui.name(ID_MATRIX, "Cel matrix");
        self.note_hint(cells, HINT);
        if let Some(press) = self.ui.focus.take_press(ID_MATRIX) {
            self.matrix_key(press);
        }
        let under = self.cel_under(cells, first_row, first);
        if state.started {
            self.press_cel(under);
        }
        if let Some(press) = self.matrix.press.as_mut().filter(|p| p.over != under) {
            press.over = under;
            self.ui_dirty = true;
        }
        if self.ui.released {
            if let Some(press) = self.matrix.press.take() {
                self.release_cel(press);
            }
        }
    }

    /// The cel under the pointer, kept to the document; its frame may be one past the last.
    fn cel_under(&self, cells: Rect, first_row: usize, first: usize) -> At {
        let (row, col) = cell_at(cells, self.ui.mouse, self.ui.scale);
        let count = self.editor.doc().layers().len() as i64;
        let frames = self.editor.doc().frames.len() as i64;
        let shown = (first_row as i64 + row).clamp(0, count - 1);
        let frame = (first as i64 + col).clamp(0, frames);
        ((count - 1 - shown) as usize, frame as usize)
    }

    fn press_cel(&mut self, at: At) {
        let mods = self.press_mods;
        let picked = self.matrix.selection.is_some_and(|b| b.contains(at));
        if !mods.shift && !picked {
            self.pick(Block::single(at));
        }
        self.matrix.press = Some(Press::new(at, mods));
        self.ui_dirty = true;
    }

    /// The block a drag carries: the picked cels if it began on one of them.
    fn carried(&self, press: &Press) -> Block {
        self.matrix
            .selection
            .filter(|b| b.contains(press.at))
            .unwrap_or(Block::single(press.at))
    }

    /// Where the block being dragged would land now, if it is being dragged.
    fn landing(&self) -> Option<Block> {
        let press = self.matrix.press?;
        let doc = self.editor.doc();
        let drag = drop_drag(
            self.carried(&press),
            &press,
            doc.layers().len(),
            doc.frames.len(),
        );
        drag.as_ref().map(Block::landed)
    }

    /// A click picks a cel and makes it current, a Shift-click picks a block, and a drop lands
    /// the block.
    fn release_cel(&mut self, press: Press) {
        self.ui_dirty = true;
        if !press.moved() {
            let current = (self.editor.layer, self.editor.frame);
            if press.mods.shift {
                self.pick(Block::spanning(current, press.at));
                return;
            }
            self.pick(Block::single(press.at));
            let (layer, frame) = press.at;
            self.defer(move |a| {
                if a.settle() {
                    a.editor.layer = layer.min(a.editor.doc().layers().len() - 1);
                    a.set_frame(frame);
                }
            });
            return;
        }
        let doc = self.editor.doc();
        let (layers, frames) = (doc.layers().len(), doc.frames.len());
        let Some(drag) = drop_drag(self.carried(&press), &press, layers, frames) else {
            return;
        };
        self.defer(move |a| {
            a.playing = false;
            if !a.settle() {
                return;
            }
            if a.run(CelCmd::Drag(drag)).is_some_and(|done| done.changed) {
                a.pick(Block::landed(&drag));
            }
        });
    }

    /// Picks cels as the document is now.
    pub(crate) fn pick(&mut self, block: Block) {
        self.matrix.selection = Some(block);
        self.matrix.picked_at = self.picked_now();
    }

    /// What `picked_at` is compared with: a step that came to nothing gives its revision back.
    fn picked_now(&self) -> (u64, u64) {
        (self.editor.revision(), self.editor.revision_rewinds())
    }

    /// A frame around the cels of a block that are in view.
    fn outline_block(
        &self,
        sf: &mut Surface<'_>,
        (cells, first_row, first): (Rect, usize, usize),
        block: Block,
        ink: u32,
    ) {
        let s = self.ui.scale;
        let count = self.editor.doc().layers().len() as i32;
        let top = count - 1 - block.layers.1 as i32 - first_row as i32;
        let rows = (block.layers.1 - block.layers.0 + 1) as i32;
        let left = block.frames.0 as i32 - first as i32;
        let cols = (block.frames.1 - block.frames.0 + 1) as i32;
        let whole = Rect::new(
            cells.x + left * CELL * s,
            cells.y + top * matrix_row() * s,
            cols * CELL * s,
            rows * matrix_row() * s,
        );
        // Only the part in view, with one column more for a block landing after the last frame.
        let past_end = block.frames.0 >= self.editor.doc().frames.len();
        let extra = if past_end { CELL * s } else { 0 };
        let room = Rect::new(cells.x, cells.y, cells.w + extra, cells.h);
        let r = whole.intersect(room);
        if r.is_empty() {
            return;
        }
        let r = Rect::new(r.x - s, r.y - s, r.w, r.h);
        let edges = [
            Rect::new(r.x, r.y, r.w, s),
            Rect::new(r.x, r.bottom() - s, r.w, s),
            Rect::new(r.x, r.y, s, r.h),
            Rect::new(r.right() - s, r.y, s, r.h),
        ];
        for edge in edges {
            sf.fill(edge, ink);
        }
    }

    /// A layer's row in the names column: an eye mark (crossed when hidden), a lock mark when
    /// locked, then its name. Shape, not colour, tells hidden and locked layers apart.
    fn draw_row_label(&self, sf: &mut Surface<'_>, r: Rect, index: usize) {
        let s = self.ui.scale;
        let ts = self.ui.text_scale();
        let layer = &self.editor.doc().layers()[index];
        let active = index == self.editor.layer;
        let ink = if active {
            theme::text()
        } else {
            theme::muted()
        };
        let tones = Tones::for_look(IconLook::Normal, icons::high_contrast());
        let eye = if layer.visible {
            MarkIcon::Eye
        } else {
            MarkIcon::EyeOff
        };
        draw_art(sf, r.x + MARK_EYE * s, r.y + 4 * s, eye.art(), tones, s);
        if layer.locked {
            draw_art(
                sf,
                r.x + MARK_LOCK * s,
                r.y + 4 * s,
                MarkIcon::Lock.art(),
                tones,
                s,
            );
        }
        if active {
            sf.fill(Rect::new(r.x, r.y, s, r.h - 2 * s), theme::accent());
        }
        let x = r.x + NAME_X * s;
        let name = crate::panel_bottom::fit_to_width(&layer.name, (r.w - NAME_X * s).max(0), ts);
        draw_text(sf, x, r.y + (r.h - cap_height() * ts) / 2, &name, ink, ts);
    }

    fn draw_matrix_cell(
        &mut self,
        sf: &mut Surface<'_>,
        r: Rect,
        (index, frame): (usize, usize),
        shared: bool,
    ) {
        let s = self.ui.scale;
        let current = frame == self.editor.frame;
        let here = current && index == self.editor.layer;
        sf.fill(
            r,
            match (here, current) {
                (true, _) => theme::accent(),
                (false, true) => theme::raised(),
                _ => theme::well(),
            },
        );
        if self.ui.naming() {
            self.ui
                .name_rect(r, &format!("Cel {} {}", index + 1, frame + 1));
        }
        let layer = &self.editor.doc().layers()[index];
        let ink = match (here, layer.visible) {
            (true, _) => theme::on_accent(),
            (false, true) => theme::text(),
            (false, false) => theme::muted(),
        };
        if icons::high_contrast() {
            // Every surface is the window's colour: the cell is a line, the current column two.
            let thick = if current && !here { 2 * s } else { s };
            outline(sf, r, thick, theme::rule());
        }
        let blank = layer.cel(frame).is_blank();
        self.draw_cel_marks(sf, r, (index, frame), (blank, shared), ink);
    }

    /// A cel's marks in `r`: a dot for a drawn cel, a ring for an empty one
    /// (docs/ui-redesign.md 3.5), and a notch when it shares its drawing.
    fn draw_cel_marks(
        &mut self,
        sf: &mut Surface<'_>,
        r: Rect,
        (index, frame): (usize, usize),
        (blank, shared): (bool, bool),
        ink: u32,
    ) {
        let s = self.ui.scale;
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        let mark = Rect::new(cx - 3 * s, cy - 3 * s, 6 * s, 6 * s);
        if blank {
            outline(sf, mark, s, ink);
        } else {
            sf.fill(mark, ink);
        }
        // A notch in the corner: this cel shares its drawing with a cel of another layer.
        if shared {
            let notch = Rect::new(r.right() - 5 * s, r.y + s, 4 * s, 4 * s);
            sf.fill(notch, ink);
            if self.ui.naming() {
                let name = format!("Shared {} {}", index + 1, frame + 1);
                self.ui.name_rect(notch, &name);
            }
        }
    }

    /// A line between neighbouring cels of one layer that show one drawing.
    fn draw_row_holds(
        &self,
        sf: &mut Surface<'_>,
        row: Rect,
        index: usize,
        first: usize,
        shown: usize,
    ) {
        let s = self.ui.scale;
        let cell = CELL * s;
        let y = row.y + (row.h - 2 * s) / 2 - s;
        for frame in first..first + shown.saturating_sub(1) {
            if self.editor.doc().held_into_next(index, frame) {
                let x = row.x + (frame - first) as i32 * cell + (cell - 2 * s) / 2;
                sf.fill(Rect::new(x, y, cell, 2 * s), theme::muted());
            }
        }
    }
}

/// A `thick` outline just inside `r`.
fn outline(sf: &mut Surface<'_>, r: Rect, thick: i32, ink: u32) {
    sf.fill(Rect::new(r.x, r.y, r.w, thick), ink);
    sf.fill(Rect::new(r.x, r.bottom() - thick, r.w, thick), ink);
    sf.fill(Rect::new(r.x, r.y, thick, r.h), ink);
    sf.fill(Rect::new(r.right() - thick, r.y, thick, r.h), ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_active_row_comes_into_view_and_the_wheel_can_look_elsewhere() {
        let mut m = Matrix::default();
        assert_eq!(m.window(0, 3, 10), 0);
        assert_eq!(m.window(8, 3, 10), 6);
        m.scroll(3);
        assert_eq!(
            m.window(8, 3, 10),
            3,
            "the wheel holds until the active row changes"
        );
        assert_eq!(m.window(1, 3, 10), 1);
        m.scroll(-50);
        assert_eq!(m.window(1, 3, 10), 7, "never past the last rows");
        assert_eq!(m.window(1, 3, 2), 0, "a list that fits starts at the top");
    }

    #[test]
    fn one_row_more_than_fit_scrolls_by_exactly_one() {
        let mut m = Matrix::default();
        assert_eq!(m.window(0, 3, 4), 0);
        assert_eq!(m.window(3, 3, 4), 1, "the last row brings the last rows in");
        m.scroll(-9);
        assert_eq!(m.window(3, 3, 4), 1, "never past the last rows");
        m.scroll(9);
        assert_eq!(m.window(3, 3, 4), 0, "the wheel can look at the top");
        m.reset_view();
        assert_eq!(m.window(0, 3, 4), 0, "a new document starts at the top");
    }

    #[test]
    fn a_point_is_found_in_its_row_and_column() {
        let cells = Rect::new(100, 50, 5 * CELL, 3 * matrix_row());
        assert_eq!(cell_at(cells, (100, 50), 1), (0, 0));
        assert_eq!(cell_at(cells, (100 + CELL, 50 + matrix_row()), 1), (1, 1));
        assert_eq!(
            cell_at(cells, (100 + 2 * CELL * 2, 50 + 2 * matrix_row() * 2), 2),
            (2, 2)
        );
        assert_eq!(cell_at(cells, (90, 40), 1), (-1, -1), "past the edges");
    }
}
