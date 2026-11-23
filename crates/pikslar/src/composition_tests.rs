//! Every way a picture leaves the document has to agree, pixel for pixel.
//!
//! An oracle written here draws random stacks one pixel at a time. The compositor and export are
//! compared with it. The drag preview, moved or scaled, is compared with the compositor's picture
//! of the same state once the floating pixels are settled, because the oracle knows nothing of
//! floating pixels. Strokes and floating pixels reach export only in the hand-written cases.
//!
//! The oracle does its own folding and offset lookup. It shares the arithmetic in `blend.rs`,
//! which that file's tests check against the written formulas, and it takes which frames ghost
//! and which frames share a drawing from core, whose own tests cover them.
//!
//! Cels sit at random offsets, negative ones included, so part of each drawing lies past the
//! canvas, and a held frame sometimes sits apart from the rest of its hold, as a file may place
//! it. The oracle looks each pixel up through its frame's offset, and leaves a ghost out only
//! when it shows the current frame's drawing at the same place.
//!
//! The same stacks are drawn again with Aseprite-compatible blending, which folds straight
//! pixels with the arithmetic in `blend_straight.rs` and shows them premultiplied.

use pikslar_core::blend_straight;
use pikslar_core::{
    onion_ghosts, BlendMode, Blending, Cel, ColorMode, ColorTable, Direction, Document, DrawCmd,
    Editor, FloatingCmd, FrameCmd, FrameInfo, Grid, Layer, LayerCmd, Mask, SelectOp, SelectionCmd,
    Span, Tool,
};
use pikslar_render::blend::{self, ghost_over, multiply, over, separable};
use pikslar_render::drag_preview::Target;
use pikslar_render::{blit_canvas, flatten, Compositor, DragPreview, Rect, Surface, View};
use std::sync::Arc;

const W: usize = 70;
const H: usize = 40;
const STACKS: u32 = 2000;
const SCREEN: (i32, i32) = (300, 200);

const PAST: u32 = 0x00e8_5a48;
const FUTURE: u32 = 0x0048_90e8;
const GHOST_OPACITY: [u32; 3] = [104, 64, 36];

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() >> 8) as usize % n
    }

    fn pick<T: Copy>(&mut self, from: &[T]) -> T {
        from[self.below(from.len())]
    }
}

struct LayerSpec {
    mode: ColorMode,
    blend: BlendMode,
    opacity: u8,
    visible: bool,
    /// One per frame.
    offsets: Vec<(i32, i32)>,
    holds: Vec<u32>,
    cels: Vec<Vec<u32>>,
}

struct Stack {
    colors: Vec<u32>,
    frames: usize,
    layers: Vec<LayerSpec>,
}

fn random_color(rng: &mut Rng) -> u32 {
    let alpha = rng.pick(&[255u32, 255, 255, 200, 128, 1, 0]);
    (rng.next() & 0x00ff_ffff) | (alpha << 24)
}

fn random_cell(rng: &mut Rng, mode: ColorMode, table: usize) -> u32 {
    match mode {
        ColorMode::Rgba => random_color(rng),
        // A few ids point past the table, which must read as empty everywhere.
        ColorMode::Palette => (rng.below(table + 3) as u32).min(65_535),
    }
}

fn random_cel(rng: &mut Rng, mode: ColorMode, table: usize) -> Vec<u32> {
    let mut cells = vec![0; W * H];
    match rng.below(4) {
        0 => {}
        1 => {
            let solid = random_cell(rng, mode, table);
            cells.fill(solid);
        }
        density => {
            for cell in &mut cells {
                if rng.below(4) < density {
                    *cell = random_cell(rng, mode, table);
                }
            }
        }
    }
    cells
}

/// Runs of frames sharing one drawing, which onion skin has to leave out.
fn random_holds(rng: &mut Rng, frames: usize) -> Vec<u32> {
    let mut holds = vec![0; frames];
    if frames >= 2 && rng.below(3) == 0 {
        let from = rng.below(frames - 1);
        holds[from] = 1;
        holds[from + 1] = 1;
    }
    holds
}

fn random_offset(rng: &mut Rng) -> (i32, i32) {
    if rng.below(2) == 0 {
        return (0, 0);
    }
    (
        rng.below(2 * W + 1) as i32 - W as i32,
        rng.below(2 * H + 1) as i32 - H as i32,
    )
}

fn random_layer(rng: &mut Rng, frames: usize, table: usize) -> LayerSpec {
    let mode = rng.pick(&[ColorMode::Rgba, ColorMode::Palette]);
    LayerSpec {
        mode,
        blend: match rng.pick(&[0, 0, 0, 1]) {
            0 => BlendMode::Normal,
            _ => rng.pick(&BlendMode::ALL),
        },
        opacity: rng.pick(&[255, 255, 200, 128, 1, 0]),
        visible: rng.below(5) != 0,
        offsets: {
            let base = random_offset(rng);
            (0..frames)
                .map(|_| {
                    if rng.below(4) == 0 {
                        random_offset(rng)
                    } else {
                        base
                    }
                })
                .collect()
        },
        holds: random_holds(rng, frames),
        cels: (0..frames).map(|_| random_cel(rng, mode, table)).collect(),
    }
}

fn random_stack(seed: u32) -> Stack {
    let mut rng = Rng(seed.wrapping_mul(2_654_435_761) | 1);
    let frames = 1 + rng.below(4);
    // Colors are made distinct, because the table stores each only once.
    let colors: Vec<u32> = std::iter::once(0)
        .chain((1..=6u32).map(|i| (random_color(&mut rng) & 0xffff_ff00) | i))
        .collect();
    let layers = (0..1 + rng.below(5))
        .map(|_| random_layer(&mut rng, frames, colors.len()))
        .collect();
    Stack {
        colors,
        frames,
        layers,
    }
}

fn document(stack: &Stack) -> Document {
    let layers = stack
        .layers
        .iter()
        .map(|spec| {
            let mut layer = Layer::new("layer".into(), spec.mode);
            layer.visible = spec.visible;
            layer.opacity = spec.opacity;
            layer.blend = spec.blend;
            let cels = spec
                .cels
                .iter()
                .zip(&spec.offsets)
                .map(|(cells, &(x, y))| {
                    let image = Grid::from_cells(W, H, cells).into_image();
                    Cel::at(image, x, y).expect("in range")
                })
                .collect();
            layer
                .with_cels(cels)
                .with_holds(&spec.holds)
                .expect("one per frame")
        })
        .collect();
    let frames = vec![FrameInfo::new(100); stack.frames];
    let colors = Arc::new(ColorTable::from_parts(&stack.colors, &[]));
    Document::from_parts(W, H, colors, layers, frames).expect("consistent")
}

/// What a layer holds at a canvas position, found through its offset.
fn cell_at(spec: &LayerSpec, layer: &Layer, frame: usize, x: usize, y: usize) -> u32 {
    let owner = (0..=frame)
        .find(|&f| layer.cel(f).same_image(layer.cel(frame)))
        .expect("a cel is its own image");
    let offset = spec.offsets[frame];
    let (local_x, local_y) = (x as i32 - offset.0, y as i32 - offset.1);
    if local_x < 0 || local_y < 0 || local_x >= W as i32 || local_y >= H as i32 {
        return 0;
    }
    spec.cels[owner][local_y as usize * W + local_x as usize]
}

fn mix(dst: u32, doc: &Document, layer: &Layer, cell: u32) -> u32 {
    let color = match layer.mode {
        ColorMode::Rgba => cell,
        ColorMode::Palette => doc.colors.color(cell),
    };
    let opacity = u32::from(layer.opacity);
    if doc.blending == Blending::Aseprite {
        return blend_straight::mix(dst, color, layer.blend, opacity);
    }
    // Pikslar's own blending draws a mode only Aseprite has as its stand-in.
    let mode: blend::Separable = match layer.blend.stand_in() {
        BlendMode::Normal => return over(dst, color, opacity),
        BlendMode::Multiply => return multiply(dst, color, opacity),
        BlendMode::Screen => blend::screen,
        BlendMode::Overlay => blend::overlay,
        BlendMode::Darken => blend::darken,
        BlendMode::Lighten => blend::lighten,
        BlendMode::Addition => blend::addition,
        BlendMode::Subtract => blend::subtract,
        BlendMode::Difference => blend::difference,
        other => unreachable!("{other:?} is not one of Pikslar's modes"),
    };
    separable(dst, color, opacity, mode)
}

/// One frame at one position. `shown` leaves out layers still showing that frame's drawing.
fn fold(
    stack: &Stack,
    doc: &Document,
    frame: usize,
    shown: Option<usize>,
    x: usize,
    y: usize,
) -> u32 {
    let mut out = 0;
    for (spec, layer) in stack.layers.iter().zip(doc.layers()) {
        let held = shown.is_some_and(|current| {
            let (cel, now) = (layer.cel(frame), layer.cel(current));
            cel.same_image(now) && cel.position() == now.position()
        });
        if layer.visible && !held {
            out = mix(out, doc, layer, cell_at(spec, layer, frame, x, y));
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
struct Onion {
    depth: usize,
    looping: bool,
}

fn whole_loop(frames: usize) -> Span {
    Span {
        from: 0,
        to: frames - 1,
        direction: Direction::Forward,
    }
}

/// The oracle: one frame, with its onion skin, as premultiplied BGRA.
fn oracle(stack: &Stack, doc: &Document, frame: usize, onion: Onion) -> Vec<u32> {
    let looping = onion.looping.then_some(whole_loop(stack.frames));
    let ghosts = onion_ghosts(frame, stack.frames, onion.depth, onion.depth, looping);
    // Aseprite-compatible blending folds straight pixels; ghosts are drawn on the screen's.
    let aseprite = doc.blending == Blending::Aseprite;
    let screen = |px: u32| match aseprite {
        true => blend_straight::to_screen(px),
        false => px,
    };
    let mut image = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            let mut out = 0;
            for ghost in &ghosts {
                let flat = screen(fold(stack, doc, ghost.frame, Some(frame), x, y));
                let tint = if ghost.distance < 0 { PAST } else { FUTURE };
                let away = ghost.distance.unsigned_abs() as usize;
                out = ghost_over(out, flat, tint, GHOST_OPACITY[away - 1]);
            }
            if aseprite {
                out = blend_straight::from_screen(out);
            }
            for (spec, layer) in stack.layers.iter().zip(doc.layers()) {
                if layer.visible {
                    out = mix(out, doc, layer, cell_at(spec, layer, frame, x, y));
                }
            }
            image.push(screen(out));
        }
    }
    image
}

fn configure(comp: &mut Compositor, onion: Onion, frames: usize) {
    comp.onion = onion.depth > 0;
    comp.onion_depth = onion.depth.max(1);
    comp.onion_loop = onion.looping.then_some(whole_loop(frames));
}

fn full(comp: &mut Compositor, e: &mut Editor) {
    e.damage_all();
    let damage = e.take_damage();
    comp.update(e, damage, &mut Vec::new());
}

fn assert_same(found: &[u32], wanted: &[u32], what: &str, seed: u32) {
    if let Some(index) = found.iter().zip(wanted).position(|(a, b)| a != b) {
        panic!(
            "{what}, seed {seed}, at {},{}: {:08x}, wanted {:08x}",
            index % W,
            index / W,
            found[index],
            wanted[index],
        );
    }
    assert_eq!(found.len(), wanted.len(), "{what}, seed {seed}");
}

fn views() -> [View; 3] {
    let at = |num, den, x, y| View { num, den, x, y };
    [at(1, 1, 8, 6), at(3, 1, 4, 3), at(1, 2, 5, 5)]
}

fn area() -> Rect {
    Rect::new(3, 2, 290, 190)
}

fn shown(comp: &Compositor, view: View) -> Vec<u32> {
    let mut px = vec![0; (SCREEN.0 * SCREEN.1) as usize];
    let mut surface = Surface {
        px: &mut px,
        w: SCREEN.0,
        h: SCREEN.1,
    };
    let (w, h) = (comp.width() as i32, comp.height() as i32);
    blit_canvas(
        comp.pixels(),
        w,
        h,
        view,
        &mut surface,
        area(),
        &mut Vec::new(),
    );
    px
}

fn previewed(comp: &mut Compositor, e: &Editor, view: View, scale: Option<Target>) -> Vec<u32> {
    let mut preview = DragPreview::build(comp, e, view, area()).expect("on screen");
    assert!(match scale {
        Some(target) => preview.render_scaled(e, target),
        None => preview.render(e),
    });
    let mut px = vec![0; (SCREEN.0 * SCREEN.1) as usize];
    let mut surface = Surface {
        px: &mut px,
        w: SCREEN.0,
        h: SCREEN.1,
    };
    preview.blit(&mut surface, &mut Vec::new());
    px
}

/// Lifts part of a layer and moves it. False if there was nothing to lift.
fn float_some(e: &mut Editor, rng: &mut Rng) -> bool {
    let (x, y) = (rng.below(W - 20) as i32, rng.below(H - 12) as i32);
    let shape = Mask::rect(x, y, 19, 11).subtract(&Mask::rect(x + 4, y + 3, 3, 2));
    e.apply(SelectionCmd::Select {
        shape: Box::new(shape),
        op: SelectOp::Replace,
    })
    .expect("select");
    let copy = rng.below(2) == 0;
    if !e
        .apply(FloatingCmd::Lift { copy })
        .is_ok_and(|outcome| outcome.changed)
    {
        return false;
    }
    let (dx, dy) = (rng.below(41) as i32 - 20, rng.below(21) as i32 - 10);
    e.apply(FloatingCmd::Move { dx, dy, join: true })
        .expect("move");
    true
}

/// The preview of a moved or scaled selection against the compositor's picture of the same state.
fn preview_matches(stack: &Stack, doc: &Document, seed: u32, onion: Onion, frame: usize) -> bool {
    let mut rng = Rng(seed ^ 0x5bd1_e995);
    let mut e = Editor::new(doc.clone());
    e.frame = frame;
    e.layer = rng.below(stack.layers.len());
    let mut comp = Compositor::new(W, H);
    configure(&mut comp, onion, stack.frames);
    full(&mut comp, &mut e);
    if !float_some(&mut e, &mut rng) {
        return false;
    }
    let scale = (rng.below(2) == 0).then(|| {
        let (x, y) = (rng.below(30) as i32 - 5, rng.below(20) as i32 - 5);
        (x, y, 1 + rng.below(40) as i32, 1 + rng.below(24) as i32)
    });
    let previews = views().map(|view| previewed(&mut comp, &e, view, scale));
    if let Some((x, y, w, h)) = scale {
        e.apply(FloatingCmd::Scale { x, y, w, h }).expect("scale");
    }
    e.settle_floating();
    full(&mut comp, &mut e);
    for (view, preview) in views().into_iter().zip(previews) {
        assert!(
            preview == shown(&comp, view),
            "preview, seed {seed}, {view:?}, {onion:?}"
        );
    }
    true
}

/// Showing what export wrote has to give exactly what the compositor shows.
fn export_matches(stack: &Stack, doc: &Document, seed: u32, frame: usize) {
    let mut e = Editor::new(doc.clone());
    e.frame = stack.frames - 1 - frame;
    let exported = flatten(&mut e, frame, &mut Compositor::new(W, H));
    let as_shown: Vec<u32> = exported.iter().map(|&p| over(0, p, 255)).collect();
    let plain = Onion {
        depth: 0,
        looping: false,
    };
    assert_same(&as_shown, &oracle(stack, doc, frame, plain), "export", seed);
    assert_eq!(e.frame, stack.frames - 1 - frame, "seed {seed}");
}

#[test]
fn random_stacks_look_the_same_through_every_path() {
    every_path(1..=STACKS, Blending::Pikslar);
}

#[test]
fn random_stacks_with_aseprite_blending_look_the_same_through_every_path() {
    every_path(1..=STACKS / 2, Blending::Aseprite);
}

fn every_path(seeds: std::ops::RangeInclusive<u32>, blending: Blending) {
    let (mut previews, count) = (0, seeds.clone().count() as u32);
    for seed in seeds {
        let stack = random_stack(seed);
        let mut doc = document(&stack);
        doc.blending = blending;
        let mut rng = Rng(seed ^ 0x1234_5678);
        let frame = rng.below(stack.frames);
        let onion = Onion {
            depth: rng.below(4),
            looping: rng.below(2) == 0,
        };

        let mut e = Editor::new(doc.clone());
        e.frame = frame;
        let mut comp = Compositor::new(W, H);
        configure(&mut comp, onion, stack.frames);
        full(&mut comp, &mut e);
        assert_same(
            comp.pixels(),
            &oracle(&stack, &doc, frame, onion),
            "compositor",
            seed,
        );

        previews += u32::from(preview_matches(&stack, &doc, seed, onion, frame));
        export_matches(&stack, &doc, seed, frame);
    }
    assert!(previews > count / 2, "only {previews} previews compared");
}

// Worked by hand from the formulas in `blend.rs`, so they do not lean on the code they check.

fn two_layers(bottom: u32, top: u32, setup: impl Fn(&mut Layer)) -> Editor {
    let mut doc = Document::new(1, 1, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("room");
    doc.set_cel(0, 0, Grid::from_cells(1, 1, &[bottom]))
        .expect("fits");
    doc.set_cel(1, 0, Grid::from_cells(1, 1, &[top]))
        .expect("fits");
    setup(&mut doc.layers_mut()[1]);
    Editor::new(doc)
}

fn screen_and_export(e: &mut Editor) -> (u32, u32) {
    let mut comp = Compositor::new(1, 1);
    full(&mut comp, e);
    let screen = comp.pixels()[0];
    (screen, flatten(e, 0, &mut Compositor::new(1, 1))[0])
}

const fn rgba(r: u32, g: u32, b: u32, a: u32) -> u32 {
    r | (g << 8) | (b << 16) | (a << 24)
}

const fn bgra(b: u32, g: u32, r: u32, a: u32) -> u32 {
    b | (g << 8) | (r << 16) | (a << 24)
}

#[test]
fn half_opacity_normal_is_exact() {
    let mut e = two_layers(rgba(255, 0, 0, 255), rgba(0, 0, 255, 255), |l| {
        l.opacity = 128
    });
    let (screen, export) = screen_and_export(&mut e);
    assert_eq!(screen, bgra(128, 0, 127, 255));
    assert_eq!(export, rgba(127, 0, 128, 255));
}

#[test]
fn multiply_is_exact() {
    let mut e = two_layers(rgba(200, 100, 50, 255), rgba(128, 128, 128, 255), |l| {
        l.blend = BlendMode::Multiply;
    });
    let (screen, export) = screen_and_export(&mut e);
    assert_eq!(screen, bgra(25, 50, 100, 255));
    assert_eq!(export, rgba(100, 50, 25, 255));
}

#[test]
fn multiply_over_nothing_is_the_source() {
    let mut e = two_layers(0, rgba(10, 20, 30, 128), |l| l.blend = BlendMode::Multiply);
    let (screen, export) = screen_and_export(&mut e);
    assert_eq!(screen, bgra(15, 10, 5, 128));
    assert_eq!(export, rgba(10, 20, 30, 128));
}

#[test]
fn hidden_and_fully_faded_layers_add_nothing() {
    let bottom = rgba(1, 2, 3, 255);
    for setup in [
        |l: &mut Layer| l.visible = false,
        |l: &mut Layer| l.opacity = 0,
    ] {
        let mut e = two_layers(bottom, rgba(200, 200, 200, 255), setup);
        let (screen, export) = screen_and_export(&mut e);
        assert_eq!(screen, bgra(3, 2, 1, 255));
        assert_eq!(export, bottom);
    }
}

#[test]
fn a_palette_color_blends_like_the_same_rgba_color() {
    let color = rgba(90, 160, 30, 128);
    let colors = Arc::new(ColorTable::from_parts(&[0, color], &[]));
    let build = |mode, cell| {
        let mut layer = Layer::new("layer".into(), mode);
        layer.opacity = 200;
        let layer = layer.with_cels(vec![Cel::new(Grid::from_cells(1, 1, &[cell]))]);
        let frames = vec![FrameInfo::new(100)];
        Document::from_parts(1, 1, colors.clone(), vec![layer], frames).expect("valid")
    };
    let mut palette = Editor::new(build(ColorMode::Palette, 1));
    let mut direct = Editor::new(build(ColorMode::Rgba, color));
    assert_eq!(
        screen_and_export(&mut palette),
        screen_and_export(&mut direct)
    );
}

fn onion_frames(cells: [u32; 3], holds: [u32; 3]) -> Editor {
    let cels = cells
        .iter()
        .map(|cell| Cel::new(Grid::from_cells(1, 1, &[*cell])))
        .collect();
    let layer = Layer::new("layer".into(), ColorMode::Rgba)
        .with_cels(cels)
        .with_holds(&holds)
        .expect("one per frame");
    let frames = vec![FrameInfo::new(100); 3];
    let colors = Arc::new(ColorTable::new());
    Editor::new(Document::from_parts(1, 1, colors, vec![layer], frames).expect("valid"))
}

#[test]
fn onion_skin_is_exact_on_both_sides() {
    let white = rgba(255, 255, 255, 255);
    let mut e = onion_frames([white, 0, white], [0, 0, 0]);
    e.frame = 1;
    let mut comp = Compositor::new(1, 1);
    comp.onion = true;
    full(&mut comp, &mut e);
    // The later ghost first, then the earlier one over it.
    assert_eq!(comp.pixels()[0], bgra(125, 118, 138, 166));
}

#[test]
fn a_held_drawing_does_not_ghost_onto_itself() {
    let white = rgba(255, 255, 255, 255);
    let mut e = onion_frames([white, white, 0], [1, 1, 0]);
    e.frame = 1;
    let mut comp = Compositor::new(1, 1);
    comp.onion = true;
    full(&mut comp, &mut e);
    assert_eq!(comp.pixels()[0], bgra(255, 255, 255, 255));
}

const RED: u32 = rgba(255, 0, 0, 255);
const RED_SHOWN: u32 = bgra(0, 0, 255, 255);

fn row_of_four(cells: [u32; 4]) -> Editor {
    let mut doc = Document::new(4, 1, ColorMode::Rgba).expect("valid");
    doc.set_cel(0, 0, Grid::from_cells(4, 1, &cells))
        .expect("fits");
    Editor::new(doc)
}

fn lift_first_cell_and_move_it(e: &mut Editor, dx: i32) {
    e.apply(SelectionCmd::Select {
        shape: Box::new(Mask::rect(0, 0, 1, 1)),
        op: SelectOp::Replace,
    })
    .expect("select");
    assert!(
        e.apply(FloatingCmd::Lift { copy: false })
            .expect("lift")
            .changed
    );
    e.apply(FloatingCmd::Move {
        dx,
        dy: 0,
        join: true,
    })
    .expect("move");
}

#[test]
fn a_single_layer_exports_floating_pixels_where_they_are_now() {
    let mut e = row_of_four([RED, 0, 0, 0]);
    lift_first_cell_and_move_it(&mut e, 2);
    let px = flatten(&mut e, 0, &mut Compositor::new(4, 1));
    assert_eq!(px, [0, 0, RED, 0]);
}

#[test]
fn a_single_layer_exports_the_stroke_in_progress_as_the_screen_shows_it() {
    let mut e = row_of_four([0; 4]);
    e.color = RED;
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 1,
        y: 0,
    })
    .expect("begin");
    let mut comp = Compositor::new(4, 1);
    full(&mut comp, &mut e);
    assert_eq!(comp.pixels()[1], RED_SHOWN);
    let px = flatten(&mut e, 0, &mut Compositor::new(4, 1));
    assert_eq!(px[1], RED);
}

#[test]
fn floating_pixels_stay_on_their_own_frame_when_another_is_exported() {
    let mut e = row_of_four([RED, 0, 0, 0]);
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.frame = 0;
    lift_first_cell_and_move_it(&mut e, 2);
    let px = flatten(&mut e, 1, &mut Compositor::new(4, 1));
    assert_eq!(px, [0; 4]);
    // Two layers take the other route through export.
    let mut e = row_of_four([RED, 0, 0, 0]);
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.frame = 0;
    e.layer = 0;
    lift_first_cell_and_move_it(&mut e, 2);
    let px = flatten(&mut e, 1, &mut Compositor::new(4, 1));
    assert_eq!(px, [0; 4]);
}

#[test]
fn export_never_includes_onion_skin() {
    let white = rgba(255, 255, 255, 255);
    let mut e = onion_frames([white, 0, white], [0, 0, 0]);
    e.frame = 1;
    let mut comp = Compositor::new(1, 1);
    comp.onion = true;
    assert_eq!(flatten(&mut e, 1, &mut comp), [0]);
    // Two layers take the other route through export.
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    assert_eq!(flatten(&mut e, 1, &mut comp), [0]);
    assert!(comp.onion, "the caller's setting is left alone");
}

#[test]
fn a_palette_id_too_large_for_the_table_exports_as_the_screen_shows_it() {
    let color = rgba(90, 160, 30, 255);
    let colors = Arc::new(ColorTable::from_parts(&[0, color], &[]));
    let layer = Layer::new("layer".into(), ColorMode::Palette)
        .with_cels(vec![Cel::new(Grid::from_cells(1, 1, &[65_537]))]);
    let frames = vec![FrameInfo::new(100)];
    let doc = Document::from_parts(1, 1, colors, vec![layer], frames).expect("valid");
    let mut e = Editor::new(doc);
    let (screen, export) = screen_and_export(&mut e);
    assert_eq!(over(0, export, 255), screen);
}

#[test]
fn a_compositor_of_the_wrong_size_still_exports_the_whole_document() {
    let mut doc = Document::new(3, 2, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("room");
    doc.set_cel(0, 0, Grid::from_cells(3, 2, &[RED; 6]))
        .expect("fits");
    let mut e = Editor::new(doc);
    let px = flatten(&mut e, 0, &mut Compositor::new(40, 40));
    assert_eq!(px, [RED; 6]);
}
