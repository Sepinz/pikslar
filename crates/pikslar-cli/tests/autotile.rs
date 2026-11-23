//! Autotiled maps for `spikes/check-autotile.py` (M11, 11.5), which reads their TMJ export and
//! checks every cell against its own reading of the rules, and for `spikes/check-tmj.py`, which
//! compares the TMJ drawn by Pillow and by Tiled with Pikslar's PNG.
//!
//! `write_autotile_fixtures` (ignored) writes, for each case, `<name>.piks` and
//! `<name>.terrain.json`: per tilemap layer, its tileset's name, each terrain set's kind and the
//! place in the tileset of the tile filling each slot (`null` when empty), how Corner4 sets were
//! painted, the cells placed by hand (pinned; TMJ never writes the pin) and the cells a stroke
//! cleared. Run it with `PIKSLAR_AUTOTILE_FIXTURES` set to a folder. Every case is built through
//! the editor's own commands, as the panel's tools would make it; the paint layer is hidden.

use pikslar_core::autotile::TerrainKind;
use pikslar_core::command::{Corners, TerrainShape, TerrainTool};
use pikslar_core::tileset::{TileId, TilesetId};
use pikslar_core::TileTool;
use pikslar_core::{rgba, ColorMode, Document, Editor, Grid, TerrainCmd, TileCmd, TileSource};
use pikslar_format::piks;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

/// Tiles are 4 pixels square; maps are 12 by 10 cells.
const TILE: i32 = 4;
const SIZE: (usize, usize) = (48, 40);

/// What the checker is told about one map layer.
struct Told {
    layer: String,
    /// Its place in the layer list; it is given its name when the case is done.
    index: usize,
    tileset: TilesetId,
    corners: Corners,
    pinned: Vec<(i32, i32)>,
    cleared: Vec<(i32, i32)>,
}

struct Case {
    editor: Editor,
    told: Vec<Told>,
}

/// A tile of distinct pixels, so that the PNG and Tiled's picture tell tiles apart.
fn pixels(seed: u32) -> Arc<pikslar_core::Image> {
    let px: Vec<u32> = (0..(TILE * TILE) as u32)
        .map(|i| match (i * 5 + seed) % 7 {
            0 => 0,
            v => rgba(
                (seed * 37) as u8,
                (v * 31 + i * 5) as u8,
                (seed * 11 + i) as u8,
                255,
            ),
        })
        .collect();
    let side = TILE as usize;
    Arc::new(Grid::from_cells(side, side, &px).into_image())
}

impl Case {
    fn new() -> Case {
        let doc = Document::new(SIZE.0, SIZE.1, ColorMode::Rgba).expect("size");
        Case {
            editor: Editor::new(doc),
            told: Vec::new(),
        }
    }

    /// A tilemap layer of a new tileset holding `tiles` drawn tiles, made active.
    fn map(&mut self, name: &str, tiles: u32) -> TilesetId {
        let at = self.editor.doc().layers().len();
        let made = self.editor.apply(TileCmd::NewMapLayer {
            at,
            name: name.into(),
            mode: ColorMode::Rgba,
            tile_w: TILE as u16,
            tile_h: TILE as u16,
        });
        let tileset = TilesetId::new(made.expect("a map").created.expect("an id"));
        for n in 0..tiles {
            let from = TileSource::Image(pixels(n + 1));
            let added = TileCmd::AddTile { tileset, from };
            self.editor.apply(added).expect("a tile");
        }
        self.told.push(Told {
            layer: name.into(),
            index: at,
            tileset,
            corners: Corners::Cell,
            pinned: Vec::new(),
            cleared: Vec::new(),
        });
        tileset
    }

    /// A terrain set, from a template whose blank tiles are then drawn, or empty.
    fn set(&mut self, tileset: TilesetId, kind: TerrainKind, template: bool) -> u16 {
        let name = format!("{kind:?}");
        let new = TerrainCmd::New {
            tileset,
            kind,
            name,
            template,
        };
        let made = self.editor.apply(new).expect("a set");
        let id = u16::try_from(made.created.expect("an id")).expect("small");
        let slots: Vec<TileId> = self
            .terrain(tileset, id)
            .slots()
            .iter()
            .flatten()
            .copied()
            .collect();
        for (n, tile) in slots.into_iter().enumerate() {
            let image = pixels(100 + n as u32 + u32::from(id) * 50);
            let draw = TileCmd::SetTileImage {
                tileset,
                tile,
                image,
            };
            self.editor.apply(draw).expect("drawn");
        }
        id
    }

    fn terrain(&self, tileset: TilesetId, set: u16) -> &pikslar_core::autotile::TerrainSet {
        self.editor.doc().terrain(tileset, set).expect("a set")
    }

    /// Fills slots of an empty set with tiles by their place in the tileset.
    fn assign(&mut self, tileset: TilesetId, set: u16, pairs: &[(u8, usize)]) {
        for &(slot, at) in pairs {
            let tile = self
                .editor
                .doc()
                .tileset(tileset)
                .expect("a tileset")
                .tiles()[at]
                .id;
            let assign = TerrainCmd::Assign {
                tileset,
                set,
                slot,
                tile,
            };
            self.editor.apply(assign).expect("assigned");
        }
    }

    fn told(&mut self) -> &mut Told {
        self.told.last_mut().expect("a map")
    }

    /// A terrain stroke through cells, or through vertices for `Corners::Vertex`.
    fn paint(
        &mut self,
        set: u16,
        shape: TerrainShape,
        clear: bool,
        corners: Corners,
        at: &[(i32, i32)],
    ) {
        let tileset = self.told().tileset;
        let point = |&(x, y): &(i32, i32)| match corners {
            Corners::Vertex => (x * TILE, y * TILE),
            Corners::Cell => (x * TILE + TILE / 2, y * TILE + TILE / 2),
        };
        let tool = TileTool::Terrain(TerrainTool {
            tileset,
            set,
            shape,
            clear,
            corners,
        });
        let path = at.iter().map(point).collect();
        let done = self
            .editor
            .apply(TileCmd::Stroke { tool, path })
            .expect("painted");
        assert!(done.changed, "a stroke that changed nothing");
        if corners == Corners::Vertex {
            self.told().corners = Corners::Vertex;
        }
        // A Corner4 cell left with no corner takes slot 0's tile, so only the others empty.
        let kind = self.terrain(tileset, set).kind();
        if clear && shape == TerrainShape::Brush && kind != TerrainKind::Corner4 {
            self.told().cleared.extend_from_slice(at);
        }
    }

    /// Places the tile at `place` in the tileset by hand on a cell: a terrain tile is pinned.
    fn place(&mut self, place: usize, (x, y): (i32, i32)) {
        let tileset = self.told().tileset;
        let tile = self
            .editor
            .doc()
            .tileset(tileset)
            .expect("a tileset")
            .tiles()[place]
            .id;
        self.editor.set_current_tile(tileset, tile).expect("a tile");
        let path = vec![(x * TILE + 1, y * TILE + 1)];
        let tool = TileTool::Place;
        self.editor
            .apply(TileCmd::Stroke { tool, path })
            .expect("placed");
        self.told().pinned.push((x, y));
    }

    fn done(self) -> (Document, String) {
        let mut doc = self.editor.doc().clone();
        doc.layers_mut()[0].visible = false;
        for t in &self.told {
            doc.layers_mut()[t.index].name = t.layer.clone();
        }
        let text = told_json(&doc, &self.told);
        (doc, text)
    }
}

fn kind_name(kind: TerrainKind) -> &'static str {
    match kind {
        TerrainKind::Edge4 => "edge4",
        TerrainKind::Corner4 => "corner4",
        TerrainKind::Blob47 => "blob47",
    }
}

fn cells_json(cells: &[(i32, i32)]) -> String {
    let each: Vec<String> = cells.iter().map(|(x, y)| format!("[{x}, {y}]")).collect();
    format!("[{}]", each.join(", "))
}

/// The sidecar: what the TMJ cannot say.
fn told_json(doc: &Document, told: &[Told]) -> String {
    let mut layers = Vec::new();
    for t in told {
        let set = doc.tileset(t.tileset).expect("a tileset");
        let place = |tile: TileId| set.tiles().iter().position(|e| e.id == tile);
        let mut sets = Vec::new();
        for terrain in set.terrains() {
            let slots: Vec<String> = terrain
                .slots()
                .iter()
                .map(|s| {
                    s.and_then(place)
                        .map_or("null".to_owned(), |p| p.to_string())
                })
                .collect();
            let kind = kind_name(terrain.kind());
            sets.push(format!(
                "{{\"kind\": \"{kind}\", \"slots\": [{}]}}",
                slots.join(", ")
            ));
        }
        let corners = match t.corners {
            Corners::Cell => "cell",
            Corners::Vertex => "vertex",
        };
        let mut one = String::new();
        let _ = write!(
            one,
            "{{\"layer\": \"{}\", \"tileset\": \"{}\", \"corners\": \"{corners}\", \"sets\": [{}], \"pinned\": {}, \"cleared\": {}}}",
            t.layer,
            set.name(),
            sets.join(", "),
            cells_json(&t.pinned),
            cells_json(&t.cleared)
        );
        layers.push(one);
    }
    format!("{{\"layers\": [\n  {}\n]}}\n", layers.join(",\n  "))
}

/// Edge4 from a template: a brush walk, a rectangle, a clearing stroke, and a pinned tile whose
/// neighbours were painted after it.
fn edge4() -> Case {
    let mut c = Case::new();
    let ts = c.map("Roads", 2);
    let set = c.set(ts, TerrainKind::Edge4, true);
    let (b, cell) = (TerrainShape::Brush, Corners::Cell);
    c.place(2 + 6, (9, 7));
    c.paint(set, b, false, cell, &[(1, 1), (6, 1), (6, 5), (2, 5)]);
    c.paint(set, TerrainShape::Rect, false, cell, &[(8, 1), (10, 4)]);
    c.paint(set, b, true, cell, &[(9, 2)]);
    c.paint(
        set,
        b,
        false,
        cell,
        &[(8, 6), (10, 6), (10, 8), (8, 8), (8, 7)],
    );
    c
}

/// Blob47 from a template: a fill of the whole map, holes cleared, a pin among them.
fn blob47_pinned() -> Case {
    let mut c = Case::new();
    let ts = c.map("Cave", 1);
    let set = c.set(ts, TerrainKind::Blob47, true);
    let (b, cell) = (TerrainShape::Brush, Corners::Cell);
    c.place(1 + 20, (5, 5));
    c.paint(set, TerrainShape::Fill, false, cell, &[(0, 0)]);
    c.paint(set, b, true, cell, &[(2, 2), (3, 2), (3, 3)]);
    c.paint(set, b, true, cell, &[(8, 1), (8, 3)]);
    c.paint(set, b, true, cell, &[(4, 4), (4, 6)]);
    c.paint(set, b, true, cell, &[(10, 8)]);
    c
}

/// Corner4 from a template painted by whole cells, then partly cleared.
fn corner4_cells() -> Case {
    let mut c = Case::new();
    let ts = c.map("Grass", 0);
    let set = c.set(ts, TerrainKind::Corner4, true);
    let cell = Corners::Cell;
    c.paint(set, TerrainShape::Rect, false, cell, &[(1, 1), (5, 4)]);
    c.paint(
        set,
        TerrainShape::Brush,
        false,
        cell,
        &[(7, 6), (10, 6), (10, 8)],
    );
    c.paint(set, TerrainShape::Brush, true, cell, &[(3, 2)]);
    c
}

/// Corner4 from a template painted by single corners, some cleared.
fn corner4_vertex() -> Case {
    let mut c = Case::new();
    let ts = c.map("Water", 0);
    let set = c.set(ts, TerrainKind::Corner4, true);
    let v = Corners::Vertex;
    c.paint(
        set,
        TerrainShape::Brush,
        false,
        v,
        &[(1, 1), (5, 1), (5, 4)],
    );
    c.paint(set, TerrainShape::Brush, false, v, &[(8, 3), (8, 8)]);
    c.paint(set, TerrainShape::Brush, true, v, &[(5, 2)]);
    c
}

/// Sets with missing slots, on drawn tiles: an Edge4 set with five slots, one tile in two of
/// them, and a Blob47 set with six, painted side by side on one layer.
fn missing_slots() -> Case {
    let mut c = Case::new();
    let ts = c.map("Patchy", 8);
    let edge = c.set(ts, TerrainKind::Edge4, false);
    c.assign(ts, edge, &[(0, 0), (5, 1), (10, 2), (15, 3), (6, 1)]);
    let blob = c.set(ts, TerrainKind::Blob47, false);
    c.assign(
        ts,
        blob,
        &[(0, 4), (1, 5), (16, 6), (46, 7), (20, 4), (33, 5)],
    );
    let (b, cell) = (TerrainShape::Brush, Corners::Cell);
    c.paint(edge, TerrainShape::Rect, false, cell, &[(0, 0), (4, 4)]);
    c.paint(edge, b, true, cell, &[(2, 2)]);
    c.paint(edge, b, false, cell, &[(1, 7), (4, 7), (4, 9)]);
    c.paint(blob, TerrainShape::Rect, false, cell, &[(6, 0), (11, 5)]);
    c.paint(blob, b, true, cell, &[(8, 2), (9, 3)]);
    c.paint(blob, b, false, cell, &[(7, 8), (11, 8)]);
    c
}

/// Two template sets of two kinds on one tileset and one layer, touching.
fn two_sets() -> Case {
    let mut c = Case::new();
    let ts = c.map("Mixed", 0);
    let edge = c.set(ts, TerrainKind::Edge4, true);
    let blob = c.set(ts, TerrainKind::Blob47, true);
    let cell = Corners::Cell;
    c.paint(edge, TerrainShape::Rect, false, cell, &[(0, 0), (5, 6)]);
    c.paint(blob, TerrainShape::Rect, false, cell, &[(6, 2), (11, 9)]);
    c.paint(edge, TerrainShape::Brush, true, cell, &[(2, 3)]);
    c.paint(blob, TerrainShape::Brush, true, cell, &[(9, 5)]);
    c
}

fn cases() -> Vec<(&'static str, Case)> {
    vec![
        ("autotile-edge4", edge4()),
        ("autotile-blob47-pinned", blob47_pinned()),
        ("autotile-corner4-cells", corner4_cells()),
        ("autotile-corner4-vertex", corner4_vertex()),
        ("autotile-missing-slots", missing_slots()),
        ("autotile-two-sets", two_sets()),
    ]
}

#[test]
fn every_case_keeps_the_map_rules_and_its_pins() {
    for (name, case) in cases() {
        let pins: Vec<(usize, (i32, i32))> = case
            .told
            .iter()
            .flat_map(|t| t.pinned.iter().map(move |&c| (t.index, c)))
            .collect();
        let (doc, text) = case.done();
        doc.check_tilemaps()
            .unwrap_or_else(|f| panic!("{name}: {f}"));
        for (layer, (x, y)) in pins {
            let bits = doc.layers()[layer].cel(0).get(x, y);
            let cell = pikslar_core::tilemap::MapCell::from_bits(bits);
            assert!(cell.is_pinned(), "{name}: ({x}, {y}) holds {bits:#x}");
        }
        let back = piks::load(&piks::save(&doc)).expect("opens");
        assert_eq!(back.tilesets.len(), doc.tilesets.len(), "{name}");
        let mut compared = 0;
        for (a, b) in doc.tilesets.iter().zip(back.tilesets.iter()) {
            let (a, b) = ((a.name(), a.terrains()), (b.name(), b.terrains()));
            assert_eq!(a, b, "{name}: the terrain sets of a tileset after a save");
            compared += a.1.len();
        }
        assert!(compared > 0, "{name}: no terrain set was compared");
        assert!(text.contains("\"slots\""), "{name}");
    }
}

#[test]
#[ignore = "writes the autotiled projects for spikes/check-autotile.py and check-tmj.py"]
fn write_autotile_fixtures() {
    let Some(dir) = std::env::var_os("PIKSLAR_AUTOTILE_FIXTURES") else {
        panic!("set PIKSLAR_AUTOTILE_FIXTURES to a folder");
    };
    let dir = PathBuf::from(dir);
    fs::create_dir_all(&dir).expect("folder");
    for (name, case) in cases() {
        let (doc, text) = case.done();
        fs::write(dir.join(format!("{name}.piks")), piks::save(&doc)).expect("project");
        fs::write(dir.join(format!("{name}.terrain.json")), text).expect("sidecar");
    }
}
