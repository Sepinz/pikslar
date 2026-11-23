//! Automatic tilemaps in random runs of their own (M11, phase 11.5). Where the mixed runs of
//! `tiles_terrain.rs` take a terrain action now and then, these runs are mostly terrain: sets
//! made, renamed, removed, filled and emptied; terrain strokes of every shape, painting and
//! clearing, by cells and by corners; tiles placed by hand, which pin; terrain tiles removed,
//! which empties their slots; undo, redo and saves, with ordinary edits among them.
//!
//! Besides what every run checks (each step undoes and redoes to the very document it made, the
//! maps keep T1 to T6, a save opens to the same document), each stroke is held to the model's own
//! reading of the rules (`autotile_rules.rs`): what the cells it changed hold, that no pin is left
//! on them, that what its preview showed is what it wrote; each hand placement to the pin rule;
//! each tile removal to its slots; and a stroke on a document saved and opened again must do
//! exactly what it does on the document it was saved from.

use super::autotile_footprint::{Footprint, Grid};
use super::autotile_rules::{placement_pins, stroke_result};
use super::{bad, Counts, Model, Plan};
use crate::go;
use crate::same::{first_difference, with_holds_numbered_afresh};
use pikslar_core::autotile::{TerrainKind, TerrainSet};
use pikslar_core::command::{Corners, TerrainShape, TerrainTool};
use pikslar_core::tileset::TilesetId;
use pikslar_core::{Editor, Image, TerrainCmd, TerrainPreview, TileCmd, TileTool};
use pikslar_format::piks;

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
/// The map tools that place tiles by hand.
const PLACING: [TileTool; 4] = [
    TileTool::Place,
    TileTool::Line,
    TileTool::Rect,
    TileTool::Fill,
];

/// A map cel: its layer and frame.
type Place = (usize, usize);

/// The canvas points a stroke's pointer visited.
type Path = Vec<(i32, i32)>;

/// One random run of terrain work. Any failure names what went wrong, and the seed reproduces it.
pub fn run(plan: Plan, seed: u64) -> Result<Counts, String> {
    let mut model = Model::new(plan, seed)?;
    for step in 0..plan.steps {
        model
            .terrain_one_step()
            .map_err(|e| format!("terrain seed {seed} step {step}: {e}"))?;
    }
    model
        .check_save()
        .map_err(|e| format!("terrain seed {seed} save: {e}"))?;
    model
        .walk_whole_timeline()
        .map_err(|e| format!("terrain seed {seed} final walk: {e}"))?;
    Ok(model.counts)
}

impl Model {
    fn terrain_one_step(&mut self) -> Result<(), String> {
        match self.rng.below(20) {
            0..=6 => self.checked_terrain_stroke(),
            7..=9 => self.terrain_set_step(),
            10 | 11 => self.checked_placement(),
            12 => self.checked_tile_removal(),
            13 => self.tile_step(),
            14 => self.one_edit(),
            15 | 16 => self.undo(),
            17 => self.redo(),
            18 => self.reloaded_stroke(),
            _ => {
                self.check_save()?;
                self.counts.saves += 1;
                Ok(())
            }
        }?;
        self.tilemaps_kept("a terrain run step")
    }

    /// A map layer, made active, and the id of one of its tileset's terrain sets, after making
    /// what is missing: a set from a template of a random kind, as a step of its own.
    fn ready_terrain(&mut self) -> Result<Option<(TilesetId, u16)>, String> {
        let Some(index) = self.ready_map_layer()? else {
            return Ok(None);
        };
        let Some(tileset) = self.editor.doc().layers()[index].tileset() else {
            return Ok(None);
        };
        if self.some_terrain(tileset).is_none() {
            let kind = KINDS[self.rng.below(KINDS.len())];
            self.edit("a terrain template", |m| {
                let (name, template) = ("ground".to_owned(), true);
                let new = TerrainCmd::New {
                    tileset,
                    kind,
                    name,
                    template,
                };
                go(&mut m.editor, new)
            })?;
        }
        Ok(self.some_terrain(tileset).map(|set| (tileset, set)))
    }

    fn map_image(&self, (layer, frame): Place) -> Option<Image> {
        let layer = self.editor.doc().layers().get(layer)?;
        Some(layer.cel(frame).image().clone())
    }

    /// A terrain stroke of a random shape, held to the rules cell by cell.
    fn checked_terrain_stroke(&mut self) -> Result<(), String> {
        let Some((tileset, set)) = self.ready_terrain()? else {
            return Ok(());
        };
        if self.rng.chance(5) {
            self.selecting()?;
        }
        let tool = self.random_terrain_tool(tileset, set);
        let place = (self.editor.layer, self.editor.frame);
        let before = self.map_image(place).ok_or("no map cel")?;
        let pushes = self.editor.history().pushes();
        let selection = self.editor.selection().is_some();
        let mut ran = None;
        self.edit("a checked terrain stroke", |m| ran = m.drive_terrain(tool))?;
        let (preview, path) = ran.unwrap_or_default();
        if self.editor.history().pushes() == pushes {
            return Ok(());
        }
        self.counts.terrain_strokes += 1;
        let after = self.map_image(place).ok_or("the map cel is gone")?;
        let found = self
            .editor
            .doc()
            .terrain(tileset, set)
            .map_err(|e| e.to_string())?;
        let foot = self.footprint(tool, &path, place, &before, found)?;
        let both = (&before, &after);
        self.counts.terrain_cells += stroke_result(found, both, &foot, selection)? as u64;
        match preview {
            Some(shown) => self.preview_was_written(&shown, place, &before, &after),
            None => Ok(()),
        }
    }

    fn random_terrain_tool(&mut self, tileset: TilesetId, set: u16) -> TileTool {
        TileTool::Terrain(TerrainTool {
            tileset,
            set,
            shape: SHAPES[self.rng.below(SHAPES.len())],
            clear: self.rng.chance(3),
            corners: [Corners::Cell, Corners::Vertex][self.rng.below(2)],
        })
    }

    /// Runs a terrain stroke along a few random points: whole, or pressed and dragged and then
    /// committed or cancelled. What a brush or rectangle showed just before its commit is given
    /// back.
    fn drive_terrain(&mut self, tool: TileTool) -> Option<(Option<TerrainPreview>, Path)> {
        let path: Path = (0..1 + self.rng.below(5)).map(|_| self.point()).collect();
        if self.rng.chance(3) {
            go(
                &mut self.editor,
                TileCmd::Stroke {
                    tool,
                    path: path.clone(),
                },
            );
            return Some((None, path));
        }
        let (x, y) = path[0];
        self.editor.apply(TileCmd::Begin { tool, x, y }).ok()?;
        for &(x, y) in &path[1..] {
            go(&mut self.editor, TileCmd::To { x, y });
        }
        let preview = self.editor.terrain_preview();
        let end = match self.rng.chance(6) {
            true => TileCmd::Cancel,
            false => TileCmd::Commit,
        };
        go(&mut self.editor, end);
        Some((preview, path))
    }

    /// What the stroke of `tool` along `path` painted, on the map `before` it.
    fn footprint(
        &self,
        tool: TileTool,
        path: &[(i32, i32)],
        (layer, frame): Place,
        before: &Image,
        set: &TerrainSet,
    ) -> Result<Footprint, String> {
        let TileTool::Terrain(tool) = tool else {
            return Err("a terrain stroke with a tool that is not terrain".to_owned());
        };
        let doc = self.editor.doc();
        let layer = &doc.layers()[layer];
        let tileset = layer.tileset().ok_or("not a map layer")?;
        let size = doc.tileset(tileset).map_err(|e| e.to_string())?.tile_size();
        let at = layer.cel(frame).position();
        let grid = Grid::new(size, at, (doc.width, doc.height));
        let corner4 = set.kind() == TerrainKind::Corner4;
        Ok(Footprint::new(&tool, path, &grid, corner4, before))
    }

    /// The cells a committed stroke changed are exactly those its preview showed, as shown.
    fn preview_was_written(
        &mut self,
        shown: &TerrainPreview,
        place: Place,
        before: &Image,
        after: &Image,
    ) -> Result<(), String> {
        if (shown.layer, shown.frame) != place {
            return bad(format!(
                "the preview was of {:?}",
                (shown.layer, shown.frame)
            ));
        }
        let written: Vec<((i32, i32), u32)> = super::autotile_rules::changed(before, after)
            .into_iter()
            .map(|(c, _, now)| (c, now))
            .collect();
        let previewed: Vec<((i32, i32), u32)> =
            shown.cells.iter().map(|&(c, v)| (c, v.bits())).collect();
        if written != previewed {
            return bad(format!(
                "the commit wrote {} cells, the preview showed {}: first written {:?}, shown {:?}",
                written.len(),
                previewed.len(),
                written.first(),
                previewed.first()
            ));
        }
        self.counts.previews += 1;
        Ok(())
    }

    /// Tiles placed by hand with a map tool, a terrain tile half the time: every cell placed
    /// with a terrain tile is pinned, every other is not.
    fn checked_placement(&mut self) -> Result<(), String> {
        let Some((tileset, set)) = self.ready_terrain()? else {
            return Ok(());
        };
        let terrain_tile = self.editor.doc().terrain(tileset, set).ok().and_then(|t| {
            let filled: Vec<_> = t.slots().iter().flatten().copied().collect();
            filled.get(self.rng.below(filled.len().max(1))).copied()
        });
        match terrain_tile.filter(|_| self.rng.chance(2)) {
            Some(tile) => drop(self.editor.set_current_tile(tileset, tile)),
            None => self.choose_stamp(tileset)?,
        }
        let place = (self.editor.layer, self.editor.frame);
        let before = self.map_image(place).ok_or("no map cel")?;
        let tool = PLACING[self.rng.below(PLACING.len())];
        self.edit("a hand placement", |m| m.drive_stroke(tool))?;
        let after = self.map_image(place).ok_or("the map cel is gone")?;
        let found = self
            .editor
            .doc()
            .tileset(tileset)
            .map_err(|e| e.to_string())?;
        self.counts.pins += placement_pins(found, &before, &after)? as u64;
        Ok(())
    }

    /// Removes a tile, one that fills a slot when there is one: afterwards no slot names it.
    fn checked_tile_removal(&mut self) -> Result<(), String> {
        let Some((tileset, set)) = self.ready_terrain()? else {
            return Ok(());
        };
        let filled = self.editor.doc().terrain(tileset, set).ok().and_then(|t| {
            let filled: Vec<_> = t.slots().iter().flatten().copied().collect();
            filled.get(self.rng.below(filled.len().max(1))).copied()
        });
        let Some(tile) = filled.or_else(|| self.some_tile(tileset)) else {
            return Ok(());
        };
        let pushes = self.editor.history().pushes();
        self.edit("removing a terrain tile", |m| {
            go(&mut m.editor, TileCmd::RemoveTile { tileset, tile })
        })?;
        if self.editor.history().pushes() == pushes {
            return Ok(());
        }
        let found = self
            .editor
            .doc()
            .tileset(tileset)
            .map_err(|e| e.to_string())?;
        if found.terrains().iter().any(|t| t.holds(tile)) {
            return bad(format!(
                "tile {} is gone but a slot still names it",
                tile.get()
            ));
        }
        self.counts.removed_tiles += 1;
        Ok(())
    }

    /// Saves, opens the file as a new document, and makes the same whole terrain stroke on both:
    /// the documents after must be the same, pins, slots and all.
    fn reloaded_stroke(&mut self) -> Result<(), String> {
        let Some((tileset, set)) = self.ready_terrain()? else {
            return Ok(());
        };
        let back = piks::load(&piks::save(self.editor.doc()))
            .map_err(|e| format!("own save will not open: {e}"))?;
        let mut reopened = Editor::new(back);
        (reopened.layer, reopened.frame) = (self.editor.layer, self.editor.frame);
        if let Some(mask) = self.editor.selection().cloned() {
            let shape = Box::new((*mask).clone());
            let op = pikslar_core::SelectOp::Replace;
            go(
                &mut reopened,
                pikslar_core::SelectionCmd::Select { shape, op },
            );
        }
        let tool = self.random_terrain_tool(tileset, set);
        let path: Vec<(i32, i32)> = (0..1 + self.rng.below(4)).map(|_| self.point()).collect();
        let stroke = TileCmd::Stroke { tool, path };
        let (theirs, ours) = (stroke.clone(), stroke);
        go(&mut reopened, theirs);
        self.edit("a stroke after a reopen", |m| go(&mut m.editor, ours))?;
        let a = with_holds_numbered_afresh(reopened.doc())?;
        let b = with_holds_numbered_afresh(self.editor.doc())?;
        if let Some(what) = first_difference(&a, &b) {
            return bad(format!(
                "a stroke on the reopened file did otherwise: {what}"
            ));
        }
        self.counts.reloaded_strokes += 1;
        Ok(())
    }
}
