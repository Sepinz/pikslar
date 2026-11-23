//! Screen sheets of the tiles UI, drawn offscreen with the test tooling rather than in a window:
//! the Tiles section, the grid and a stroke in progress, the tile menu and TMJ in the export
//! panel, at three window sizes. Run with `PIKSLAR_SHEETS` naming a folder for the PNGs:
//! `cargo test -p pikslar tile_sheets -- --ignored`.

use crate::app::App;
use crate::test_app::{centre_of, map_app};
use pikslar_core::{Grid, TileCmd, TileSource, TileTool};
use pikslar_format::png;
use pikslar_platform::{Button, Mods, Window};
use pikslar_render::Surface;
use std::path::Path;
use std::sync::Arc;

const SIZES: [(i32, i32); 3] = [(1366, 705), (1920, 1017), (2560, 1377)];

/// Tiles of stripes and checks, so their thumbnails and their cells on the canvas read apart.
fn add_patterns(a: &mut App) {
    let set = a.active_tileset().expect("a map");
    for n in 0..9u32 {
        let cells: Vec<u32> = (0..16)
            .map(|i| {
                let (x, y) = (i % 4, i / 4);
                let on = (x + y * (n % 3 + 1)) % 2 == 0;
                if on {
                    0xff20_6040 + n * 0x0a10
                } else {
                    0xffd0_c090 - n * 0x0800
                }
            })
            .collect();
        let image = Arc::new(Grid::from_cells(4, 4, &cells).into_image());
        a.editor
            .apply(TileCmd::AddTile {
                tileset: set,
                from: TileSource::Image(image),
            })
            .expect("a tile");
    }
    let tiles: Vec<_> = a
        .editor
        .doc()
        .tileset(set)
        .expect("set")
        .tiles()
        .iter()
        .map(|t| t.id)
        .collect();
    for (i, &tile) in tiles.iter().enumerate() {
        a.editor.set_current_tile(set, tile).expect("chosen");
        let (x, y) = ((i as i32 % 6) * 4 + 1, (i as i32 / 6) * 4 + 1);
        a.editor
            .apply(TileCmd::Stroke {
                tool: TileTool::Place,
                path: vec![(x, y)],
            })
            .expect("placed");
    }
}

fn render(a: &mut App, (w, h): (i32, i32)) -> Vec<u32> {
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = Surface { px: &mut px, w, h };
    a.full_canvas = true;
    a.draw_interface(&mut surface, Window::detached(), &mut Vec::new());
    a.draw_canvas(&mut surface, &mut Vec::new());
    a.draw_interface(&mut surface, Window::detached(), &mut Vec::new());
    px
}

fn save(dir: &Path, name: &str, (w, h): (i32, i32), px: &[u32]) {
    let image = png::Image {
        width: w as usize,
        height: h as usize,
        px: px.iter().map(|p| p | 0xff00_0000).collect(),
    };
    let bytes = png::encode(&image).expect("encoded");
    std::fs::write(dir.join(format!("{name}-{w}x{h}.png")), bytes).expect("written");
}

#[test]
#[ignore = "writes screen sheets when PIKSLAR_SHEETS names a folder"]
fn tile_sheets() {
    let Some(dir) = std::env::var_os("PIKSLAR_SHEETS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("the folder");
    for size in SIZES {
        let (mut a, set) = map_app();
        a.resized(size.0, size.1, 1);
        add_patterns(&mut a);
        a.fit_view();
        render(&mut a, size);
        // A block of two chosen, and a stroke of it in progress.
        a.choose_tile(set, 4, false);
        a.choose_tile(set, 5, true);
        let (x, y) = crate::test_app::screen(&a, 5, 21);
        a.pointer_down(x, y, Button::Left, Mods::default());
        let (x2, y2) = crate::test_app::screen(&a, 25, 21);
        a.ui.track(x2, y2);
        a.canvas_move(x2, y2);
        save(&dir, "tiles-stroke", size, &render(&mut a, size));
        a.pointer_up(x2, y2, Button::Left);
        let at = centre_of(&a, crate::panel_tiles::ID_TILE + 1);
        a.pointer_down(at.0, at.1, Button::Right, Mods::default());
        save(&dir, "tiles-menu", size, &render(&mut a, size));
        a.close_menu();
        a.open_export_dialog(false);
        a.export_dialog = a.export_dialog.map(|c| c.with_map());
        save(&dir, "tiles-export", size, &render(&mut a, size));
    }
}

/// An Edge4 set with six of its 16 slots filled, the rest missing, chosen to paint with.
fn add_terrain(a: &mut App) -> (pikslar_core::tileset::TilesetId, u16) {
    let set = a.active_tileset().expect("a map");
    a.new_terrain(0);
    let terrain = a.tile_ui.terrain.chosen.expect("chosen").1;
    let ids: Vec<_> = a
        .editor
        .doc()
        .tileset(set)
        .expect("set")
        .tiles()
        .iter()
        .map(|t| t.id)
        .collect();
    for (slot, tile) in [0u8, 2, 8, 10, 5, 15].into_iter().zip(ids) {
        let command = pikslar_core::TerrainCmd::Assign {
            tileset: set,
            set: terrain,
            slot,
            tile,
        };
        a.editor.apply(command).expect("assigned");
    }
    (set, terrain)
}

#[test]
#[ignore = "writes screen sheets when PIKSLAR_SHEETS names a folder"]
fn terrain_sheets() {
    let Some(dir) = std::env::var_os("PIKSLAR_SHEETS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).expect("the folder");
    for size in SIZES {
        let (mut a, _) = map_app();
        a.resized(size.0, size.1, 1);
        add_patterns(&mut a);
        add_terrain(&mut a);
        a.fit_view();
        save(&dir, "terrain-slots", size, &render(&mut a, size));
        a.tile_ui.terrain.slots = false;
        // A terrain stroke in progress, its cells drawn before the commit.
        let (x, y) = crate::test_app::screen(&a, 5, 29);
        a.pointer_down(x, y, Button::Left, Mods::default());
        let (x2, y2) = crate::test_app::screen(&a, 21, 29);
        a.ui.track(x2, y2);
        a.canvas_move(x2, y2);
        save(&dir, "terrain-stroke", size, &render(&mut a, size));
        a.pointer_up(x2, y2, Button::Left);
        let row = a.tile_ui.terrain.row;
        a.open_terrain_menu((row.x, row.bottom()));
        save(&dir, "terrain-menu", size, &render(&mut a, size));
        a.close_menu();
    }
}
