//! The palette section of the right panel: the swatches, which are dragged to reorder, and the
//! tools above them. Everything else about the palette is in the menu a right click on the
//! header opens (`palette_menu.rs`).

use crate::app::App;
use crate::color_ui::to_screen;
use crate::palette_naming::GroupNaming;
use crate::panel_fit::{List, PALETTE_ROW};
use crate::panel_right::Section;
use pikslar_core::PaletteCmd;
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{button, draw_text_edit, fold::ARROW_ROOM, icon_button, icon_button_if, Icon};

const ID_PALETTE: u32 = 220;
const ID_SWATCH: u32 = 1_000;

/// View state of the palette section. Never undone, never saved.
#[derive(Debug, Default)]
pub(crate) struct PaletteUi {
    /// The swatch clicked before the selected one: the start of a ramp, or the colour that
    /// "replace a color" replaces.
    pub previous: Option<u32>,
    /// The group whose members are marked on the swatches.
    pub marked: Option<usize>,
    /// The group whose name is being typed.
    pub naming: Option<GroupNaming>,
    /// Where the header was last drawn, for its menu.
    pub header: Rect,
}

/// Where the swatches are on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Grid {
    pub x: i32,
    pub top: i32,
    pub cell: i32,
    pub per_row: i32,
    /// The first row in view.
    pub first: usize,
    pub rows: i32,
    pub len: usize,
}

impl Grid {
    /// The cell of swatch `index`, if it is in view.
    pub fn cell_rect(&self, index: usize) -> Option<Rect> {
        let per_row = self.per_row as usize;
        let row = (index / per_row).checked_sub(self.first)?;
        if row as i32 >= self.rows {
            return None;
        }
        let (col, row) = ((index % per_row) as i32, row as i32);
        let side = self.cell - self.cell / 11;
        Some(Rect::new(
            self.x + col * self.cell,
            self.top + row * self.cell,
            side,
            side,
        ))
    }

    /// The swatch a drop at the point lands on, if it is over the grid. Past the last swatch is
    /// the last swatch.
    pub fn target(&self, (x, y): (i32, i32)) -> Option<usize> {
        let (dx, dy) = (x - self.x, y - self.top);
        if dx < 0 || dy < 0 || dy >= self.rows * self.cell || dx >= self.per_row * self.cell {
            return None;
        }
        let row = self.first + (dy / self.cell) as usize;
        let index = row * self.per_row as usize + (dx / self.cell) as usize;
        Some(index.min(self.len.checked_sub(1)?))
    }
}

pub(crate) fn outline(sf: &mut Surface<'_>, r: Rect, t: i32, color: u32) {
    sf.fill(Rect::new(r.x, r.y, r.w, t), color);
    sf.fill(Rect::new(r.x, r.bottom() - t, r.w, t), color);
    sf.fill(Rect::new(r.x, r.y, t, r.h), color);
    sf.fill(Rect::new(r.right() - t, r.y, t, r.h), color);
}

impl App {
    /// Selects a swatch, remembering the one it replaces as the earlier of the pair.
    pub(crate) fn select_swatch(&mut self, id: u32) {
        if self.selected_swatch != Some(id) {
            self.palette_ui.previous = self.selected_swatch;
        }
        self.selected_swatch = Some(id);
        self.ui_dirty = true;
    }

    /// The Palette folded to its strip draws nothing: no swatch is left to the sliders, and no
    /// header or list answers a click or the wheel.
    pub(crate) fn release_folded_palette(&mut self) {
        self.selected_swatch = None;
        self.palette_ui.header = Rect::default();
        self.panel_scroll.hide(List::Palette);
    }

    /// The Palette panel in `body`: its header, and as many rows of swatches as fit under it
    /// unless folded.
    pub(crate) fn draw_palette(&mut self, sf: &mut Surface<'_>, body: Rect, folded: bool) {
        let s = self.ui.scale;
        let (area, top) = (body, body.y);
        let per_row = (area.w / (PALETTE_ROW * s)).max(1);
        let tools_left = area.right() - 22 * s * 2 - 62 * s;
        // One unit short of Live: with the title's own three units that parts them by four,
        // and in Large text it is what lets "Palette" show beside the tools.
        let header = Rect::new(area.x, top, tools_left - area.x - s, 22 * s);
        self.palette_ui.header = header;
        self.section_header(sf, Section::Palette, header, folded);
        self.draw_group_name(sf, header);
        if folded {
            // A swatch nobody can see must not be recolored by the sliders.
            self.selected_swatch = None;
            self.panel_scroll.hide(List::Palette);
            return;
        }
        self.draw_palette_tools(sf, area, top);
        let swatches: Vec<u32> = self.editor.doc().colors.swatches().to_vec();
        let cell = PALETTE_ROW * s;
        let total = ((swatches.len() as i32 + per_row - 1) / per_row).max(1) as usize;
        let grid_top = top + 26 * s;
        let rows = ((body.h - 26 * s) / cell).clamp(0, total as i32);
        let list = Rect::new(area.x, grid_top, area.w, rows * cell);
        let selected = self
            .selected_swatch
            .and_then(|id| swatches.iter().position(|&s| s == id))
            .map(|i| i / per_row as usize);
        let first = self
            .panel_scroll
            .palette_window(list, rows as usize, total, selected);
        self.draw_scroll_bar(sf, List::Palette, list, (first, rows as usize, total));
        let grid = Grid {
            x: area.x,
            top: grid_top,
            cell,
            per_row,
            first,
            rows,
            len: swatches.len(),
        };
        self.draw_swatches(sf, &swatches, grid);
    }

    fn draw_swatches(&mut self, sf: &mut Surface<'_>, swatches: &[u32], grid: Grid) {
        let marked = self.marked_ids();
        let per_row = grid.per_row as usize;
        let shown = swatches
            .iter()
            .enumerate()
            .skip(grid.first * per_row)
            .take(grid.rows as usize * per_row);
        for (index, &id) in shown {
            if let Some(r) = grid.cell_rect(index) {
                self.draw_swatch(sf, id, r, marked.contains(&id));
            }
        }
        self.draw_swatch_drag(sf, swatches, grid);
    }

    /// While a swatch is held and the pointer is over another cell, that cell is marked as the
    /// drop place. Letting go there moves the swatch: order only, no id and no pixel.
    fn draw_swatch_drag(&mut self, sf: &mut Surface<'_>, swatches: &[u32], grid: Grid) {
        let active = self.ui.active;
        let from = swatches.iter().position(|&id| ID_SWATCH + id == active);
        let Some(from) = from.filter(|_| self.ui.down || self.ui.released) else {
            return;
        };
        let to = grid.target(self.ui.mouse).filter(|&to| to != from);
        let Some(to) = to else {
            return;
        };
        if let Some(r) = grid.cell_rect(to) {
            outline(sf, r, self.ui.scale * 2, theme::accent());
        }
        if self.ui.released {
            self.defer(move |a| {
                a.run(PaletteCmd::MoveSwatch { from, to });
            });
        }
    }

    fn draw_swatch(&mut self, sf: &mut Surface<'_>, id: u32, r: Rect, marked: bool) {
        let state = self.ui.interact(ID_SWATCH + id, r);
        if self.ui.naming() {
            self.ui.name(ID_SWATCH + id, &format!("Swatch {}", id + 1));
        }
        let color = self.editor.doc().colors.color(id);
        sf.fill(r, to_screen(color));
        pikslar_ui::chrome::colour_outline(sf, r, self.ui.scale);
        let chosen = self.selected_swatch == Some(id);
        if chosen || state.hover {
            let edge = if chosen {
                theme::accent()
            } else {
                theme::text()
            };
            outline(sf, r, self.ui.scale * 2, edge);
        }
        if marked {
            let (dot, s) = (6 * self.ui.scale, self.ui.scale);
            let at = Rect::new(r.right() - dot - 2 * s, r.bottom() - dot - 2 * s, dot, dot);
            sf.fill(at, theme::text());
            sf.fill(at.inset(s), theme::accent());
        }
        if state.clicked {
            self.editor.color = color;
            self.select_swatch(id);
        }
    }

    /// The swatches of the group whose members are marked.
    fn marked_ids(&self) -> Vec<u32> {
        let groups = self.editor.doc().colors.groups();
        self.palette_ui
            .marked
            .and_then(|g| groups.get(g))
            .map(|g| g.ids.clone())
            .unwrap_or_default()
    }

    /// While a group's name is typed, the header shows the name in place of the title.
    fn draw_group_name(&self, sf: &mut Surface<'_>, header: Rect) {
        let Some(naming) = &self.palette_ui.naming else {
            return;
        };
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let x = header.x + ARROW_ROOM * s;
        sf.fill(
            Rect::new(x, header.y, header.right() - x, header.h),
            theme::raised(),
        );
        let at = (
            x + 2 * s,
            header.y + (header.h - pikslar_ui::cap_height() * ts) / 2,
        );
        let room = header.right() - at.0 - 2 * s;
        draw_text_edit(
            sf,
            at,
            room,
            &naming.edit,
            (theme::text(), theme::accent()),
            ts,
        );
    }

    fn draw_palette_tools(&mut self, sf: &mut Surface<'_>, area: Rect, top: i32) {
        let s = self.ui.scale;
        let size = 22 * s;
        let live = Rect::new(area.right() - size * 2 - 62 * s, top, 58 * s, size);
        self.note_hint(live, "Live: editing a swatch recolors the art");
        if button(
            &mut self.ui,
            sf,
            ID_PALETTE,
            live,
            "Live",
            self.live_palette,
        ) {
            self.live_palette = !self.live_palette;
            self.ui_dirty = true;
        }
        let add = Rect::new(area.right() - size * 2 - 2 * s, top, size, size);
        self.note_hint(add, "Add this color to the palette");
        if icon_button(&mut self.ui, sf, ID_PALETTE + 1, add, Icon::Plus, false) {
            let color = self.editor.color;
            self.defer(move |a| {
                if let Some(outcome) = a.run(PaletteCmd::AddSwatch { color }) {
                    match outcome.created {
                        Some(id) => a.select_swatch(id),
                        None => a.selected_swatch = None,
                    }
                }
            });
        }
        let remove = Rect::new(area.right() - size, top, size, size);
        self.note_hint(remove, "Remove the selected swatch");
        let usable = self.selected_swatch.is_some();
        let id = ID_PALETTE + 2;
        if icon_button_if(usable, &mut self.ui, sf, id, remove, Icon::Minus) {
            if let Some(id) = self.selected_swatch.take() {
                self.defer(move |a| {
                    a.run(PaletteCmd::RemoveSwatch { id });
                });
            }
        }
    }
}

#[cfg(test)]
#[path = "panel_palette_tests.rs"]
mod tests;
