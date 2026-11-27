//! The Tiles section of the right panel, shown only while a tilemap layer is active: the
//! tileset's tiles at a whole-number zoom, the Tiles and Pixels mode buttons, and the tileset's
//! name. Click a tile to place it, Shift-click another for the block between them, drag one to
//! reorder. The menus a right click opens are in `tile_menu.rs`. See `docs/tilemaps.md`.

use crate::app::App;
use crate::color_ui::to_screen;
use crate::panel_fit::List;
use crate::panel_palette::{outline, Grid};
use crate::terrain_ui::TERRAIN_ROW;
use crate::tile_naming::TilesetNaming;
use pikslar_core::tileset::{TileId, Tileset, TilesetId};
use pikslar_core::{ColorMode, Document, Image, TileCmd};
use pikslar_render::{Rect, Surface};
use pikslar_ui::theme;
use pikslar_ui::{cap_height, draw_text, draw_text_edit};
use std::sync::Arc;

/// The mode buttons; the scroll bar is `ID_SCROLL` with `List::Tiles`.
pub(crate) const ID_TILES: u32 = 285;
/// A thumbnail, by its place in the panel.
pub(crate) const ID_TILE: u32 = 120_000;
/// A thumbnail's cell, in interface units.
pub(crate) const TILE_CELL: i32 = 34;
/// The header row and the gap under it.
pub(crate) const HEAD: i32 = 26;
/// More rows than this scroll.
const MOST_ROWS: i32 = 3;
/// The button in the empty panel that adds a tilemap layer.
pub(crate) const ID_ADD_TILEMAP: u32 = ID_TILES + 2;
/// The lines of message the empty panel keeps room for above its button: "No tileset yet"
/// fits on one line in the narrowest dock in both faces, so the button costs the panels beside
/// it no height.
const EMPTY_LINES: i32 = 1;
/// The empty panel's button, from the longest label to the shortest; the first that fits.
const ADD_LABELS: [&str; 3] = ["Add tilemap layer...", "Add tilemap...", "Add..."];

/// The height in units of the empty panel's button: a row of 22, or the Large text letters
/// with room above and below them.
fn add_button_h() -> i32 {
    22.max(empty_ink() + 8)
}

/// The height in units of one line of the empty panel's letters, descenders included.
fn empty_ink() -> i32 {
    use pikslar_ui::font::{cap_height, face, text_scale, Face};
    let below = match face() {
        Face::Mono => 1,
        Face::Text => 2,
    };
    (cap_height() + below) * text_scale(1)
}

/// The distance in units between the empty panel's lines: the usual 16 in Pikslar Text, and in
/// Large text the doubled letters with four units between them, so the three lines fit the room
/// the panel always had and take none from the Palette beside it.
fn empty_pitch() -> i32 {
    16.max(empty_ink() + 4)
}

/// The panel body's least height in units: its header and one row of tiles, or the empty
/// panel's wrapped messages in the face in use, whichever is taller, so that Large text never
/// cuts them off.
pub(crate) fn body_least() -> i32 {
    let empty = 4 + EMPTY_LINES * empty_pitch() + add_button_h();
    (HEAD + TILE_CELL).max(empty)
}

/// What the pointer does on a tilemap layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TileMode {
    /// The tools place, clear, fill and pick cells.
    #[default]
    Tiles,
    /// The tools draw on the tiles the cells place.
    Pixels,
}

/// View state of the Tiles section. Never undone, never saved.
#[derive(Debug, Default)]
pub(crate) struct TileUi {
    pub mode: TileMode,
    /// The panel places of the tile clicked last and of the Shift-click that made a block.
    pub block: Option<(usize, usize)>,
    /// Where the header, the thumbnails and the New layer button were last drawn, for menus.
    pub header: Rect,
    pub grid: Option<Grid>,
    pub list: Rect,
    pub new_layer: Rect,
    /// The tileset whose name is being typed.
    pub naming: Option<TilesetNaming>,
    /// A map stroke is in progress, so moves continue it.
    pub map_stroke: bool,
    /// The tile a right click opened the tile menu on.
    pub menu_tile: Option<TileId>,
    /// The grid last drawn over the canvas, so a change redraws it.
    pub drawn_grid: Option<GridKey>,
    /// The terrain row and the slot view.
    pub terrain: crate::terrain_ui::TerrainUi,
}

/// A drawn grid: the layer, its cell size and its origin.
pub(crate) type GridKey = (usize, (usize, usize), (i16, i16));

/// How many thumbnails a row of `width` pixels holds.
pub(crate) fn per_row(width: i32, scale: i32) -> i32 {
    (width / (TILE_CELL * scale)).max(1)
}

/// The panel places in the block between places `a` and `b` of a grid `per_row` wide, row by row.
pub(crate) fn block_places(per_row: usize, a: usize, b: usize) -> (usize, usize, Vec<usize>) {
    let per_row = per_row.max(1);
    let (ac, ar, bc, br) = (a % per_row, a / per_row, b % per_row, b / per_row);
    let (left, right, top, bottom) = (ac.min(bc), ac.max(bc), ar.min(br), ar.max(br));
    let places = (top..=bottom)
        .flat_map(|r| (left..=right).map(move |c| r * per_row + c))
        .collect();
    (right - left + 1, bottom - top + 1, places)
}

/// The colour a tile's pixel shows, or None where it is clear.
pub(crate) fn shown_color(doc: &Document, mode: ColorMode, raw: u32) -> Option<u32> {
    let color = match mode {
        ColorMode::Palette => doc.colors.color(raw),
        ColorMode::Rgba => raw,
    };
    (color >> 24 != 0).then_some(color)
}

/// The zoom a tile of `size` is drawn at in `room` pixels, and which pixels are drawn: every
/// one at a whole-number zoom, or every `step`th when the tile is larger than the room.
pub(crate) fn thumb_zoom((w, h): (usize, usize), room: i32) -> (i32, i32) {
    let big = w.max(h).max(1) as i32;
    if big <= room {
        (room / big, 1)
    } else {
        (1, (big + room - 1) / room.max(1))
    }
}

/// Draws a tile's image in `r`, centred, at a whole-number zoom.
pub(crate) fn draw_thumb(
    sf: &mut Surface<'_>,
    doc: &Document,
    set: &Tileset,
    image: &Image,
    r: Rect,
) {
    sf.fill(r, theme::raised());
    let (tw, th) = set.tile_size();
    let room = r.w - 4;
    let (zoom, step) = thumb_zoom((tw, th), room);
    let (cols, rows) = (tw as i32 / step, th as i32 / step);
    let x0 = r.x + (r.w - cols * zoom) / 2;
    let y0 = r.y + (r.h - rows * zoom) / 2;
    for ty in 0..rows {
        for tx in 0..cols {
            let raw = image.get(tx * step, ty * step);
            if let Some(color) = shown_color(doc, set.mode(), raw) {
                let px = Rect::new(x0 + tx * zoom, y0 + ty * zoom, zoom, zoom);
                sf.fill(px, to_screen(color));
            }
        }
    }
}

impl App {
    /// The tileset of the active layer, if it is a tilemap layer.
    pub(crate) fn active_tileset(&self) -> Option<TilesetId> {
        self.editor.doc().layers().get(self.editor.layer)?.tileset()
    }

    /// True while the active layer is a tilemap layer in Tiles mode.
    pub(crate) fn placing_tiles(&self) -> bool {
        self.active_tileset().is_some() && self.tile_ui.mode == TileMode::Tiles
    }

    /// The body height the Tiles panel asks for in a body `width` pixels wide, in units: its
    /// header, the terrain row when the tileset has terrain sets, and up to three rows.
    pub(crate) fn tiles_wanted(&self, width: i32) -> Option<i32> {
        let set = self.editor.doc().tileset(self.active_tileset()?).ok()?;
        let per_row = per_row(width, self.ui.scale) as usize;
        let total = set.len().div_ceil(per_row);
        let rows = total
            .max(self.slot_rows(per_row))
            .clamp(1, MOST_ROWS as usize) as i32;
        let terrain = if set.terrains().is_empty() {
            0
        } else {
            TERRAIN_ROW
        };
        Some(HEAD + terrain + rows * TILE_CELL)
    }

    /// The Tiles panel in `body`: its header and terrain row, then as many rows of tiles as fit.
    /// The empty panel: what is missing, wrapped to the panel's width rather than cut short,
    /// and a button that opens the same New tilemap menu as a right click on the Layers +
    /// button, so the way to a tileset is in plain sight.
    fn draw_no_tileset(&mut self, sf: &mut Surface<'_>, body: Rect) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let line = empty_pitch() * s;
        let mut y = body.y + 4 * s;
        for part in pikslar_ui::wrap("No tileset yet", body.w, ts) {
            let shown = crate::panel_bottom::fit_to_width(&part, body.w, ts);
            draw_text(sf, body.x, y, &shown, theme::text(), ts);
            y += line;
        }
        let pad = 16 * s;
        let Some(label) = ADD_LABELS
            .iter()
            .find(|l| pikslar_ui::text_width(l, ts) + pad <= body.w)
        else {
            return;
        };
        let w = pikslar_ui::text_width(label, ts) + pad;
        let r = Rect::new(body.x, y, w, add_button_h() * s);
        if r.bottom() > body.bottom() {
            return;
        }
        self.note_hint(r, "Add a tilemap layer: choose its tile size");
        if pikslar_ui::button(&mut self.ui, sf, ID_ADD_TILEMAP, r, label, false) {
            let at = (r.x, r.bottom());
            self.defer(move |a| {
                a.open_palette_kind(at, crate::frame_menu::MenuKind::NewTilemap);
            });
        }
    }

    pub(crate) fn draw_tiles(&mut self, sf: &mut Surface<'_>, body: Rect) {
        let Some(set_id) = self.active_tileset() else {
            self.panel_scroll.hide(List::Tiles);
            self.draw_no_tileset(sf, body);
            return;
        };
        let s = self.ui.scale;
        let (area, top) = (body, body.y);
        // The terrain row shows when a row of tiles still fits under it.
        let terrain = self
            .editor
            .doc()
            .tileset(set_id)
            .is_ok_and(|t| !t.terrains().is_empty());
        self.tile_ui.terrain.row_shown = terrain && body.h / s >= HEAD + TERRAIN_ROW + TILE_CELL;
        let rows = ((body.h / s - self.tiles_head()) / TILE_CELL).max(0);
        self.draw_tiles_header(sf, area, top, set_id);
        self.draw_terrain_row(sf, area, top + HEAD * s);
        let (grid_top, cell) = (top + self.tiles_head() * s, TILE_CELL * s);
        let list = Rect::new(area.x, grid_top, area.w, rows * cell);
        self.tile_ui.list = list;
        if self.showing_slots() {
            self.tile_ui.grid = None;
            self.draw_slots(sf, list, rows);
        } else {
            self.tile_ui.terrain.grid = None;
            self.draw_tile_list(sf, set_id, list, rows);
        }
    }

    /// The thumbnails in `list`, `rows` rows of them, with their scroll bar.
    fn draw_tile_list(&mut self, sf: &mut Surface<'_>, set_id: TilesetId, list: Rect, rows: i32) {
        let (s, area, grid_top) = (self.ui.scale, list, list.y);
        let cell = TILE_CELL * s;
        let per_row = per_row(area.w, s);
        let tiles: Vec<(TileId, Arc<Image>)> = match self.editor.doc().tileset(set_id) {
            Ok(set) => set
                .tiles()
                .iter()
                .map(|t| (t.id, t.image.clone()))
                .collect(),
            Err(_) => Vec::new(),
        };
        let total = tiles.len().div_ceil(per_row as usize).max(1);
        let first = self.panel_scroll.tiles_window(list, rows as usize, total);
        self.draw_scroll_bar(sf, List::Tiles, list, (first, rows as usize, total));
        let grid = Grid {
            x: area.x,
            top: grid_top,
            cell,
            per_row,
            first,
            rows,
            len: tiles.len(),
        };
        self.tile_ui.grid = Some(grid);
        if tiles.is_empty() {
            let ts = self.ui.text_scale();
            let y = grid_top + (cell - cap_height() * ts) / 2;
            draw_text(
                sf,
                area.x,
                y,
                "No tiles. Right click: New tile",
                theme::muted(),
                ts,
            );
        }
        self.note_hint(
            list,
            "Click a tile to place it, Shift click a block. Drag to reorder. Right click for more",
        );
        self.draw_thumbs(sf, set_id, &tiles, grid);
    }

    /// The tileset's name, and the two mode buttons at the right.
    fn draw_tiles_header(&mut self, sf: &mut Surface<'_>, area: Rect, top: i32, set: TilesetId) {
        let (s, ts) = (self.ui.scale, self.ui.text_scale());
        let size = 22 * s;
        let header = Rect::new(area.x, top, area.w, size);
        self.tile_ui.header = header;
        self.note_hint(header, "The tileset. Right click for the tileset menu");
        let y = top + (size - cap_height() * ts) / 2;
        if let Some(naming) = self.tile_ui.naming.as_ref().filter(|n| n.terrain.is_none()) {
            sf.fill(header, theme::raised());
            let room = header.w - 4 * s;
            draw_text_edit(
                sf,
                (header.x + 2 * s, y),
                room,
                &naming.edit,
                (theme::text(), theme::accent()),
                ts,
            );
        } else if let Ok(found) = self.editor.doc().tileset(set) {
            let (w, h) = found.tile_size();
            let label = format!("{} {w}x{h}", found.name());
            let shown = crate::panel_bottom::fit_to_width(&label, header.w, ts);
            draw_text(sf, header.x, y, &shown, theme::text(), ts);
        }
    }

    fn draw_thumbs(
        &mut self,
        sf: &mut Surface<'_>,
        set_id: TilesetId,
        tiles: &[(TileId, Arc<Image>)],
        grid: Grid,
    ) {
        let chosen = self.chosen_places(set_id, tiles);
        let shift = self.ui.shift;
        let per_row = grid.per_row as usize;
        let shown = (grid.first * per_row)..(grid.first * per_row + grid.rows as usize * per_row);
        for index in shown.filter(|&i| i < tiles.len()) {
            let Some(r) = grid.cell_rect(index) else {
                continue;
            };
            let id = ID_TILE + index as u32;
            let state = self.ui.interact(id, r);
            if self.ui.naming() {
                self.ui.name(id, &format!("Tile {}", index + 1));
            }
            if let Ok(set) = self.editor.doc().tileset(set_id) {
                draw_thumb(sf, self.editor.doc(), set, &tiles[index].1, r);
            }
            if chosen.contains(&index) || state.hover {
                let edge = if chosen.contains(&index) {
                    theme::accent()
                } else {
                    theme::text()
                };
                outline(sf, r, self.ui.scale * 2, edge);
            }
            if state.clicked {
                self.defer(move |a| a.choose_tile(set_id, index, shift));
            }
        }
        self.draw_tile_drag(sf, set_id, tiles, grid);
    }

    /// While a thumbnail is held over another place, that place is marked; letting go there
    /// moves the tile in the panel. Its id, and so the map, stay as they are.
    fn draw_tile_drag(
        &mut self,
        sf: &mut Surface<'_>,
        tileset: TilesetId,
        tiles: &[(TileId, Arc<Image>)],
        grid: Grid,
    ) {
        let active = self.ui.active;
        let from = active.checked_sub(ID_TILE).map(|i| i as usize);
        let held = self.ui.down || self.ui.released;
        let Some(from) = from.filter(|&i| i < tiles.len() && held) else {
            return;
        };
        let Some(to) = grid.target(self.ui.mouse).filter(|&to| to != from) else {
            return;
        };
        if let Some(r) = grid.cell_rect(to) {
            outline(sf, r, self.ui.scale * 2, theme::accent());
        }
        if self.ui.released {
            let tile = tiles[from].0;
            self.defer(move |a| {
                a.tile_ui.block = None;
                a.run(TileCmd::MoveTile { tileset, tile, to });
            });
        }
    }

    /// The panel places the stamp holds: the tile chosen, or the block chosen by Shift-click.
    fn chosen_places(&self, set_id: TilesetId, tiles: &[(TileId, Arc<Image>)]) -> Vec<usize> {
        let state = self.editor.tiles();
        if let Some((set, tile, _)) = state.current() {
            let place = tiles.iter().position(|t| t.0 == tile);
            return place.filter(|_| set == set_id).into_iter().collect();
        }
        let ours = state.stamp().is_some_and(|s| s.tileset() == set_id);
        match (self.tile_ui.block, self.tile_ui.grid) {
            (Some((a, b)), Some(grid)) if ours => block_places(grid.per_row as usize, a, b).2,
            _ => Vec::new(),
        }
    }

    /// Chooses the tile at panel place `index`, or with Shift the block from the last tile
    /// clicked to it.
    pub(crate) fn choose_tile(&mut self, set: TilesetId, index: usize, shift: bool) {
        let Ok(found) = self.editor.doc().tileset(set) else {
            return;
        };
        let ids: Vec<TileId> = found.tiles().iter().map(|t| t.id).collect();
        let per_row = self.tile_ui.grid.map_or(1, |g| g.per_row as usize);
        let anchor = self.tile_ui.block.map(|(a, _)| a).filter(|_| shift);
        let result = match (anchor, ids.get(index)) {
            (Some(a), _) => {
                let (w, h, places) = block_places(per_row, a, index);
                let cells: Vec<Option<TileId>> =
                    places.iter().map(|&i| ids.get(i).copied()).collect();
                self.editor.set_tile_block(set, w, h, &cells)
            }
            (None, Some(&tile)) => self.editor.set_current_tile(set, tile),
            (None, None) => return,
        };
        match result {
            Ok(()) => {
                self.tile_ui.block = Some((anchor.unwrap_or(index), index));
                self.tile_ui.terrain.painting = false;
            }
            Err(e) => self.notify_error(e.to_string()),
        }
        self.ui_dirty = true;
    }

    /// Switches between placing tiles and drawing on them.
    pub(crate) fn set_tile_mode(&mut self, mode: TileMode) {
        if self.tile_ui.mode != mode && self.settle() {
            self.tile_ui.mode = mode;
        }
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    /// J: the other mode, said on the status line when no tilemap layer is active.
    pub(crate) fn toggle_tile_mode(&mut self) {
        let next = match self.tile_ui.mode {
            TileMode::Tiles => TileMode::Pixels,
            TileMode::Pixels => TileMode::Tiles,
        };
        self.set_tile_mode(next);
        let said = match (self.active_tileset().is_some(), self.tile_ui.mode) {
            (false, _) => "Tiles and Pixels modes work on a tilemap layer",
            (true, TileMode::Tiles) => "Tiles mode: the tools place, clear and pick tiles",
            (true, TileMode::Pixels) => "Pixels mode: the tools draw on the tiles placed here",
        };
        self.notify(said.to_string());
    }
}

#[cfg(test)]
#[path = "panel_tiles_tests.rs"]
mod tests;
