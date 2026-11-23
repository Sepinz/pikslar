//! The terrain menus: the terrain button (or a right click on the terrain row) opens the terrain
//! menu, which chooses a set to paint with and makes, renames and removes sets; "New terrain
//! set..." (also in the tileset menu) opens the kinds; a right click on a slot opens the slot
//! menu. Every change is one `TerrainCmd`.

use crate::app::App;
use crate::frame_menu::{Entries, MenuKind};
use crate::palette_menu::row_of;
use crate::terrain_ui::{kind_name, missing};
use pikslar_core::autotile::TerrainKind;
use pikslar_core::{Corners, TerrainCmd};

/// The kinds a new set is offered in, then the same with template tiles.
const KINDS: [TerrainKind; 3] = [
    TerrainKind::Edge4,
    TerrainKind::Corner4,
    TerrainKind::Blob47,
];
const NEW_ROWS: [&str; 6] = [
    "Edge4, 16 slots",
    "Corner4, 16 slots",
    "Blob47, 47 slots",
    "Edge4 with template tiles",
    "Corner4 with template tiles",
    "Blob47 with template tiles",
];

/// Where the terrain menu's rows are, after one row for each set.
mod row {
    pub const TILES: usize = 0;
    pub const NEW: usize = 1;
    pub const RENAME: usize = 2;
    pub const REMOVE: usize = 3;
    pub const CORNERS: usize = 4;
}

impl App {
    /// The terrain menu a right click at the point opens: on the terrain row, or on a slot.
    pub(crate) fn terrain_menu_at(&mut self, x: i32, y: i32) -> Option<MenuKind> {
        if self.tile_ui.terrain.row.contains(x, y) && self.shown_terrain().is_some() {
            return Some(MenuKind::Terrain);
        }
        if !self.showing_slots() || !self.tile_ui.list.contains(x, y) {
            return None;
        }
        self.tile_ui.terrain.menu_slot = self.slot_at(x, y);
        Some(MenuKind::TerrainSlot)
    }

    pub(crate) fn open_terrain_menu(&mut self, at: (i32, i32)) {
        self.open_palette_kind(at, MenuKind::Terrain);
    }

    pub(crate) fn terrain_entries(&self, kind: MenuKind) -> Entries {
        match kind {
            MenuKind::TerrainNew => NEW_ROWS.iter().map(|&r| row_of(r, true)).collect(),
            MenuKind::TerrainSlot => self.slot_entries(),
            _ => self.terrain_menu_entries(),
        }
    }

    fn terrain_menu_entries(&self) -> Entries {
        let doc = self.editor.doc();
        let set = self.active_tileset().and_then(|t| doc.tileset(t).ok());
        let sets = set.map_or(&[][..], |s| s.terrains());
        let mut rows: Entries = sets
            .iter()
            .map(|t| {
                let (name, kind) = (t.name(), kind_name(t.kind()));
                match missing(t) {
                    0 => row_of(format!("Paint {name} ({kind})"), true),
                    n => row_of(format!("Paint {name} ({kind}, {n} missing)"), true),
                }
            })
            .collect();
        let shown = self.shown_terrain();
        let corner4 = shown.is_some_and(|t| t.kind() == TerrainKind::Corner4);
        let corners = match self.tile_ui.terrain.corners {
            Corners::Cell => "Corner4: paint single corners",
            Corners::Vertex => "Corner4: paint whole cells",
        };
        rows.extend([
            row_of("Paint tiles, not terrain", self.tile_ui.terrain.painting),
            row_of("New terrain set...", set.is_some()),
            row_of("Rename terrain", shown.is_some()),
            row_of("Remove terrain (its tiles stay)", shown.is_some()),
            row_of(corners, corner4),
        ]);
        rows
    }

    fn slot_entries(&self) -> Entries {
        let shown = self.shown_terrain();
        let slot = self.tile_ui.terrain.menu_slot;
        let filled = slot.and_then(|s| shown?.slot(usize::from(s))).is_some();
        let tile = self.editor.tiles().current();
        let ours = tile.is_some_and(|c| Some(c.0) == self.active_tileset());
        vec![
            row_of("Put the chosen tile here", ours && slot.is_some()),
            row_of("Clear slot", filled),
        ]
    }

    pub(crate) fn terrain_menu_action(&mut self, kind: MenuKind, n: usize, at: (i32, i32)) {
        match kind {
            MenuKind::TerrainNew => self.new_terrain(n),
            MenuKind::TerrainSlot => self.slot_action(n),
            _ => self.terrain_action(n, at),
        }
        self.full_canvas = true;
        self.ui_dirty = true;
    }

    fn terrain_action(&mut self, n: usize, at: (i32, i32)) {
        let Some(tileset) = self.active_tileset() else {
            return;
        };
        let ids: Vec<u16> = match self.editor.doc().tileset(tileset) {
            Ok(set) => set.terrains().iter().map(|t| t.id()).collect(),
            Err(_) => return,
        };
        if let Some(&set) = ids.get(n) {
            let ui = &mut self.tile_ui.terrain;
            (ui.chosen, ui.painting) = (Some((tileset, set)), true);
            let said = "Painting terrain: B paints, E clears, U and G fill, Alt-click re-picks";
            return self.notify(said.to_string());
        }
        let shown = self.shown_terrain_id();
        let ui = &mut self.tile_ui.terrain;
        match (n - ids.len(), shown) {
            (row::TILES, _) => ui.painting = false,
            (row::CORNERS, _) => {
                ui.corners = match ui.corners {
                    Corners::Cell => Corners::Vertex,
                    Corners::Vertex => Corners::Cell,
                }
            }
            (row::NEW, _) => self.open_palette_kind(at, MenuKind::TerrainNew),
            (row::RENAME, Some((tileset, set))) => self.start_terrain_naming(tileset, set),
            (row::REMOVE, Some((tileset, set))) => {
                self.run(TerrainCmd::Remove { tileset, set });
            }
            _ => {}
        }
    }

    /// Makes a terrain set of the `n`th kind (with template tiles from the fourth row), shows
    /// its slots and paints with it.
    pub(crate) fn new_terrain(&mut self, n: usize) {
        let (Some(tileset), Some(&kind)) = (self.active_tileset(), KINDS.get(n % 3)) else {
            return;
        };
        let count = self
            .editor
            .doc()
            .tileset(tileset)
            .map_or(0, |s| s.terrains().len());
        let command = TerrainCmd::New {
            tileset,
            kind,
            name: format!("Terrain {}", count + 1),
            template: n >= KINDS.len(),
        };
        let made = self.run(command).and_then(|o| o.created);
        if let Some(set) = made.and_then(|id| u16::try_from(id).ok()) {
            let ui = &mut self.tile_ui.terrain;
            (ui.chosen, ui.painting, ui.slots) = (Some((tileset, set)), true, true);
        }
    }

    fn slot_action(&mut self, n: usize) {
        let (Some((tileset, set)), Some(slot)) =
            (self.shown_terrain_id(), self.tile_ui.terrain.menu_slot)
        else {
            return;
        };
        match n {
            0 => self.put_in_slot(slot),
            1 => {
                self.run(TerrainCmd::Clear { tileset, set, slot });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "terrain_menu_tests.rs"]
mod tests;
