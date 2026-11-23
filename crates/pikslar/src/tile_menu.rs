//! The tile menus: a right click on a thumbnail (or the thumbnails' room) opens the tile menu,
//! one on the Tiles header the tileset menu, and one on the New layer button the menu that makes
//! a tilemap layer, with one of the document's tilesets or a new one of a tile size.

use crate::app::App;
use crate::frame_menu::{Entries, MenuKind};
use crate::palette_menu::row_of;
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::{TileCmd, TileSource};

/// The tile sizes a new tileset is offered in.
pub(crate) const TILE_SIZES: [u16; 3] = [8, 16, 32];

/// Where the tileset menu's rows are.
mod row {
    pub const NEW: usize = 0;
    pub const RENAME: usize = 1;
    pub const INDEPENDENT: usize = 2;
    pub const UNUSED: usize = 3;
    pub const TERRAIN: usize = 4;
}

impl App {
    /// Opens a tile menu if a right click at `(x, y)` is on the New layer button, the Tiles
    /// header or the thumbnails.
    pub(crate) fn open_tile_menus(&mut self, x: i32, y: i32) -> bool {
        let kind = if self.tile_ui.new_layer.contains(x, y) {
            MenuKind::NewTilemap
        } else if self.active_tileset().is_none() {
            return false;
        } else if self.tile_ui.header.contains(x, y) {
            MenuKind::Tileset
        } else if let Some(kind) = self.terrain_menu_at(x, y) {
            kind
        } else if self.tile_ui.list.contains(x, y) {
            self.tile_ui.menu_tile = self.tile_at(x, y);
            MenuKind::Tile
        } else {
            return false;
        };
        self.open_palette_kind((x, y), kind);
        true
    }

    /// The tile whose thumbnail is at the point.
    fn tile_at(&self, x: i32, y: i32) -> Option<TileId> {
        let grid = self.tile_ui.grid?;
        let index = grid.target((x, y))?;
        grid.cell_rect(index).filter(|r| r.contains(x, y))?;
        let set = self.editor.doc().tileset(self.active_tileset()?).ok()?;
        set.tiles().get(index).map(|t| t.id)
    }

    pub(crate) fn tile_entries(&self, kind: MenuKind) -> Entries {
        match kind {
            MenuKind::Tileset => self.tileset_entries(),
            MenuKind::NewTilemap => self.new_tilemap_entries(true),
            MenuKind::TileSize => self.new_tilemap_entries(false),
            MenuKind::TileUnused => self
                .unused_tilesets()
                .into_iter()
                .map(|(_, name)| row_of(format!("Remove {name}"), true))
                .collect(),
            MenuKind::Terrain | MenuKind::TerrainNew | MenuKind::TerrainSlot => {
                self.terrain_entries(kind)
            }
            _ => self.tile_menu_entries(),
        }
    }

    fn tile_menu_entries(&self) -> Entries {
        let tile = self.tile_ui.menu_tile;
        let set = self.active_tileset();
        let remove = match (set, tile) {
            (Some(set), Some(tile)) => {
                let cells = self.editor.doc().cells_using(set, tile);
                format!("Remove tile, emptying {cells} cell(s)")
            }
            _ => "Remove tile".to_string(),
        };
        vec![
            row_of("New tile", true),
            row_of("Tiles from selection", self.has_selection()),
            row_of("Duplicate tile", tile.is_some()),
            row_of(remove, tile.is_some()),
        ]
    }

    fn tileset_entries(&self) -> Entries {
        let doc = self.editor.doc();
        let shared = self
            .active_tileset()
            .is_some_and(|set| doc.tileset_users(set) > 1);
        vec![
            row_of("New tileset and layer...", true),
            row_of("Rename tileset", true),
            row_of("Make independent (own copy)", shared),
            row_of(
                "Remove an unused tileset...",
                !self.unused_tilesets().is_empty(),
            ),
            row_of("New terrain set...", true),
        ]
    }

    /// The document's tilesets to make a layer with, if `with_tilesets`, then new tile sizes.
    fn new_tilemap_entries(&self, with_tilesets: bool) -> Entries {
        let doc = self.editor.doc();
        let mut rows: Entries = Vec::new();
        if with_tilesets {
            for set in doc.tilesets.iter() {
                let (w, h) = set.tile_size();
                rows.push(row_of(format!("Tilemap of {} {w}x{h}", set.name()), true));
            }
        }
        for n in TILE_SIZES {
            rows.push(row_of(format!("Tilemap, new tileset {n}x{n}"), true));
        }
        rows
    }

    /// The tilesets no layer uses, with their names.
    fn unused_tilesets(&self) -> Vec<(TilesetId, String)> {
        let doc = self.editor.doc();
        doc.tilesets
            .iter()
            .filter(|set| doc.tileset_users(set.id()) == 0)
            .map(|set| (set.id(), set.name().to_string()))
            .collect()
    }

    pub(crate) fn tile_menu_action(&mut self, kind: MenuKind, n: usize, at: (i32, i32)) {
        match kind {
            MenuKind::Tileset => self.tileset_action(n, at),
            MenuKind::NewTilemap => {
                let ids: Vec<TilesetId> =
                    self.editor.doc().tilesets.iter().map(|s| s.id()).collect();
                match ids.get(n) {
                    Some(&tileset) => self.new_tilemap_layer(tileset),
                    None => self.new_map_layer(n - ids.len()),
                }
            }
            MenuKind::TileSize => self.new_map_layer(n),
            MenuKind::Terrain | MenuKind::TerrainNew | MenuKind::TerrainSlot => {
                return self.terrain_menu_action(kind, n, at)
            }
            MenuKind::TileUnused => {
                if let Some(&(tileset, _)) = self.unused_tilesets().get(n) {
                    self.run(TileCmd::RemoveTileset { tileset });
                }
            }
            _ => self.tile_action(n),
        }
        self.ui_dirty = true;
    }

    fn tileset_action(&mut self, n: usize, at: (i32, i32)) {
        let Some(set) = self.active_tileset() else {
            return;
        };
        match n {
            row::NEW => self.open_palette_kind(at, MenuKind::TileSize),
            row::RENAME => self.start_tileset_naming(set),
            row::INDEPENDENT => {
                if let Some(layer) = self.editor.layer_id(self.editor.layer) {
                    if self.run(TileCmd::CopyTilesetFor { layer }).is_some() {
                        self.tile_ui.block = None;
                        self.notify("This layer now has its own copy of the tileset".to_string());
                    }
                }
            }
            row::UNUSED => self.open_palette_kind(at, MenuKind::TileUnused),
            row::TERRAIN => self.open_palette_kind(at, MenuKind::TerrainNew),
            _ => {}
        }
    }

    /// A tilemap layer above the active one, placing tiles of `tileset`.
    fn new_tilemap_layer(&mut self, tileset: TilesetId) {
        if !self.settle() {
            return;
        }
        let at = self.editor.layer + 1;
        self.run(TileCmd::NewLayer { at, tileset });
    }

    /// A tilemap layer above the active one with a new tileset of the `n`th size.
    fn new_map_layer(&mut self, n: usize) {
        let Some(&side) = TILE_SIZES.get(n) else {
            return;
        };
        if !self.settle() {
            return;
        }
        let doc = self.editor.doc();
        let mode = doc.layers()[self.editor.layer].mode;
        let name = format!("Tileset {}", doc.tilesets.len() + 1);
        let at = self.editor.layer + 1;
        let command = TileCmd::NewMapLayer {
            at,
            name,
            mode,
            tile_w: side,
            tile_h: side,
        };
        if self.run(command).is_some() {
            self.tile_ui.block = None;
        }
    }

    fn tile_action(&mut self, n: usize) {
        let (Some(tileset), tile) = (self.active_tileset(), self.tile_ui.menu_tile) else {
            return;
        };
        match (n, tile) {
            (0, _) => self.add_tile(tileset, TileSource::Blank),
            (1, _) => self.add_tile(tileset, TileSource::Selection),
            (2, Some(tile)) => {
                if let Some(made) = self.run(TileCmd::DuplicateTile { tileset, tile }) {
                    self.choose_created(tileset, made.created);
                }
            }
            (3, Some(tile)) => self.remove_tile(tileset, tile),
            _ => {}
        }
    }

    fn add_tile(&mut self, tileset: TilesetId, from: TileSource) {
        let cut = matches!(from, TileSource::Selection);
        let Some(made) = self.run(TileCmd::AddTile { tileset, from }) else {
            return;
        };
        if cut && !made.changed {
            let said = "No new tiles: the selected tiles are clear or the tileset has them";
            self.notify(said.to_string());
        }
        self.choose_created(tileset, made.created);
    }

    /// Makes a tile just made the stamp.
    fn choose_created(&mut self, tileset: TilesetId, created: Option<u32>) {
        if let Some(tile) = created.and_then(TileId::new) {
            if self.editor.set_current_tile(tileset, tile).is_ok() {
                let place = self
                    .editor
                    .doc()
                    .tileset(tileset)
                    .ok()
                    .and_then(|s| s.position(tile));
                self.tile_ui.block = place.map(|p| (p, p));
            }
        }
    }

    /// Removes a tile, saying how many cells that emptied.
    fn remove_tile(&mut self, tileset: TilesetId, tile: TileId) {
        let cells = self.editor.doc().cells_using(tileset, tile);
        if self.run(TileCmd::RemoveTile { tileset, tile }).is_none() {
            return;
        }
        if self
            .editor
            .tiles()
            .current()
            .is_some_and(|(_, t, _)| t == tile)
        {
            self.editor.clear_tile_stamp();
        }
        self.tile_ui.block = None;
        self.notify(format!("Removed the tile and emptied {cells} cell(s)"));
    }
}

#[cfg(test)]
#[path = "tile_menu_tests.rs"]
mod tests;
