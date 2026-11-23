//! Terrain sets and terrain strokes in the random runs (M11's 11.2 and 11.3, mixed in with the
//! M10 checks): sets of the three kinds, from templates or empty, renamed, removed, their slots
//! filled and emptied, and strokes of every shape that paint or clear, in both corner modes.

use super::Model;
use crate::go;
use pikslar_core::autotile::TerrainKind;
use pikslar_core::command::{Corners, TerrainShape, TerrainTool};
use pikslar_core::tileset::TilesetId;
use pikslar_core::{TerrainCmd, TileTool};

const KINDS: [TerrainKind; 3] = [
    TerrainKind::Edge4,
    TerrainKind::Corner4,
    TerrainKind::Blob47,
];
const SHAPES: [TerrainShape; 4] = [
    TerrainShape::Brush,
    TerrainShape::Rect,
    TerrainShape::Fill,
    TerrainShape::Retile,
];

impl Model {
    /// The id of one of the tileset's terrain sets, if it has any.
    pub(super) fn some_terrain(&mut self, tileset: TilesetId) -> Option<u16> {
        let set = self.editor.doc().tilesets.get(tileset)?;
        let ids: Vec<u16> = set.terrains().iter().map(|t| t.id()).collect();
        (!ids.is_empty()).then(|| ids[self.rng.below(ids.len())])
    }

    pub(super) fn terrain_set_step(&mut self) -> Result<(), String> {
        let pushes = self.editor.history().pushes();
        self.edit("a terrain set", Self::terrain_command)?;
        self.counts.terrains += u64::from(self.editor.history().pushes() != pushes);
        Ok(())
    }

    fn terrain_command(&mut self) {
        let Some(tileset) = self.some_tileset() else {
            return;
        };
        let set = self.some_terrain(tileset);
        let slot = self.rng.below(47) as u8;
        let command = match (self.rng.below(6), set) {
            (_, None) | (0, _) => TerrainCmd::New {
                tileset,
                kind: KINDS[self.rng.below(KINDS.len())],
                name: format!("terrain {}", self.rng.below(9)),
                template: self.rng.chance(2),
            },
            (1, Some(set)) => TerrainCmd::Rename {
                tileset,
                set,
                name: format!("t{}", self.rng.below(9)),
            },
            (2, Some(set)) => TerrainCmd::Remove { tileset, set },
            (3, Some(set)) => TerrainCmd::Clear { tileset, set, slot },
            (_, Some(set)) => match self.some_tile(tileset) {
                Some(tile) => TerrainCmd::Assign {
                    tileset,
                    set,
                    slot,
                    tile,
                },
                None => TerrainCmd::Clear { tileset, set, slot },
            },
        };
        go(&mut self.editor, command);
    }

    /// A terrain stroke on a map layer, under a selection now and then. What it lacks (a map
    /// layer, a terrain set of its tileset from a template, a selection) is made first, each a
    /// step of its own.
    pub(super) fn counted_terrain_stroke(&mut self) -> Result<(), String> {
        let Some(index) = self.ready_map_layer()? else {
            return Ok(());
        };
        let Some(tileset) = self.editor.doc().layers()[index].tileset() else {
            return Ok(());
        };
        if self.some_terrain(tileset).is_none() {
            let kind = KINDS[self.rng.below(KINDS.len())];
            self.edit("a terrain template", |m| {
                let name = "ground".into();
                let template = true;
                go(
                    &mut m.editor,
                    TerrainCmd::New {
                        tileset,
                        kind,
                        name,
                        template,
                    },
                )
            })?;
        }
        let Some(set) = self.some_terrain(tileset) else {
            return Ok(());
        };
        if self.rng.chance(4) {
            self.selecting()?;
        }
        let tool = TerrainTool {
            tileset,
            set,
            shape: SHAPES[self.rng.below(SHAPES.len())],
            clear: self.rng.chance(3),
            corners: [Corners::Cell, Corners::Vertex][self.rng.below(2)],
        };
        let pushes = self.editor.history().pushes();
        self.edit("a terrain stroke", |m| {
            m.drive_stroke(TileTool::Terrain(tool))
        })?;
        self.counts.terrain_strokes += u64::from(self.editor.history().pushes() != pushes);
        Ok(())
    }
}
