use super::*;
use pikslar_core::DrawCmd;
use pikslar_core::{rgba, BlendMode, ColorMode, Document, Tool};

const RED: u32 = rgba(255, 0, 0, 255);
const BGRA_RED: u32 = 0xffff_0000;
const BGRA_BLUE: u32 = 0xff00_00ff;

fn setup(mode: ColorMode) -> (Editor, Compositor, Vec<Rect>) {
    let editor = Editor::new(Document::new(40, 24, mode).expect("valid"));
    (editor, Compositor::new(40, 24), Vec::new())
}

fn sync(e: &mut Editor, c: &mut Compositor, changed: &mut Vec<Rect>) {
    changed.clear();
    let damage = e.take_damage();
    c.update(e, damage, changed);
}

fn at(c: &Compositor, x: usize, y: usize) -> u32 {
    c.pixels()[y * c.width() + x]
}

#[test]
fn stroke_shows_before_commit_in_both_modes() {
    for mode in [ColorMode::Palette, ColorMode::Rgba] {
        let (mut e, mut c, mut changed) = setup(mode);
        sync(&mut e, &mut c, &mut changed);
        e.color = RED;
        e.apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x: 35,
            y: 20,
        })
        .expect("begin");
        sync(&mut e, &mut c, &mut changed);
        assert_eq!(at(&c, 35, 20), BGRA_RED, "{mode:?}");
        assert_eq!(
            changed,
            vec![Rect::new(32, 16, 8, 8)],
            "tile clipped to the canvas"
        );
        e.apply(DrawCmd::Cancel).expect("cancel");
        sync(&mut e, &mut c, &mut changed);
        assert_eq!(at(&c, 35, 20), 0);
    }
}

#[test]
fn repeated_damage_composites_once() {
    let (mut e, mut c, mut changed) = setup(ColorMode::Rgba);
    sync(&mut e, &mut c, &mut changed);
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 1,
        y: 1,
    })
    .expect("begin");
    e.apply(DrawCmd::To { x: 2, y: 1 }).expect("drag");
    e.apply(DrawCmd::To { x: 3, y: 1 }).expect("drag");
    sync(&mut e, &mut c, &mut changed);
    assert_eq!(changed.len(), 1);
}

#[test]
fn higher_layers_cover_lower_and_hidden_layers_do_not() {
    let (mut e, mut c, mut changed) = setup(ColorMode::Rgba);
    e.color = RED;
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 1,
        y: 1,
    })
    .expect("begin");
    e.apply(DrawCmd::Commit).expect("commit");
    e.apply(pikslar_core::LayerCmd::Add {
        at: 1,
        mode: ColorMode::Palette,
    })
    .expect("room");
    e.layer = 1;
    e.color = rgba(0, 0, 255, 255);
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 1,
        y: 1,
    })
    .expect("begin");
    e.apply(DrawCmd::Commit).expect("commit");
    sync(&mut e, &mut c, &mut changed);
    assert_eq!(at(&c, 1, 1), BGRA_BLUE);
    e.apply(pikslar_core::LayerCmd::SetVisible {
        layer: e.layer_id(1).expect("layer"),
        visible: false,
    })
    .expect("set");
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    assert_eq!(at(&c, 1, 1), BGRA_RED);
}

#[test]
fn palette_edit_recolors_the_screen() {
    let (mut e, mut c, mut changed) = setup(ColorMode::Palette);
    e.color = RED;
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 1,
        y: 1,
    })
    .expect("begin");
    e.apply(DrawCmd::Commit).expect("commit");
    let id = e.doc().layers()[0].cel(0).get(1, 1);
    e.apply(pikslar_core::PaletteCmd::Recolor {
        id,
        color: rgba(0, 0, 255, 255),
    })
    .expect("recolor");
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    assert_eq!(at(&c, 1, 1), BGRA_BLUE);
}

#[test]
fn onion_skin_shows_previous_frame_faintly() {
    let (mut e, mut c, mut changed) = setup(ColorMode::Rgba);
    e.color = RED;
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 1,
        y: 1,
    })
    .expect("begin");
    e.apply(DrawCmd::Commit).expect("commit");
    e.apply(pikslar_core::FrameCmd::Add { at: 1, copy: false })
        .expect("room");
    e.frame = 1;
    c.onion = true;
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    let alpha = at(&c, 1, 1) >> 24;
    assert!(alpha > 0 && alpha < 128, "alpha {alpha}");
}

fn onion_scene(frames: usize) -> (Editor, Compositor, Vec<Rect>) {
    let (mut e, mut c, changed) = setup(ColorMode::Rgba);
    for at in 1..frames {
        e.apply(pikslar_core::FrameCmd::Add { at, copy: false })
            .expect("room");
    }
    e.color = rgba(128, 128, 128, 255);
    for frame in 0..frames {
        e.frame = frame;
        e.apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x: frame as i32,
            y: 1,
        })
        .expect("begin");
        e.apply(DrawCmd::Commit).expect("commit");
    }
    c.onion = true;
    (e, c, changed)
}

fn red_and_blue(p: u32) -> (u32, u32) {
    ((p >> 16) & 255, p & 255)
}

#[test]
fn earlier_frames_look_warm_and_later_frames_cool() {
    let (mut e, mut c, mut changed) = onion_scene(3);
    e.frame = 1;
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    let (r, b) = red_and_blue(at(&c, 0, 1));
    assert!(r > b, "the frame before: {r} {b}");
    let (r, b) = red_and_blue(at(&c, 2, 1));
    assert!(b > r, "the frame after: {r} {b}");
}

#[test]
fn ghosts_fade_with_distance() {
    let (mut e, mut c, mut changed) = onion_scene(4);
    c.onion_depth = 3;
    e.frame = 3;
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    let alpha = |x| at(&c, x, 1) >> 24;
    assert!(alpha(2) > alpha(1) && alpha(1) > alpha(0) && alpha(0) > 0);
}

#[test]
fn the_first_frame_of_a_loop_sees_its_last() {
    let (mut e, mut c, mut changed) = onion_scene(3);
    e.frame = 0;
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    assert_eq!(at(&c, 2, 1) >> 24, 0, "no wrap without a loop");
    c.onion_loop = Some(pikslar_core::Span::whole(3));
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    assert!(at(&c, 2, 1) >> 24 > 0);
}

#[test]
fn a_loop_left_over_from_more_frames_is_harmless() {
    let (mut e, mut c, mut changed) = onion_scene(3);
    c.onion_depth = 3;
    c.onion_loop = Some(pikslar_core::Span {
        from: 0,
        to: 8,
        direction: pikslar_core::Direction::Forward,
    });
    e.frame = 2;
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    assert!(at(&c, 1, 1) >> 24 > 0, "still shows its real neighbours");
}

#[test]
fn a_held_drawing_casts_no_ghost_of_itself() {
    let (mut e, mut c, mut changed) = onion_scene(2);
    e.frame = 1;
    e.apply(pikslar_core::FrameCmd::HoldPrevious {
        layer: e.layer_id(0).expect("layer"),
        frame: 1,
    })
    .expect("frames");
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    assert_eq!(at(&c, 1, 1) >> 24, 0, "nothing was drawn there");
    assert_eq!(at(&c, 0, 1) >> 24, 255, "only the drawing itself");
}

#[test]
fn playback_shows_no_ghosts() {
    let (mut e, mut c, mut changed) = onion_scene(2);
    c.playing = true;
    e.frame = 1;
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    assert_eq!(at(&c, 0, 1) >> 24, 0);
}

#[test]
fn multiply_layers_darken() {
    let (mut e, mut c, mut changed) = setup(ColorMode::Rgba);
    e.color = rgba(100, 100, 100, 255);
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 1,
        y: 1,
    })
    .expect("begin");
    e.apply(DrawCmd::Commit).expect("commit");
    e.apply(pikslar_core::LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    e.layer = 1;
    e.apply(pikslar_core::LayerCmd::SetBlend {
        layer: e.layer_id(1).expect("layer"),
        blend: BlendMode::Multiply,
    })
    .expect("set");
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 1,
        y: 1,
    })
    .expect("begin");
    e.apply(DrawCmd::Commit).expect("commit");
    sync(&mut e, &mut c, &mut changed);
    assert!((at(&c, 1, 1) & 255) < 60);
}

/// The rows of `region` from a whole composited frame.
fn cut(c: &Compositor, (x0, y0, w, h): (usize, usize, usize, usize)) -> Vec<u32> {
    (y0..y0 + h)
        .flat_map(|y| c.pixels()[y * c.width() + x0..][..w].iter().copied())
        .collect()
}

#[test]
fn a_plain_region_is_the_frame_as_composited_but_without_ghosts() {
    let (mut e, mut c, mut changed) = onion_scene(3);
    e.frame = 1;
    for (x, y) in [(0, 0), (31, 5), (32, 5), (39, 23), (20, 12)] {
        e.apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x,
            y,
        })
        .expect("begin");
        e.apply(DrawCmd::Commit).expect("commit");
    }
    let regions = [
        (0, 0, 40, 24),
        (5, 3, 30, 17),
        (31, 0, 9, 24),
        (0, 23, 40, 1),
        (32, 5, 1, 1),
    ];
    let mut plain = Vec::new();
    // Without ghosts, a region is exactly what the compositor holds.
    c.onion = false;
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    let wanted: Vec<Vec<u32>> = regions.iter().map(|&r| cut(&c, r)).collect();
    for (&region, want) in regions.iter().zip(&wanted) {
        c.compose_plain(&e, region, &mut plain);
        assert_eq!(&plain, want, "{region:?}");
    }
    // With ghosts on, the plain region still leaves them out.
    c.onion = true;
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    assert_ne!(
        cut(&c, regions[0]),
        wanted[0],
        "the ghosts do show on the canvas"
    );
    for (&region, want) in regions.iter().zip(&wanted) {
        c.compose_plain(&e, region, &mut plain);
        assert_eq!(&plain, want, "{region:?} with ghosts");
    }
    c.compose_plain(&e, (30, 0, 20, 1), &mut plain);
    assert!(
        plain.iter().all(|&p| p == 0),
        "a region past the edge is left empty"
    );
}

fn dot_at(e: &mut Editor, x: i32, y: i32) {
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x,
        y,
    })
    .expect("begin");
    e.apply(DrawCmd::Commit).expect("commit");
}

/// What a compositor made just now, with the same onion skin, shows.
fn fresh_of(e: &Editor, like: &Compositor) -> Vec<u32> {
    let mut comp = like.resized(like.width(), like.height());
    comp.update(e, Damage::All, &mut Vec::new());
    comp.pixels().to_vec()
}

/// A frame sharing its drawing with the shown one leaves no ghost. A stroke that ends the sharing
/// makes the whole ghost show, and undoing it hides the whole ghost again: each update has to
/// redraw all of it, not only where the stroke went.
#[test]
fn ending_or_restoring_a_shared_drawing_redraws_the_whole_ghost() {
    let (mut e, mut c, mut changed) = setup(ColorMode::Rgba);
    // Translucent, so a ghost beneath the same drawing shows through it.
    e.color = rgba(128, 128, 128, 120);
    dot_at(&mut e, 1, 1);
    dot_at(&mut e, 30, 20);
    // Two frames showing one drawing without a hold, as a file or a paste can leave them.
    let mut doc = e.doc().clone();
    doc.add_frame(1, false).expect("room");
    let drawing = doc.layers()[0].cel(0).clone();
    doc.put_cel(0, 1, drawing).expect("a cel");
    let mut e = Editor::new(doc);
    e.frame = 1;
    let layer = &e.doc().layers()[0];
    assert!(layer.cel(0).looks_the_same(layer.cel(1)), "shared");
    c.onion = true;
    e.damage_all();
    sync(&mut e, &mut c, &mut changed);
    assert!(c.pixels() == fresh_of(&e, &c).as_slice());
    e.color = RED;
    dot_at(&mut e, 35, 3);
    sync(&mut e, &mut c, &mut changed);
    assert!(
        c.pixels() == fresh_of(&e, &c).as_slice(),
        "the ghost shows whole"
    );
    e.apply(pikslar_core::StepCmd::Undo).expect("undo");
    sync(&mut e, &mut c, &mut changed);
    assert!(
        c.pixels() == fresh_of(&e, &c).as_slice(),
        "the ghost goes whole"
    );
}
