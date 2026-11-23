//! The drag preview must look exactly like the compositor once the pixels land.

use super::*;
use crate::view::{blit_canvas, set_backdrop, Backdrop};
use pikslar_core::{rgba, BlendMode, Cel, ColorMode, Document, Grid, Mask, SelectOp, Tool};
use pikslar_core::{DrawCmd, FloatingCmd, SelectionCmd};
use std::sync::Arc;

pub(super) const SIZE: usize = 40;
const SCREEN: (i32, i32) = (130, 100);

/// Art with opaque, translucent and empty cells, so every blend path gets used.
pub(super) fn paint(e: &mut Editor, layer: usize, seed: u32) {
    e.layer = layer;
    e.pixel_perfect = false;
    for i in 0..260u32 {
        let n = i.wrapping_mul(2_654_435_761).wrapping_add(seed);
        let (x, y) = (
            (n >> 8) as i32 % SIZE as i32,
            (n >> 18) as i32 % SIZE as i32,
        );
        let alpha = [255, 255, 128, 40][(n & 3) as usize];
        e.color = rgba(n as u8 | 1, (n >> 3) as u8, (n >> 6) as u8, alpha);
        e.apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x,
            y,
        })
        .expect("begin");
        e.apply(DrawCmd::Commit).expect("commit");
    }
}

pub(super) fn scene(active: ColorMode) -> Editor {
    let mut doc = Document::new(SIZE, SIZE, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, active).expect("room");
    doc.add_layer(2, ColorMode::Rgba).expect("room");
    doc.add_layer(3, ColorMode::Palette).expect("room");
    let mut e = Editor::new(doc);
    for (layer, seed) in [(0, 11), (1, 222), (2, 3333), (3, 44444)] {
        paint(&mut e, layer, seed);
    }
    e.layer = 1;
    e
}

/// [`scene`] with a tilemap layer of translucent and opaque tiles, turned every way, put at
/// `at` from the bottom, its map offset off the grid of the canvas's tiles.
pub(super) fn scene_with_map(at: usize) -> Editor {
    let mut doc = scene(ColorMode::Rgba).doc().clone();
    let set = doc.add_tileset("T", ColorMode::Rgba, (8, 8)).expect("set");
    for color in [rgba(200, 40, 40, 255), rgba(40, 200, 90, 128)] {
        let mut px = vec![color; 64];
        px[1] = 0;
        px[10] = rgba(250, 250, 250, 255);
        let image = Grid::from_cells(8, 8, &px).into_image();
        doc.add_tile(set, Arc::new(image)).expect("tile");
    }
    doc.add_tilemap_layer(at, set).expect("layer");
    let cells: Vec<u32> = (0..30u32)
        .map(|i| {
            if i % 4 == 3 {
                0
            } else {
                (1 + i % 2) | (i % 8) << 29
            }
        })
        // Oblong turns would be refused, but the tiles are square.
        .collect();
    let image = Grid::from_cells(6, 5, &cells).into_image();
    doc.put_cel(at, 0, Cel::at(image, -5, 3).expect("in reach"))
        .expect("cel");
    Editor::new(doc)
}

fn select_and_lift(e: &mut Editor, copy: bool) {
    let shape = Mask::rect(6, 4, 22, 19).subtract(&Mask::rect(12, 10, 4, 3));
    e.apply(SelectionCmd::Select {
        shape: Box::new(shape),
        op: SelectOp::Replace,
    })
    .expect("select");
    assert!(e.apply(FloatingCmd::Lift { copy }).expect("lift").changed);
}

pub(super) fn blank() -> Vec<u32> {
    vec![0; (SCREEN.0 * SCREEN.1) as usize]
}

pub(super) fn surface(px: &mut [u32]) -> Surface<'_> {
    Surface {
        px,
        w: SCREEN.0,
        h: SCREEN.1,
    }
}

pub(super) fn area() -> Rect {
    Rect::new(3, 2, 120, 90)
}

/// What the compositor shows for the same state, the thing a preview must match.
pub(super) fn through_compositor(e: &mut Editor, comp: &mut Compositor, view: View) -> Vec<u32> {
    let mut px = blank();
    let mut changed = Vec::new();
    let damage = e.take_damage();
    comp.update(e, damage, &mut changed);
    let (w, h) = (comp.width() as i32, comp.height() as i32);
    blit_canvas(
        comp.pixels(),
        w,
        h,
        view,
        &mut surface(&mut px),
        area(),
        &mut Vec::new(),
    );
    px
}

fn through_preview(e: &mut Editor, comp: &mut Compositor, view: View) -> Vec<u32> {
    let mut preview = DragPreview::build(comp, e, view, area()).expect("visible");
    assert!(preview.render(e));
    let mut px = blank();
    preview.blit(&mut surface(&mut px), &mut Vec::new());
    px
}

pub(super) fn views() -> [View; 6] {
    let at = |num, den, x, y| View { num, den, x, y };
    [
        at(1, 1, 8, 6),
        at(3, 1, 4, 3),
        at(8, 1, -30, -20),
        at(1, 2, 5, 5),
        at(1, 4, 40, 30),
        at(2, 1, 110, 80),
    ]
}

fn assert_matches_compositor(e: &mut Editor, onion: bool, what: &str) {
    assert_matches_with(e, |comp| comp.onion = onion, what);
}

fn assert_matches_with(e: &mut Editor, setup: impl Fn(&mut Compositor), what: &str) {
    let moves = [(0, 0), (5, 3), (-9, 4), (30, 30), (-60, -2)];
    for view in views() {
        for (dx, dy) in moves {
            let mut comp = Compositor::new(SIZE, SIZE);
            setup(&mut comp);
            e.damage_all();
            through_compositor(e, &mut comp, view);
            e.apply(FloatingCmd::Move {
                dx: dx + 1,
                dy,
                join: true,
            })
            .expect("move");
            e.apply(FloatingCmd::Move {
                dx: -1,
                dy: 0,
                join: true,
            })
            .expect("move");
            let preview = through_preview(e, &mut comp, view);
            let settled = through_compositor(e, &mut comp, view);
            assert!(preview == settled, "{what}, {view:?}, moved {dx},{dy}");
        }
    }
}

#[test]
fn matches_the_compositor_for_each_color_mode_and_blend() {
    for mode in [ColorMode::Palette, ColorMode::Rgba] {
        for blend in [BlendMode::Normal, BlendMode::Multiply] {
            let mut e = scene(mode);
            e.apply(pikslar_core::LayerCmd::SetBlend {
                layer: e.layer_id(1).expect("layer"),
                blend,
            })
            .expect("set");
            e.apply(pikslar_core::LayerCmd::SetBlend {
                layer: e.layer_id(2).expect("layer"),
                blend: BlendMode::Multiply,
            })
            .expect("set");
            e.apply(pikslar_core::LayerCmd::SetOpacity {
                layer: e.layer_id(3).expect("layer"),
                opacity: 140,
            })
            .expect("set");
            select_and_lift(&mut e, false);
            assert_matches_compositor(&mut e, false, "layers");
        }
    }
}

/// The document's edge goes around the whole document, not around the part of it the preview
/// samples, so moving a selection never makes the edge jump, even with the document partly off
/// the screen.
#[test]
fn matches_the_compositor_with_the_document_edge_drawn() {
    set_backdrop(Backdrop {
        edge_width: 2,
        edge: 0xff0b_0b0d,
        ..Backdrop::NEUTRAL
    });
    let mut e = scene(ColorMode::Rgba);
    select_and_lift(&mut e, false);
    assert_matches_compositor(&mut e, false, "edge");
    set_backdrop(Backdrop::NEUTRAL);
}

#[test]
fn matches_the_compositor_at_partial_opacity_with_a_copy() {
    let mut e = scene(ColorMode::Rgba);
    e.apply(pikslar_core::LayerCmd::SetOpacity {
        layer: e.layer_id(1).expect("layer"),
        opacity: 90,
    })
    .expect("set");
    select_and_lift(&mut e, true);
    assert_matches_compositor(&mut e, false, "translucent copy");
}

#[test]
fn matches_the_compositor_when_layers_are_hidden() {
    let mut e = scene(ColorMode::Palette);
    e.apply(pikslar_core::LayerCmd::SetVisible {
        layer: e.layer_id(3).expect("layer"),
        visible: false,
    })
    .expect("set");
    select_and_lift(&mut e, false);
    assert_matches_compositor(&mut e, false, "layer above hidden");
    let mut e = scene(ColorMode::Palette);
    e.apply(pikslar_core::LayerCmd::SetVisible {
        layer: e.layer_id(1).expect("layer"),
        visible: false,
    })
    .expect("set");
    select_and_lift(&mut e, false);
    assert_matches_compositor(&mut e, false, "active layer hidden");
}

#[test]
fn matches_the_compositor_with_onion_skin() {
    let mut e = scene(ColorMode::Rgba);
    e.apply(pikslar_core::FrameCmd::Add { at: 1, copy: true })
        .expect("room");
    e.frame = 1;
    paint(&mut e, 1, 5);
    e.layer = 1;
    select_and_lift(&mut e, false);
    assert_matches_compositor(&mut e, true, "onion");
}

/// Five frames, each drawn, with a hold over frames 2 to 4, and a selection lifted on frame 2.
fn held_frames() -> Editor {
    let mut e = scene(ColorMode::Rgba);
    for at in 1..5 {
        e.apply(pikslar_core::FrameCmd::Add { at, copy: true })
            .expect("room");
        e.frame = at;
        paint(&mut e, 1, at as u32 + 5);
    }
    e.apply(pikslar_core::FrameCmd::HoldAcross {
        layer: e.layer_id(0).expect("layer"),
        source: 1,
        from: 1,
        to: 3,
    })
    .expect("frames");
    e.frame = 1;
    e.layer = 1;
    select_and_lift(&mut e, false);
    e
}

#[test]
fn matches_the_compositor_with_a_deep_looping_onion_skin_and_holds() {
    let mut e = held_frames();
    let span = pikslar_core::Span::whole(5);
    let setup = |comp: &mut Compositor| {
        comp.onion = true;
        comp.onion_depth = 3;
        comp.onion_loop = Some(span);
    };
    assert_matches_with(&mut e, setup, "deep onion");
}

/// Every onion style the tests try.
pub(super) fn onion_styles() -> [crate::onion::OnionStyle; 4] {
    use crate::onion::{OnionColors, OnionStyle};
    let normal = OnionStyle::default();
    [
        OnionStyle {
            past: false,
            ..normal
        },
        OnionStyle {
            future: false,
            ..normal
        },
        OnionStyle {
            strength: 4,
            colors: OnionColors::GreenPurple,
            ..normal
        },
        OnionStyle {
            strength: 0,
            colors: OnionColors::Own,
            ..normal
        },
    ]
}

#[test]
fn matches_the_compositor_in_every_onion_style() {
    for style in onion_styles() {
        let mut e = held_frames();
        let setup = move |comp: &mut Compositor| {
            comp.onion = true;
            comp.onion_depth = 2;
            comp.onion_style = style;
        };
        assert_matches_with(&mut e, setup, &format!("{style:?}"));
    }
}

/// One drawing shown at several places: frames 1 to 3 of layer 1 hold one drawing with the
/// middle frame moved alone, and the first cel of layer 0 linked to the second of layer 2,
/// which sits elsewhere. Floating pixels are lifted from the moved frame.
pub(super) fn shared_apart() -> Editor {
    use pikslar_core::{CelCmd, CelDrag, DragKind, FrameCmd, Landing};
    let mut e = scene(ColorMode::Rgba);
    for at in 1..4 {
        e.apply(FrameCmd::Add { at, copy: true }).expect("room");
        e.frame = at;
        paint(&mut e, 1, at as u32 + 5);
    }
    let (held, other) = (e.layer_id(1).expect("layer"), e.layer_id(2).expect("layer"));
    e.apply(FrameCmd::HoldAcross {
        layer: held,
        source: 1,
        from: 1,
        to: 3,
    })
    .expect("frames");
    let apart = |layer, frame, dx, dy| CelCmd::MoveOne {
        layer,
        frame,
        dx,
        dy,
    };
    e.apply(apart(held, 2, 4, -3)).expect("moved");
    e.apply(CelCmd::Drag(CelDrag {
        layers: (0, 0),
        frames: (0, 0),
        to_layer: 2,
        to_frame: 1,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    }))
    .expect("linked");
    e.apply(apart(other, 1, -5, 2)).expect("moved");
    e.apply(pikslar_core::StepCmd::EndDrag).expect("end");
    (e.layer, e.frame) = (1, 2);
    e
}

#[test]
fn matches_the_compositor_with_one_drawing_at_several_places_in_every_onion_style() {
    for style in onion_styles() {
        let mut e = shared_apart();
        select_and_lift(&mut e, false);
        let setup = move |comp: &mut Compositor| {
            comp.onion = true;
            comp.onion_depth = 2;
            comp.onion_style = style;
        };
        assert_matches_with(&mut e, setup, &format!("{style:?}"));
    }
}

#[test]
fn matches_the_compositor_after_a_flip() {
    let mut e = scene(ColorMode::Palette);
    select_and_lift(&mut e, false);
    e.apply(FloatingCmd::FlipHorizontal)
        .expect("fliphorizontal");
    assert_matches_compositor(&mut e, false, "flipped");
}

fn through_scaled_preview(
    e: &mut Editor,
    comp: &mut Compositor,
    view: View,
    target: Target,
) -> Vec<u32> {
    let mut preview = DragPreview::build(comp, e, view, area()).expect("visible");
    assert!(preview.render_scaled(e, target));
    let mut px = blank();
    preview.blit(&mut surface(&mut px), &mut Vec::new());
    px
}

fn assert_scaling_matches_compositor(e: &mut Editor, what: &str) {
    let targets = [
        (6, 4, 44, 19),
        (0, 0, 11, 40),
        (-7, 9, 30, 61),
        (20, 20, 1, 1),
    ];
    for view in views() {
        for target in targets {
            let mut comp = Compositor::new(SIZE, SIZE);
            e.damage_all();
            through_compositor(e, &mut comp, view);
            let shown = through_scaled_preview(e, &mut comp, view, target);
            e.apply(FloatingCmd::Scale {
                x: target.0,
                y: target.1,
                w: target.2,
                h: target.3,
            })
            .expect("scale");
            let applied = through_compositor(e, &mut comp, view);
            assert!(shown == applied, "{what}, {view:?}, to {target:?}");
        }
    }
}

#[test]
fn a_scale_not_yet_applied_looks_exactly_as_it_will_when_applied() {
    for mode in [ColorMode::Palette, ColorMode::Rgba] {
        let mut e = scene(mode);
        e.apply(pikslar_core::LayerCmd::SetBlend {
            layer: e.layer_id(2).expect("layer"),
            blend: BlendMode::Multiply,
        })
        .expect("set");
        e.apply(pikslar_core::LayerCmd::SetOpacity {
            layer: e.layer_id(3).expect("layer"),
            opacity: 140,
        })
        .expect("set");
        select_and_lift(&mut e, false);
        assert_scaling_matches_compositor(&mut e, "scaled");
    }
}

#[test]
fn scaling_is_previewed_correctly_after_flips_turns_and_for_copies() {
    let mut e = scene(ColorMode::Palette);
    select_and_lift(&mut e, false);
    e.apply(FloatingCmd::FlipVertical).expect("flipvertical");
    e.apply(FloatingCmd::Rotate { clockwise: true })
        .expect("rotate");
    assert_scaling_matches_compositor(&mut e, "flipped and turned");
    let mut e = scene(ColorMode::Rgba);
    e.apply(pikslar_core::LayerCmd::SetOpacity {
        layer: e.layer_id(1).expect("layer"),
        opacity: 90,
    })
    .expect("set");
    select_and_lift(&mut e, true);
    assert_scaling_matches_compositor(&mut e, "translucent copy");
}

#[test]
fn rendering_the_same_scale_again_does_nothing() {
    let mut e = scene(ColorMode::Rgba);
    select_and_lift(&mut e, false);
    let mut comp = Compositor::new(SIZE, SIZE);
    let mut preview = DragPreview::build(&mut comp, &e, views()[0], area()).expect("visible");
    assert!(preview.render_scaled(&e, (1, 1, 9, 9)));
    assert!(!preview.render_scaled(&e, (1, 1, 9, 9)));
    assert!(preview.render_scaled(&e, (1, 1, 9, 10)));
}

#[test]
fn rendering_again_without_a_move_does_nothing() {
    let mut e = scene(ColorMode::Rgba);
    select_and_lift(&mut e, false);
    let mut comp = Compositor::new(SIZE, SIZE);
    let mut preview = DragPreview::build(&mut comp, &e, views()[0], area()).expect("visible");
    assert!(preview.render(&e));
    assert!(!preview.render(&e));
    e.apply(FloatingCmd::Move {
        dx: 1,
        dy: 0,
        join: true,
    })
    .expect("move");
    assert!(preview.render(&e));
}

#[test]
fn a_view_with_no_canvas_in_it_has_no_preview() {
    let mut e = scene(ColorMode::Rgba);
    select_and_lift(&mut e, false);
    let mut comp = Compositor::new(SIZE, SIZE);
    let away = View {
        num: 1,
        den: 1,
        x: 900,
        y: 900,
    };
    assert!(DragPreview::build(&mut comp, &e, away, area()).is_none());
}

/// Puts layer `index` in a group, hides the group and makes layer 1 active again.
fn hide_in_group(e: &mut Editor, index: usize) {
    let layer = e.layer_id(index).expect("layer");
    e.apply(pikslar_core::LayerCmd::Group { layer })
        .expect("grouped");
    let group = e.layer_id(e.layer).expect("group");
    e.apply(pikslar_core::LayerCmd::SetVisible {
        layer: group,
        visible: false,
    })
    .expect("hidden");
    e.layer = 1;
}

#[test]
fn matches_the_compositor_when_a_hidden_group_holds_layers() {
    let mut e = scene(ColorMode::Rgba);
    hide_in_group(&mut e, 3);
    select_and_lift(&mut e, false);
    assert_matches_compositor(&mut e, false, "a layer above in a hidden group");
    let mut e = scene(ColorMode::Rgba);
    hide_in_group(&mut e, 1);
    select_and_lift(&mut e, false);
    assert_matches_compositor(&mut e, false, "the active layer in a hidden group");
}

#[test]
fn matches_the_compositor_with_tilemap_layers_under_and_over_the_floating_pixels() {
    for map in [0, 2, 4] {
        let mut e = scene_with_map(map);
        e.layer = if map == 2 { 1 } else { 2 };
        select_and_lift(&mut e, false);
        assert_matches_compositor(&mut e, false, &format!("a map at {map}"));
        assert_matches_compositor(&mut e, true, &format!("a map at {map}, with onion skin"));
    }
}
