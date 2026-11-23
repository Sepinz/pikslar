//! Renaming a tileset or a terrain set: the tileset menu opens the name in the Tiles header, the
//! terrain menu a terrain set's name in the terrain row, for typing. Enter keeps it as one step of
//! undo, Escape or an empty name leaves it as it was, and a press anywhere else keeps it. The
//! same route as a palette group's name.

use crate::app::App;
use pikslar_core::autotile::MAX_TERRAIN_NAME;
use pikslar_core::tileset::{TilesetId, MAX_TILESET_NAME};
use pikslar_core::{StepCmd, TerrainCmd, TileCmd};
use pikslar_platform::{Key, Mods};
use pikslar_ui::{edit_for, Edit, TextEdit};

/// The tileset (or, with `terrain`, its terrain set) whose name is being typed, and the name so
/// far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TilesetNaming {
    pub tileset: TilesetId,
    pub terrain: Option<u16>,
    pub edit: TextEdit,
}

/// The longest name a tileset, or with `terrain` a terrain set, takes.
fn most(terrain: Option<u16>) -> usize {
    match terrain {
        Some(_) => MAX_TERRAIN_NAME,
        None => MAX_TILESET_NAME,
    }
}

impl App {
    pub(crate) fn start_tileset_naming(&mut self, tileset: TilesetId) {
        if let Ok(found) = self.editor.doc().tileset(tileset) {
            let name = found.name().to_string();
            self.start_naming_in_tiles(tileset, None, &name);
        }
    }

    /// Opens a terrain set's name for typing in the terrain row.
    pub(crate) fn start_terrain_naming(&mut self, tileset: TilesetId, set: u16) {
        if let Ok(found) = self.editor.doc().terrain(tileset, set) {
            let name = found.name().to_string();
            self.start_naming_in_tiles(tileset, Some(set), &name);
        }
    }

    fn start_naming_in_tiles(&mut self, tileset: TilesetId, terrain: Option<u16>, name: &str) {
        let mut edit = TextEdit::new(name, most(terrain));
        edit.apply(Edit::End { extend: false });
        self.finish_naming();
        self.finish_slice_naming();
        self.finish_group_naming();
        self.tile_ui.naming = Some(TilesetNaming {
            tileset,
            terrain,
            edit,
        });
        self.ui_dirty = true;
    }

    /// Keeps the name typed, as one step, unless it is empty or the same.
    pub(crate) fn finish_tileset_naming(&mut self) {
        let Some(naming) = self.tile_ui.naming.take() else {
            return;
        };
        self.ui_dirty = true;
        let name = naming.edit.text().to_string();
        if name.is_empty() {
            return;
        }
        let tileset = naming.tileset;
        match naming.terrain {
            Some(set) => self.run(TerrainCmd::Rename { tileset, set, name }),
            None => self.run(TileCmd::RenameTileset { tileset, name }),
        };
        // The next rename is a step of its own.
        self.run(StepCmd::EndDrag);
    }

    /// While a tileset is being named, the keyboard belongs to the name.
    pub(crate) fn tileset_naming_key(&mut self, key: Key, mods: Mods) {
        let edit = match key {
            Key::Enter => return self.finish_tileset_naming(),
            Key::Escape => {
                self.tile_ui.naming = None;
                self.ui_dirty = true;
                return;
            }
            // Characters come as text, from the keyboard layout.
            _ => match edit_for(key, mods) {
                Some(edit) => edit,
                None => return,
            },
        };
        self.edit_tileset_name(edit);
    }

    pub(crate) fn edit_tileset_name(&mut self, edit: Edit) {
        let Some(naming) = &mut self.tile_ui.naming else {
            return;
        };
        self.ui_dirty = true;
        let most = most(naming.terrain);
        let full = naming.edit.text().chars().count() >= most;
        if !naming.edit.apply(edit) && matches!(edit, Edit::Insert(_)) && full {
            self.notify(format!("Names stop at {most} letters"));
        }
    }
}
