//! Draws the whole interface at the window sizes and display scales people really have, and
//! checks that nothing overlaps, runs off the window or disappears.

use crate::app::App;
use crate::context_bar::{ID_MORE, ID_STEPPER};
use crate::panel_bottom::ID_TRANSPORT;
use crate::panel_matrix::ID_MATRIX;
use crate::panel_right::{ID_COLOR, ID_LAYER_ROW, ID_LAYER_TOOLS, ID_SECTION};
use crate::panel_side::{ID_BRUSH, ID_FILE, ID_TOOL};
use crate::panel_tiles::{ID_TILE, ID_TILES};
use crate::workspace::model::{Panel, Side};
use pikslar_core::PaletteCmd;
use pikslar_platform::{interface_scale, Button, Mods, Window};
use pikslar_render::{Rect, Surface};

/// Client sizes in physical pixels, as Windows reports them for a per-monitor-aware program.
pub(crate) const SCREENS: [(&str, i32, i32, i32); 23] = [
    ("1366x768 100% maximized", 96, 1366, 705),
    ("1366x768 100% restored", 96, 1213, 652),
    ("1920x1080 100% maximized", 96, 1920, 1017),
    ("1920x1080 100% restored", 96, 1384, 861),
    ("1920x1080 125% maximized", 120, 1920, 1009),
    ("1920x1080 125% restored", 120, 1380, 855),
    ("1920x1080 150% maximized", 144, 1920, 1003),
    ("1920x1080 150% restored", 144, 1712, 933),
    ("1920x1080 150% half screen", 144, 960, 1003),
    ("1920x1080 200% maximized", 192, 1920, 993),
    ("1920x1080 200% restored", 192, 1696, 909),
    ("2560x1440 100% maximized", 96, 2560, 1377),
    ("2560x1440 100% restored", 96, 1384, 861),
    ("2560x1440 150% maximized", 144, 2560, 1363),
    ("2560x1440 150% restored", 144, 2288, 1257),
    ("2256x1504 150% maximized", 144, 2256, 1433),
    ("3840x2160 150% maximized", 144, 3840, 2083),
    ("3840x2160 200% maximized", 192, 3840, 2073),
    ("3840x2160 300% maximized", 288, 3840, 2050),
    ("Exactly enough room for 2x", 192, 2000, 1200),
    ("Just short of room for 2x", 192, 1999, 1199),
    ("Smallest window at 100%", 96, 624, 561),
    ("Smallest window at 300%", 288, 600, 500),
];

/// What is chosen by hand in Draw's right dock: the Color, Palette and third slots folded, and
/// Pixel Lint shown in the third slot in place of Layers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Folds {
    color: bool,
    palette: bool,
    layers: bool,
    lint: bool,
}

impl Folds {
    const ALL: Folds = Folds {
        color: true,
        palette: true,
        layers: true,
        lint: true,
    };

    fn apply(self, a: &mut App) {
        use crate::workspace::model::{Side, Slot};
        a.edit_layout(|ws| {
            let open = [!self.color, !self.palette, !self.layers];
            let area = *ws.area(Side::Right);
            let area = (0..3).fold(area, |area, i| {
                let sl = area.slots()[i];
                let active = if i == 2 && self.lint { 2 } else { 0 };
                area.with_slot(
                    i,
                    Slot {
                        open: open[i],
                        active,
                        ..sl
                    },
                )
            });
            ws.with_area(Side::Right, area)
        });
    }
}

const STATES: [(usize, usize, bool); 4] =
    [(1, 0, false), (2, 0, false), (5, 0, false), (30, 60, true)];

fn check(name: &str, dpi: i32, w: i32, h: i32, state: (usize, usize, bool)) {
    let scale = interface_scale(dpi, w, h);
    for bits in 0..16 {
        let folds = Folds {
            color: bits & 1 != 0,
            palette: bits & 2 != 0,
            layers: bits & 4 != 0,
            lint: bits & 8 != 0,
        };
        check_at(name, (w, h, scale), state, folds);
    }
}

fn check_at(name: &str, size: (i32, i32, i32), state: (usize, usize, bool), folds: Folds) {
    check_with(name, size, state, folds, None);
}

/// No terrain set, a terrain set (so the terrain row), or a terrain set with its slots shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Terrain {
    None,
    Row,
    Slots,
}

/// As `check_at`, with a tilemap layer of `tiles` tiles on top, active, when `tiles` is some,
/// and its tileset's terrain sets as `Terrain` says.
fn check_with(
    name: &str,
    (w, h, scale): (i32, i32, i32),
    state: (usize, usize, bool),
    folds: Folds,
    map: Option<(usize, Terrain)>,
) {
    let (tiles, terrain) = (map.map(|m| m.0), map.map_or(Terrain::None, |m| m.1));
    let (layers, swatches, lint) = state;
    let mut a = App::launch(None, Some(32), None);
    a.resized(w, h, scale);
    folds.apply(&mut a);
    for _ in 1..layers {
        a.layer_action(0);
    }
    let layers = layers + usize::from(tiles.is_some());
    if let Some(tiles) = tiles {
        add_map(&mut a, tiles);
    }
    if terrain != Terrain::None {
        a.new_terrain(2);
        a.tile_ui.terrain.slots = terrain == Terrain::Slots;
    }
    for i in 0..swatches {
        a.editor
            .apply(PaletteCmd::AddSwatch {
                color: 0xff00_0000 | i as u32,
            })
            .expect("room");
    }
    if lint {
        a.toggle_lint();
    }
    let label = format!(
        "{name} ({w}x{h}, scale {scale}, {layers} layers, lint {lint}, tiles {tiles:?}, {terrain:?}, {folds:?})"
    );
    assert_whole(&mut a, &label, (w, h), (layers, folds));
    // Tiles shows beside the Palette, so the Palette's slot folded by hand folds it too.
    if let Some(tiles) = tiles.filter(|_| !folds.palette) {
        // In units: Draw's full Color and Layers leave Tiles a row and its terrain row only
        // from 628 units (600 and the context bar's 28), which 1366x768 has and 1080p at 2x has not.
        let tiny = h / scale < 628;
        let cramped = tiny || lint || folds != Folds::default();
        assert_map(&a, &label, (tiles, terrain), (tiny, cramped));
    }
    // Again with the cel matrix open under the strip.
    a.toggle_matrix();
    assert!(a.matrix.open);
    assert_whole(
        &mut a,
        &format!("{label}, matrix open"),
        (w, h),
        (layers, folds),
    );
    let rows = a.layout.matrix_rows(scale) as usize;
    let cells = a.ui.widgets().iter().any(|&(_, id)| id == ID_MATRIX);
    assert_eq!(cells, rows > 0, "{label}: {rows} matrix rows");
}

/// The Tiles section, its terrain row and slots, and a row of thumbnails or slots unless the
/// panel is `cramped` (under 520 units tall, which is `tiny`, or something folded by hand).
fn assert_map(
    a: &App,
    label: &str,
    (tiles, terrain): (usize, Terrain),
    (tiny, cramped): (bool, bool),
) {
    if terrain != Terrain::None {
        assert_terrain(a, label, terrain, tiny);
    }
    let grid = match terrain {
        Terrain::Slots => a.tile_ui.terrain.grid,
        _ => {
            assert_tiles(a, label, tiles);
            a.tile_ui.grid
        }
    };
    let rows = grid.map_or(0, |g| g.rows);
    assert!(rows > 0 || cramped, "{label}: no row of tiles");
}

fn assert_whole(a: &mut App, label: &str, (w, h): (i32, i32), (layers, folds): (usize, Folds)) {
    draw(a, w, h);
    // A container (a tab strip's box) holds the controls inside it rather than overlapping them,
    // and a splitter's reach yields to the controls beside its gutter (docs/ui-redesign.md 2.5).
    let container = |id: u32| {
        a.ui.kind_of(id).is_some_and(|k| {
            use pikslar_ui::role::Role;
            matches!(k.role, Role::TabList | Role::Group | Role::Splitter)
        })
    };
    let widgets: Vec<(Rect, u32)> =
        a.ui.widgets()
            .iter()
            .copied()
            .filter(|&(_, id)| !container(id))
            .collect();
    assert_inside_window(label, &widgets, w, h);
    assert_no_overlaps(label, &widgets);
    let tiles = a.active_tileset().is_some();
    // A folded Palette is its tab strip alone, by hand or for room.
    let palette_folded = [Side::Left, Side::Right].into_iter().any(|side| {
        let placed = a.layout.solved.area(side);
        placed
            .slots()
            .iter()
            .any(|s| s.panel == Some(Panel::Palette) && !s.open)
    });
    let folds = Folds {
        palette: folds.palette || palette_folded,
        ..folds
    };
    // In Large text a window under about 490 units tall cannot hold the right dock (its slots'
    // tab strips grow with the words), so it is a rail, whose icons reach the panels.
    let railed = pikslar_ui::font::face() == pikslar_ui::font::Face::Mono
        && a.layout.solved.area(Side::Right).rail;
    assert_essentials(label, &widgets, layers, (folds, tiles, railed));
    assert_context_bar(a, label, &widgets);
    let active = ID_LAYER_ROW + a.editor.layer as u32;
    assert!(
        folds.lint || railed || widgets.iter().any(|&(_, id)| id == active),
        "{label}: the active layer's row is not shown"
    );
    let canvas = a.layout.canvas;
    assert!(
        canvas.w >= 280 && canvas.h >= 280,
        "{label}: canvas {canvas:?}"
    );
}

pub(crate) fn draw(a: &mut App, w: i32, h: i32) {
    let mut px = vec![0u32; (w * h) as usize];
    let mut surface = Surface { px: &mut px, w, h };
    let mut dirty = Vec::new();
    a.draw_interface(&mut surface, Window::detached(), &mut dirty);
}

pub(crate) fn assert_inside_window(label: &str, widgets: &[(Rect, u32)], w: i32, h: i32) {
    for (r, id) in widgets {
        let inside = r.x >= 0 && r.y >= 0 && r.right() <= w && r.bottom() <= h;
        assert!(inside, "{label}: widget {id} at {r:?} leaves the window");
    }
}

pub(crate) fn assert_no_overlaps(label: &str, widgets: &[(Rect, u32)]) {
    for (i, (a, id_a)) in widgets.iter().enumerate() {
        for (b, id_b) in &widgets[i + 1..] {
            let shared = a.intersect(*b);
            // Neighbouring segments of one control (consecutive ids) share an edge unit, whatever
            // the scale (1 to 3).
            let edge = id_a.abs_diff(*id_b) == 1 && shared.w <= 3 && shared.h == a.h && a.h == b.h;
            assert!(
                shared.is_empty() || edge,
                "{label}: widgets {id_a} {a:?} and {id_b} {b:?} overlap"
            );
        }
    }
}

/// Every tool, the brush size, file actions and the shown panels' fold headers are always
/// there, with the layer tools while Layers is shown; the color sliders and the active layer's
/// settings unless their slots are folded. Tiles shows in the Palette's slot on a tilemap layer.
fn assert_essentials(
    label: &str,
    widgets: &[(Rect, u32)],
    layers: usize,
    (folds, tiles, railed): (Folds, bool, bool),
) {
    let has = |id: u32| widgets.iter().any(|&(_, seen)| seen == id);
    let mut wanted: Vec<u32> = Vec::new();
    wanted.extend((0..crate::menu_bar::MENUS as u32).map(|i| ID_FILE + i));
    wanted.extend((0..11).map(|i| ID_TOOL + i));
    // The pencil's settings, in the context bar: the size stepper always, the rest unless the
    // bar was too narrow, when they are in its More menu.
    wanted.extend((0..3).map(|i| ID_STEPPER + i));
    // The right dock's controls, which a rail leaves to its icons.
    if !railed {
        wanted.push(ID_SECTION);
        if !tiles && !folds.palette {
            wanted.push(ID_SECTION + 1);
        }
        if folds.lint {
            wanted.push(ID_SECTION + 3);
        } else {
            wanted.push(ID_SECTION + 2);
            wanted.extend((0..4).map(|i| ID_LAYER_TOOLS + i));
        }
        if !folds.color {
            wanted.extend((0..4).map(|i| ID_COLOR + i));
        }
        if !folds.layers && !folds.lint {
            wanted.extend((10..13).map(|i| ID_LAYER_TOOLS + i));
        }
    }
    wanted.extend((0..7).map(|i| ID_TRANSPORT + i));
    for id in wanted {
        assert!(has(id), "{label}: control {id} is missing");
    }
    if folds.lint && folds.layers {
        let first_row = crate::panel_lint::ID_LINT;
        assert!(!has(first_row), "{label}: Pixel Lint's rows stay folded");
    }
    if folds.lint || railed {
        return;
    }
    let rows = widgets
        .iter()
        .filter(|&&(_, id)| (ID_LAYER_ROW..ID_LAYER_ROW + 1_000).contains(&id))
        .count();
    assert!(rows >= layers.min(2), "{label}: only {rows} layer rows");
}

/// The context bar holds its controls, shows the brush size, and, where it is wide enough,
/// every setting of the pencil; where it is not, the rest is behind More.
pub(crate) fn assert_context_bar(a: &App, label: &str, widgets: &[(Rect, u32)]) {
    let bar = a.layout.context;
    let has = |id: u32| widgets.iter().any(|&(_, seen)| seen == id);
    let own = |id: u32| {
        (ID_STEPPER..ID_STEPPER + 3).contains(&id)
            || (ID_BRUSH + 2..ID_BRUSH + 5).contains(&id)
            || id == ID_MORE
            || (crate::panel_tiles::ID_TILES - 1..crate::panel_tiles::ID_TILES + 2).contains(&id)
    };
    for &(r, id) in widgets.iter().filter(|&&(_, id)| own(id)) {
        assert!(
            bar.contains(r.x, r.y) && r.right() <= bar.right() && r.bottom() <= bar.bottom(),
            "{label}: bar control {id} at {r:?} leaves {bar:?}"
        );
    }
    let settings = [ID_BRUSH + 2, ID_BRUSH + 3, ID_BRUSH + 4];
    let all = settings.iter().all(|&id| has(id));
    assert!(
        all || has(ID_MORE),
        "{label}: settings neither shown nor under More"
    );
    if a.active_tileset().is_some() {
        assert!(
            has(ID_TILES) && has(ID_TILES + 1),
            "{label}: no tile switch"
        );
    }
}

/// A tilemap layer on top, active, whose new tileset of 16x16 tiles has `tiles` tiles.
pub(crate) fn add_map(a: &mut App, tiles: usize) {
    use pikslar_core::{ColorMode, TileCmd, TileSource};
    let at = a.editor.doc().layers().len();
    let made = a
        .editor
        .apply(TileCmd::NewMapLayer {
            at,
            name: "Map".into(),
            mode: ColorMode::Rgba,
            tile_w: 16,
            tile_h: 16,
        })
        .expect("room");
    let tileset = pikslar_core::tileset::TilesetId::new(made.created.expect("a tileset"));
    for _ in 0..tiles {
        let from = TileSource::Blank;
        a.editor
            .apply(TileCmd::AddTile { tileset, from })
            .expect("room");
    }
}

/// The Tiles section's mode buttons, and at least one row of thumbnails when there are tiles.
fn assert_tiles(a: &App, label: &str, tiles: usize) {
    let widgets = a.ui.widgets();
    let has = |id: u32| widgets.iter().any(|&(_, seen)| seen == id);
    assert!(
        has(ID_TILES) && has(ID_TILES + 1),
        "{label}: no mode switch in the context bar"
    );
    let thumbs = widgets
        .iter()
        .filter(|w| w.1 >= ID_TILE && w.1 < ID_TILE + 10_000);
    let shown = thumbs.count();
    let rows = a.tile_ui.grid.map_or(0, |g| g.rows);
    assert!(
        rows == 0 || shown >= tiles.min(4),
        "{label}: {shown} thumbnails of {tiles}"
    );
}

/// The terrain row's two buttons, and the slots in place of the tiles when they are shown.
fn assert_terrain(a: &App, label: &str, terrain: Terrain, tiny: bool) {
    use crate::terrain_ui::{ID_SLOT, ID_TERRAIN};
    let widgets = a.ui.widgets();
    let has = |id: u32| widgets.iter().any(|&(_, seen)| seen == id);
    let row = a.tile_ui.terrain.row_shown;
    assert_eq!(
        has(ID_TERRAIN) && has(ID_TERRAIN + 1),
        row,
        "{label}: the terrain row"
    );
    // Only the smallest window, at 300%, leaves the row out: it shows only the Tiles header.
    assert!(row || tiny, "{label}: no room for the terrain row");
    let rows = a.tile_ui.terrain.grid.map_or(0, |g| g.rows);
    let slots = terrain == Terrain::Slots && rows > 0;
    assert_eq!(has(ID_SLOT), slots, "{label}: slots");
    assert!(!slots || !has(ID_TILE), "{label}: tiles under the slots");
}

fn sprite_offset(a: &App) -> (i32, i32) {
    let c = a.layout.canvas;
    (c.x + c.w / 2 - a.view.x, c.y + c.h / 2 - a.view.y)
}

#[test]
fn resizing_keeps_the_sprite_in_place_within_the_canvas() {
    let mut a = App::launch(None, Some(32), None);
    a.resized(1400, 900, 1);
    let before = sprite_offset(&a);
    for (w, h, scale) in [
        (1920, 1009, 1),
        (2560, 1369, 2),
        (640, 600, 1),
        (1400, 900, 1),
    ] {
        a.resized(w, h, scale);
        assert_eq!(sprite_offset(&a), before, "{w}x{h} at {scale}");
    }
}

#[test]
fn a_stroke_in_progress_keeps_its_place_when_the_window_changes() {
    let mut a = App::launch(None, Some(32), None);
    a.resized(1920, 1009, 1);
    let c = a.layout.canvas;
    a.pointer_down(c.x + c.w / 2, c.y + c.h / 2, Button::Left, Mods::default());
    let view = (a.view.x, a.view.y);
    a.resized(960, 1009, 1);
    assert_eq!((a.view.x, a.view.y), view);
}

#[test]
fn another_document_starts_with_its_lists_at_the_top() {
    let mut a = App::launch(None, Some(32), None);
    let area = Rect::new(0, 0, 100, 100);
    a.panel_scroll.palette_window(area, 1, 100, None);
    assert!(a.panel_scroll.wheel(-3, 5, 5));
    assert_eq!(a.panel_scroll.palette_window(area, 1, 100, None), 3);
    a.replace_document(crate::app::starter_document(32), None);
    assert_eq!(a.panel_scroll.palette_window(area, 1, 100, None), 0);
}

#[test]
fn the_interface_scale_follows_the_display_only_while_it_fits() {
    let cases = [
        (96, 1920, 1017, 1),
        (120, 1920, 1009, 1),
        (144, 1920, 1003, 2),
        (144, 1366, 705, 1),
        (144, 2560, 1363, 2),
        (168, 3840, 2083, 2),
        (192, 1920, 993, 2),
        (192, 3840, 2073, 2),
        (288, 3840, 2050, 3),
        (288, 1920, 1003, 2),
    ];
    for (dpi, w, h, expected) in cases {
        assert_eq!(interface_scale(dpi, w, h), expected, "{dpi} {w}x{h}");
    }
}

#[test]
fn every_common_screen_keeps_the_interface_whole() {
    for (name, dpi, w, h) in SCREENS {
        for state in STATES {
            check(name, dpi, w, h, state);
        }
    }
}

#[test]
fn every_common_screen_keeps_the_interface_whole_with_a_tilemap_layer() {
    for (name, dpi, w, h) in SCREENS {
        let scale = interface_scale(dpi, w, h);
        for bits in 0..16 {
            let folds = Folds {
                color: bits & 1 != 0,
                palette: bits & 2 != 0,
                layers: bits & 4 != 0,
                lint: bits & 8 != 0,
            };
            check_with(
                name,
                (w, h, scale),
                STATES[0],
                folds,
                Some((3, Terrain::None)),
            );
            check_with(
                name,
                (w, h, scale),
                STATES[3],
                folds,
                Some((60, Terrain::None)),
            );
        }
    }
}

#[test]
fn a_chosen_2x_on_a_full_hd_laptop_at_150_keeps_the_interface_whole() {
    for state in STATES {
        for folds in [Folds::default(), Folds::ALL] {
            check_at("1920x1080 150% chosen 2x", (1920, 1003, 2), state, folds);
            check_at(
                "1920x1080 150% chosen 2x, restored",
                (1712, 960, 2),
                state,
                folds,
            );
        }
    }
}

#[test]
fn any_window_size_keeps_the_interface_whole() {
    for dpi in [96, 144, 192] {
        for w in (624..=2624).step_by(250) {
            for h in (561..=1461).step_by(100) {
                check("sweep", dpi, w, h, STATES[1]);
                check("sweep", dpi, w, h, STATES[3]);
            }
        }
    }
}

#[test]
fn every_common_screen_keeps_the_interface_whole_with_a_terrain_set() {
    for (name, dpi, w, h) in SCREENS {
        let scale = interface_scale(dpi, w, h);
        for bits in [0, 1, 2, 4, 8, 15] {
            let folds = Folds {
                color: bits & 1 != 0,
                palette: bits & 2 != 0,
                layers: bits & 4 != 0,
                lint: bits & 8 != 0,
            };
            for terrain in [Terrain::Row, Terrain::Slots] {
                check_with(name, (w, h, scale), STATES[0], folds, Some((3, terrain)));
                check_with(name, (w, h, scale), STATES[3], folds, Some((60, terrain)));
            }
        }
    }
}

#[test]
fn only_neighbouring_segments_may_share_an_edge_unit() {
    let seg = |x| Rect::new(x, 0, 21, 20);
    assert_no_overlaps("segments", &[(seg(0), 51), (seg(20), 52)]);
    let caught = std::panic::catch_unwind(|| {
        assert_no_overlaps("strangers", &[(seg(0), 51), (seg(20), 90)]);
    });
    assert!(caught.is_err(), "unrelated widgets overlapping by a unit");
}

#[test]
fn every_common_screen_keeps_the_interface_whole_in_large_text() {
    crate::test_app::large_text();
    for (name, dpi, w, h) in SCREENS {
        for state in STATES {
            check(name, dpi, w, h, state);
        }
    }
}

#[test]
fn any_window_size_keeps_the_interface_whole_in_large_text() {
    crate::test_app::large_text();
    for dpi in [96, 144, 192] {
        for w in (624..=2624).step_by(500) {
            for h in (561..=1461).step_by(200) {
                check("sweep", dpi, w, h, STATES[1]);
            }
        }
    }
}

/// The least window height, in pixels, at which Draw's right dock is not a rail at 2x on a
/// 1624 pixel wide window with two layers.
fn least_height_without_a_rail(large: bool) -> i32 {
    let found = (300..1200).find(|&h| {
        let mut a = App::launch(None, Some(32), None);
        a.set_large_text(large);
        a.resized(1624, h, 2);
        a.layer_action(0);
        draw(&mut a, 1624, h);
        !a.layout.solved.area(Side::Right).rail
    });
    pikslar_ui::font::set_face(pikslar_ui::font::Face::Text);
    found.expect("some height holds the dock")
}

#[test]
fn large_text_folds_the_right_dock_to_a_rail_at_a_pinned_height() {
    let (normal, large) = (
        least_height_without_a_rail(false),
        least_height_without_a_rail(true),
    );
    // Three slots with 4 more units of tab strip each, and the Layers panel's doubled details:
    // 20 units (40 pixels at 2x) more. A change here changes where Large text gives way.
    assert_eq!((normal, large), (944, 984));
}

#[test]
fn the_solver_tests_default_strip_is_the_strips_least_height() {
    assert_eq!(
        crate::workspace::model::TAB_STRIP,
        pikslar_ui::tabs::STRIP_H
    );
    assert_eq!(
        crate::workspace::metrics::Metrics::LIGHTBOX.strip,
        pikslar_ui::tabs::STRIP_H
    );
}
