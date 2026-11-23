//! Terrain sets in the Tiles section (M11): a row under the header, shown once the tileset has a
//! terrain set, naming the set chosen with a button that opens the terrain menu and a **Slots**
//! button that shows the set's slots in place of the tiles. With a terrain chosen to paint with,
//! the tools in Tiles mode paint it: B paints, E and the right button clear, U fills a rectangle,
//! G a region, Alt-click re-picks one cell. A slot is filled by choosing a tile, then clicking the
//! slot; a missing slot shows the mask it stands for. Menus are in `terrain_menu.rs`; the
//! conventions are in `docs/autotiling.md`. Everything goes through `TerrainCmd`.

use crate::app::App;
use crate::panel_fit::List;
use crate::panel_palette::{outline, Grid};
use crate::panel_tiles::{draw_thumb, per_row, TILE_CELL};
use pikslar_core::autotile::rules::{blob_mask_of_slot, E, N, NE, NW, S, SE, SW, W};
use pikslar_core::autotile::{TerrainKind, TerrainSet};
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{Corners, TerrainCmd, TerrainShape, TerrainTool, TileTool, Tool};
use pikslar_platform::{Button, Mods};
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{button, cap_height, draw_text_edit};

/// The terrain button; `+ 1` the Slots button.
pub(crate) const ID_TERRAIN: u32 = 288;
/// A slot, by its number.
pub(crate) const ID_SLOT: u32 = 130_000;
/// The terrain row and the gap under it, in interface units.
pub(crate) const TERRAIN_ROW: i32 = 26;
const TERRAIN_HINT: &str = "The terrain set. Click to choose one to paint with, or for more";
const SLOTS_HINT: &str = "Show the terrain's slots in place of the tiles";
const SLOT_HINT: &str = "Choose a tile, then click a slot to put it there. Right click for more";

/// View state of the terrain row. Never undone, never saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerrainUi {
    /// The terrain set shown, by its tileset and id.
    pub chosen: Option<(TilesetId, u16)>,
    /// The tools paint the chosen terrain instead of placing tiles.
    pub painting: bool,
    /// The slot view is shown in place of the tiles.
    pub slots: bool,
    /// How a Corner4 set is painted.
    pub corners: Corners,
    /// The terrain row has room this draw.
    pub row_shown: bool,
    /// Where the row and the slots were last drawn, for menus.
    pub row: Rect,
    pub grid: Option<Grid>,
    /// The slot a right click opened the slot menu on.
    pub menu_slot: Option<u8>,
}

/// The kind's name, as the panel and the menus say it.
pub(crate) fn kind_name(kind: TerrainKind) -> &'static str {
    match kind {
        TerrainKind::Edge4 => "Edge4",
        TerrainKind::Corner4 => "Corner4",
        TerrainKind::Blob47 => "Blob47",
    }
}

/// How many of a set's slots have no tile.
pub(crate) fn missing(set: &TerrainSet) -> usize {
    set.kind().slot_count() - set.defined().count_ones() as usize
}

/// The eight-neighbour mask (the rule engine's weights) a slot stands for.
pub(crate) fn slot_mask(kind: TerrainKind, slot: u8) -> u8 {
    let pick = |bits: [u8; 4]| {
        (0..4)
            .filter(|i| slot >> i & 1 == 1)
            .fold(0, |m, i| m | bits[i])
    };
    match kind {
        TerrainKind::Edge4 => pick([N, E, S, W]),
        TerrainKind::Corner4 => pick([NE, SE, SW, NW]),
        TerrainKind::Blob47 => blob_mask_of_slot(slot).unwrap_or(0),
    }
}

/// How many slots a row of the slot view holds: the 4 by 4 table for the 16-slot kinds, as many
/// as fit for Blob47.
pub(crate) fn slots_per_row(kind: TerrainKind, fit: i32) -> i32 {
    match kind {
        TerrainKind::Edge4 | TerrainKind::Corner4 => fit.min(4),
        TerrainKind::Blob47 => fit,
    }
}

/// Draws a missing slot: the mask it stands for as a 3 by 3 diagram, the middle and the set
/// neighbours filled, inside a muted frame.
fn draw_mask(sf: &mut Surface<'_>, mask: u8, r: Rect, scale: i32) {
    const AT: [(i32, i32, u8); 8] = [
        (1, 0, N),
        (2, 0, NE),
        (2, 1, E),
        (2, 2, SE),
        (1, 2, S),
        (0, 2, SW),
        (0, 1, W),
        (0, 0, NW),
    ];
    sf.fill(r, theme::raised());
    outline(sf, r, scale, theme::muted());
    let d = r.w / 4;
    let (x0, y0) = (r.x + (r.w - 3 * d) / 2, r.y + (r.h - 3 * d) / 2);
    let at = |(c, w): (i32, i32)| Rect::new(x0 + c * d, y0 + w * d, d - scale, d - scale);
    sf.fill(at((1, 1)), theme::muted());
    for (c, w, bit) in AT {
        if mask & bit != 0 {
            sf.fill(at((c, w)), theme::muted());
        }
    }
}

impl App {
    /// The terrain set the row shows: the one chosen if it is the active tileset's, else its
    /// first. None without a tilemap layer or a terrain set.
    pub(crate) fn shown_terrain(&self) -> Option<&TerrainSet> {
        let set = self.editor.doc().tileset(self.active_tileset()?).ok()?;
        let chosen = self.tile_ui.terrain.chosen;
        let found = chosen
            .filter(|c| c.0 == set.id())
            .and_then(|c| set.terrain(c.1));
        found.or_else(|| set.terrains().first())
    }

    /// The terrain set the tools paint, if one is chosen to paint with and is still there.
    fn painted_terrain(&self) -> Option<(TilesetId, u16)> {
        let ui = self.tile_ui.terrain;
        let (tileset, set) = ui.chosen.filter(|_| ui.painting)?;
        if self.active_tileset() != Some(tileset) {
            return None;
        }
        self.editor.doc().terrain(tileset, set).ok()?;
        Some((tileset, set))
    }

    /// The terrain tool a press is, while a terrain is chosen to paint with. None for the picker,
    /// which picks a tile as ever.
    pub(crate) fn terrain_tool(
        &self,
        chosen: Tool,
        button: Button,
        mods: Mods,
    ) -> Option<TileTool> {
        let (tileset, set) = self.painted_terrain()?;
        let (shape, clear) = match (button, chosen) {
            (Button::Right, _) => (TerrainShape::Brush, true),
            _ if mods.alt => (TerrainShape::Retile, false),
            (_, Tool::Picker) => return None,
            (_, Tool::Eraser) => (TerrainShape::Brush, true),
            (_, Tool::Rect) => (TerrainShape::Rect, false),
            (_, Tool::Fill) => (TerrainShape::Fill, false),
            (_, Tool::Pencil | Tool::Line) => (TerrainShape::Brush, false),
        };
        let corners = self.tile_ui.terrain.corners;
        Some(TileTool::Terrain(TerrainTool {
            tileset,
            set,
            shape,
            clear,
            corners,
        }))
    }

    /// The Tiles section's header height: the tileset row, and the terrain row when the tileset
    /// has a terrain set and the panel has room for it.
    pub(crate) fn tiles_head(&self) -> i32 {
        let row = self.tile_ui.terrain.row_shown;
        crate::panel_tiles::HEAD + if row { TERRAIN_ROW } else { 0 }
    }

    /// Draws the terrain row from `top`, if the tileset has a terrain set and it has room.
    pub(crate) fn draw_terrain_row(&mut self, sf: &mut Surface<'_>, area: Rect, top: i32) {
        let found = self
            .shown_terrain()
            .filter(|_| self.tile_ui.terrain.row_shown);
        let Some(found) = found else {
            self.tile_ui.terrain.row = Rect::default();
            return;
        };
        let id = found.id();
        let label = found.name().to_string();
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let size = 22 * s;
        let wide = pikslar_ui::text_width("Slots", ts) + 10 * s;
        let slots = Rect::new(area.right() - wide, top, wide, size);
        let row = Rect::new(area.x, top, slots.x - area.x - 2 * s, size);
        self.tile_ui.terrain.row = row;
        self.note_hint(row, TERRAIN_HINT);
        self.note_hint(slots, SLOTS_HINT);
        let naming = self
            .tile_ui
            .naming
            .as_ref()
            .filter(|n| n.terrain == Some(id));
        if let Some(naming) = naming {
            sf.fill(row, theme::raised());
            let y = top + (size - cap_height() * ts) / 2;
            let at = (row.x + 2 * s, y);
            draw_text_edit(
                sf,
                at,
                row.w - 4 * s,
                &naming.edit,
                (theme::text(), theme::accent()),
                ts,
            );
        } else {
            let shown = crate::panel_bottom::fit_to_width(&label, row.w - 10 * s, ts);
            let on = self.painted_terrain().is_some();
            if button(&mut self.ui, sf, ID_TERRAIN, row, &shown, on) {
                let at = (row.x, row.bottom());
                self.defer(move |a| a.open_terrain_menu(at));
            }
        }
        let on = self.tile_ui.terrain.slots;
        if button(&mut self.ui, sf, ID_TERRAIN + 1, slots, "Slots", on) {
            self.defer(|a| a.toggle_slots());
        }
    }

    /// True while the slot view takes the tiles' place.
    pub(crate) fn showing_slots(&self) -> bool {
        self.tile_ui.terrain.slots && self.shown_terrain().is_some()
    }

    /// The rows the slot view wants in a panel `fit` slots wide, 0 while it is not shown.
    pub(crate) fn slot_rows(&self, fit: usize) -> usize {
        match self.shown_terrain().filter(|_| self.tile_ui.terrain.slots) {
            Some(t) => {
                let per_row = slots_per_row(t.kind(), fit as i32).max(1) as usize;
                t.kind().slot_count().div_ceil(per_row)
            }
            None => 0,
        }
    }

    fn toggle_slots(&mut self) {
        self.tile_ui.terrain.slots = !self.tile_ui.terrain.slots;
        self.ui_dirty = true;
    }

    /// Draws the shown set's slots in `list`, `rows` rows of them.
    pub(crate) fn draw_slots(&mut self, sf: &mut Surface<'_>, list: Rect, rows: i32) {
        let Some(found) = self.shown_terrain() else {
            return;
        };
        let (kind, slots) = (found.kind(), found.slots().to_vec());
        let s = self.ui.scale;
        let per_row = slots_per_row(kind, per_row(list.w, s));
        let total = slots.len().div_ceil(per_row as usize).max(1);
        let first = self.panel_scroll.tiles_window(list, rows as usize, total);
        self.draw_scroll_bar(sf, List::Tiles, list, (first, rows as usize, total));
        let grid = Grid {
            x: list.x,
            top: list.y,
            cell: TILE_CELL * s,
            per_row,
            first,
            rows,
            len: slots.len(),
        };
        self.tile_ui.terrain.grid = Some(grid);
        self.note_hint(list, SLOT_HINT);
        for (slot, tile) in slots.into_iter().enumerate() {
            if let Some(r) = grid.cell_rect(slot) {
                self.draw_slot(sf, (kind, slot as u8, tile), r);
            }
        }
    }

    fn draw_slot(&mut self, sf: &mut Surface<'_>, (kind, slot, tile): SlotAt, r: Rect) {
        let state = self.ui.interact(ID_SLOT + u32::from(slot), r);
        let tileset = self.active_tileset();
        let doc = self.editor.doc();
        let set = tileset.and_then(|t| doc.tileset(t).ok());
        match (set, tile.and_then(|t| set?.tile(t))) {
            (Some(set), Some(entry)) => draw_thumb(sf, doc, set, &entry.image, r),
            _ => draw_mask(sf, slot_mask(kind, slot), r, self.ui.scale),
        }
        if state.hover {
            outline(sf, r, self.ui.scale * 2, theme::text());
        }
        if state.clicked {
            self.defer(move |a| a.put_in_slot(slot));
        }
    }

    /// Puts the chosen tile in a slot of the shown set, one step.
    pub(crate) fn put_in_slot(&mut self, slot: u8) {
        let Some((tileset, set)) = self.shown_terrain_id() else {
            return;
        };
        let current = self.editor.tiles().current();
        match current.filter(|c| c.0 == tileset) {
            Some((_, tile, _)) => {
                self.run(TerrainCmd::Assign {
                    tileset,
                    set,
                    slot,
                    tile,
                });
            }
            None => self.notify("Choose a tile first, then click a slot".to_string()),
        }
        self.ui_dirty = true;
    }

    /// The shown set, by its tileset and id.
    pub(crate) fn shown_terrain_id(&self) -> Option<(TilesetId, u16)> {
        Some((self.active_tileset()?, self.shown_terrain()?.id()))
    }

    /// The slot whose cell is at the point, in the slot view.
    pub(crate) fn slot_at(&self, x: i32, y: i32) -> Option<u8> {
        let grid = self.tile_ui.terrain.grid?;
        let index = grid.target((x, y))?;
        grid.cell_rect(index).filter(|r| r.contains(x, y))?;
        u8::try_from(index).ok()
    }
}

/// A slot as it is drawn: the set's kind, its number and its tile.
type SlotAt = (TerrainKind, u8, Option<TileId>);

#[cfg(test)]
#[path = "terrain_ui_tests.rs"]
mod tests;
