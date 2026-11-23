//! The layout suite for every preset (docs/ui-redesign.md 5.3): each common screen, four
//! document states and a tilemap, at the scale the display gives and at 2x where it fits.

use crate::app::App;
use crate::layout_tests::{
    add_map, assert_context_bar, assert_inside_window, assert_no_overlaps, draw, SCREENS,
};
use crate::panel_bottom::ID_TRANSPORT;
use crate::panel_matrix::ID_MATRIX;
use crate::panel_side::{ID_FILE, ID_TOOL};
use crate::workspace::model::{Preset, Side};
use pikslar_core::PaletteCmd;
use pikslar_platform::interface_scale;
use pikslar_render::Rect;
use pikslar_ui::role::Role;

/// Layers, swatches, Pixel Lint on, and a tilemap layer of that many tiles.
const STATES: [(usize, usize, bool, Option<usize>); 5] = [
    (1, 0, false, None),
    (2, 0, false, None),
    (5, 0, false, None),
    (30, 60, true, None),
    (3, 20, false, Some(40)),
];

fn shown(preset: Preset, (w, h, scale): (i32, i32, i32), state: usize) -> App {
    let (layers, swatches, lint, tiles) = STATES[state];
    let mut a = App::launch(None, Some(32), None);
    a.resized(w, h, scale);
    a.choose_preset(preset);
    for _ in 1..layers {
        a.layer_action(0);
    }
    if let Some(tiles) = tiles {
        add_map(&mut a, tiles);
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
    draw(&mut a, w, h);
    a
}

fn check(preset: Preset, name: &str, size: (i32, i32, i32), state: usize) {
    let (w, h, scale) = size;
    let a = shown(preset, size, state);
    let label = format!("{preset:?}, {name} ({w}x{h}, scale {scale}), state {state}");
    let container = |id: u32| {
        a.ui.kind_of(id)
            .is_some_and(|k| matches!(k.role, Role::TabList | Role::Group | Role::Splitter))
    };
    let widgets: Vec<(Rect, u32)> =
        a.ui.widgets()
            .iter()
            .copied()
            .filter(|&(_, id)| !container(id))
            .collect();
    assert_inside_window(&label, &widgets, w, h);
    assert_no_overlaps(&label, &widgets);
    assert_context_bar(&a, &label, &widgets);
    let has = |id: u32| widgets.iter().any(|&(_, seen)| seen == id);
    let wanted = (0..crate::menu_bar::MENUS as u32)
        .map(|i| ID_FILE + i)
        .chain((0..11).map(|i| ID_TOOL + i))
        .chain((0..7).map(|i| ID_TRANSPORT + i));
    for id in wanted {
        assert!(has(id), "{label}: control {id} is missing");
    }
    let canvas = a.layout.canvas;
    assert!(
        canvas.w >= 280 && canvas.h >= 280,
        "{label}: canvas {canvas:?}"
    );
    let rows = a.layout.matrix_rows(scale);
    assert_eq!(has(ID_MATRIX), rows > 0, "{label}: {rows} matrix rows");
    for side in Side::ALL {
        let placed = a.layout.solved.area(side);
        for slot in placed.slots().iter().filter(|s| s.open) {
            let r = slot.rect;
            let inside = r.x >= 0 && r.y >= 0 && r.right() <= w && r.bottom() <= h;
            assert!(inside, "{label}: {side:?} slot {r:?}");
        }
    }
}

#[test]
fn every_preset_keeps_the_interface_whole_on_every_common_screen() {
    for preset in Preset::BUILT_IN {
        for (name, dpi, w, h) in SCREENS {
            let scale = interface_scale(dpi, w, h);
            for state in 0..STATES.len() {
                check(preset, name, (w, h, scale), state);
            }
        }
    }
}

#[test]
fn every_preset_keeps_the_interface_whole_at_2x_where_it_fits() {
    for preset in Preset::BUILT_IN {
        for (name, _, w, h) in SCREENS {
            if w / 2 < 640 || h / 2 < 480 {
                continue;
            }
            for state in [0, 3, 4] {
                check(preset, name, (w, h, 2), state);
            }
        }
    }
}

#[test]
fn every_preset_gives_the_canvas_its_share_at_1366_by_705() {
    // docs/ui-redesign.md 3.6 and decision 6: about 53 % in Draw, above 52 % in Animate; the
    // context bar's 28 units took 3 points from Animate (phase 7's 24-unit menu bar gives 8 back).
    let least = [
        (Preset::Draw, 55),
        (Preset::Animate, 49),
        (Preset::Tiles, 55),
        (Preset::Palette, 40),
    ];
    for (preset, percent) in least {
        let a = shown(preset, (1366, 705, 1), 0);
        let c = a.layout.canvas;
        let share = c.w * c.h * 100 / (1366 * 705);
        assert!(share >= percent, "{preset:?}: canvas {share} %");
    }
}

#[test]
fn switching_presets_back_and_forth_gives_the_same_layout() {
    let size = (1366, 705, 1);
    let mut a = shown(Preset::Draw, size, 1);
    let first = a.layout;
    for preset in [
        Preset::Animate,
        Preset::Tiles,
        Preset::Palette,
        Preset::Draw,
    ] {
        a.choose_preset(preset);
        draw(&mut a, size.0, size.1);
    }
    assert_eq!(a.layout, first);
}

#[test]
fn compacting_color_keeps_the_palette_open_where_that_alone_makes_room() {
    // 1400 by 1050 at 2x: the full Color, the Palette and Layers do not fit, the compact
    // Color with them does, as before the docks.
    let a = shown(Preset::Draw, (1384, 1075, 2), 2);
    let right = a.layout.solved.area(Side::Right);
    assert!(a.layout.solved.effective.color_compact, "Color compact");
    assert!(right.slots().iter().all(|s| s.open), "{:?}", right.slots());
}

/// The Layers header's title stays inside its header: the gap between the header and the
/// first layer tool is plain panel, at every screen of the suite, in both presets with Layers.
#[test]
fn the_layers_title_never_runs_under_its_tools() {
    use crate::panel_right::{ID_LAYER_TOOLS, ID_SECTION};
    let mut checked = 0;
    for preset in [Preset::Draw, Preset::Tiles] {
        for (name, dpi, w, h) in SCREENS {
            let scale = interface_scale(dpi, w, h);
            let mut a = shown(preset, (w, h, scale), 1);
            let mut px = vec![0u32; (w * h) as usize];
            let mut sf = pikslar_render::Surface { px: &mut px, w, h };
            a.draw_interface(
                &mut sf,
                pikslar_platform::Window::detached(),
                &mut Vec::new(),
            );
            let found = |id: u32| {
                a.ui.widgets()
                    .iter()
                    .rev()
                    .find(|&&(_, w)| w == id)
                    .map(|&(r, _)| r)
            };
            let (Some(header), Some(tool)) = (found(ID_SECTION + 2), found(ID_LAYER_TOOLS + 6))
            else {
                continue;
            };
            let gap = Rect::new(header.right(), header.y, tool.x - header.right(), header.h);
            let ground = px[(gap.y * w + gap.x) as usize];
            let plain = (gap.y..gap.bottom())
                .flat_map(|y| (gap.x..gap.right()).map(move |x| (x, y)))
                .all(|(x, y)| px[(y * w + x) as usize] == ground);
            assert!(plain, "{preset:?}, {name}: the title runs into {gap:?}");
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "no screen drew the Layers header beside its tools"
    );
}
