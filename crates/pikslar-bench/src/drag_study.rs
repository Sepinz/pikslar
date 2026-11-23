//! `pikslar-bench drag-study [scale] [tiles]`: where the time goes while dragging a selection.
//!
//! The gate says whether dragging got slower. This says why, phase by phase and size by
//! size, following the path the app takes for one pointer event. `scale` drags a corner
//! handle instead of the selection. `tiles` turns off the screen-sized preview, to show
//! what compositing alone would cost.

use crate::live_bytes;
use crate::workloads::{document, percentile, Rng};
use pikslar_core::{scale_edges, Edge, Editor, SelectOp};
use pikslar_core::{FloatingCmd, SelectionCmd};
use pikslar_render::{
    blit_canvas, Compositor, DragPreview, Rect, Surface, View, PREVIEW_MIN_CELLS,
};
use std::hint::black_box;
use std::time::Instant;

const ACTIVE_LAYER: usize = 2;
const SCREEN: (i32, i32) = (1400, 900);
const EVENTS: usize = 80;
/// Selection side, the side of the square canvas, and whether to view it at 1:1.
///
/// Fitting a big canvas to the window zooms it out, so it reaches fewer screen pixels.
/// The 1:1 case fills the whole viewport, which is the most a preview ever has to draw.
const CASES: [(i32, usize, bool); 7] = [
    (256, 1024, false),
    (512, 1024, false),
    (1024, 1024, false),
    (2048, 2048, true),
    (2048, 2048, false),
    (4096, 4096, false),
    (8192, 8192, false),
];

#[derive(Default)]
struct Samples {
    moving: Vec<f64>,
    render: Vec<f64>,
    blit: Vec<f64>,
    total: Vec<f64>,
}

struct Screen {
    comp: Compositor,
    fb: Vec<u32>,
    view: View,
    xmap: Vec<i32>,
    changed: Vec<Rect>,
    preview: Option<DragPreview>,
}

fn area() -> Rect {
    Rect::new(0, 0, SCREEN.0, SCREEN.1)
}

impl Screen {
    fn new(size: usize, one_to_one: bool) -> Self {
        let side = size as i32;
        let view = if one_to_one {
            View {
                num: 1,
                den: 1,
                x: (SCREEN.0 - side) / 2,
                y: (SCREEN.1 - side) / 2,
            }
        } else {
            View::fit(side, side, area())
        };
        Screen {
            comp: Compositor::new(size, size),
            fb: vec![0; (SCREEN.0 * SCREEN.1) as usize],
            view,
            xmap: Vec::with_capacity(SCREEN.0 as usize),
            changed: Vec::with_capacity(64),
            preview: None,
        }
    }

    fn composite(&mut self, editor: &mut Editor) {
        self.changed.clear();
        let damage = editor.take_damage();
        self.comp.update(editor, damage, &mut self.changed);
    }

    /// The composite or preview step: everything before pixels go to the screen.
    fn render(&mut self, editor: &mut Editor) {
        match &mut self.preview {
            Some(preview) => {
                preview.render(editor);
            }
            None => self.composite(editor),
        }
    }

    fn blit(&mut self) {
        let mut target = Surface {
            px: &mut self.fb,
            w: SCREEN.0,
            h: SCREEN.1,
        };
        if let Some(preview) = &self.preview {
            preview.blit(&mut target, &mut self.xmap);
            return;
        }
        let (w, h) = (self.comp.width() as i32, self.comp.height() as i32);
        let px = self.comp.pixels();
        blit_canvas(px, w, h, self.view, &mut target, area(), &mut self.xmap);
    }
}

fn one_event(editor: &mut Editor, screen: &mut Screen, step: (i32, i32), out: &mut Samples) {
    let start = Instant::now();
    editor
        .apply(FloatingCmd::Move {
            dx: step.0,
            dy: step.1,
            join: true,
        })
        .expect("move");
    black_box(editor.selection());
    let moved = Instant::now();
    screen.render(editor);
    let rendered = Instant::now();
    screen.blit();
    let end = Instant::now();
    let span = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1e3;
    out.moving.push(span(start, moved));
    out.render.push(span(moved, rendered));
    out.blit.push(span(rendered, end));
    out.total.push(span(start, end));
}

/// One step of a corner drag: applied as it goes, or only shown when there is a preview.
fn one_scale_event(
    editor: &mut Editor,
    screen: &mut Screen,
    outline: &[Edge],
    from: (i32, i32, i32, i32),
    to: (i32, i32, i32, i32),
    out: &mut Samples,
) {
    let start = Instant::now();
    if screen.preview.is_none() {
        editor
            .apply(FloatingCmd::Scale {
                x: to.0,
                y: to.1,
                w: to.2,
                h: to.3,
            })
            .expect("scale");
        black_box(editor.selection().map(|m| m.outline()));
    } else {
        black_box(scale_edges(outline, from, to));
    }
    let moved = Instant::now();
    match &mut screen.preview {
        Some(preview) => {
            preview.render_scaled(editor, to);
        }
        None => screen.composite(editor),
    }
    let rendered = Instant::now();
    screen.blit();
    let end = Instant::now();
    let span = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1e3;
    out.moving.push(span(start, moved));
    out.render.push(span(moved, rendered));
    out.blit.push(span(rendered, end));
    out.total.push(span(start, end));
}

fn drive(
    editor: &mut Editor,
    screen: &mut Screen,
    scale: bool,
    samples: &mut Samples,
    peak: &mut usize,
) {
    let mut rng = Rng(23);
    let from = editor.selection().map_or((0, 0, 1, 1), |m| m.bounds());
    let outline = editor.selection().map_or_else(Vec::new, |m| m.outline());
    for _ in 0..EVENTS {
        if scale {
            let grow = rng.range(33) - 16;
            let to = (from.0, from.1, from.2 + grow, from.3 + grow);
            one_scale_event(editor, screen, &outline, from, to, samples);
        } else {
            let step = (rng.range(5) - 2, rng.range(5) - 2);
            one_event(editor, screen, step, samples);
        }
        *peak = (*peak).max(live_bytes());
    }
    if scale && screen.preview.is_some() {
        let release = Instant::now();
        editor
            .apply(FloatingCmd::Scale {
                x: from.0,
                y: from.1,
                w: from.2 + 5,
                h: from.3 + 5,
            })
            .expect("scale");
        println!("  release {:.1} ms", release.elapsed().as_secs_f64() * 1e3);
    }
}

fn row(label: &str, values: &mut [f64]) {
    values.sort_by(f64::total_cmp);
    println!(
        "  {label:<10}{:>9.2}{:>9.2}",
        percentile(values, 0.5),
        percentile(values, 0.99)
    );
}

fn study(side: i32, canvas: usize, one_to_one: bool, previews: bool, scale: bool) {
    let mut editor = Editor::new(document(canvas));
    editor.layer = ACTIVE_LAYER;
    let mut screen = Screen::new(canvas, one_to_one);
    screen.composite(&mut editor);
    let far = (canvas as i32 - side) / 2 + side - 1;
    let shape = editor.rect_shape(
        ((canvas as i32 - side) / 2, (canvas as i32 - side) / 2),
        (far, far),
    );
    editor
        .apply(SelectionCmd::Select {
            shape: Box::new(shape),
            op: SelectOp::Replace,
        })
        .expect("select");
    let idle = live_bytes();
    let pick_up = Instant::now();
    editor
        .apply(FloatingCmd::Lift { copy: false })
        .expect("lift");
    if previews && (side as usize).pow(2) >= PREVIEW_MIN_CELLS {
        screen.preview = DragPreview::build(&mut screen.comp, &editor, screen.view, area());
    }
    let pick_up_ms = pick_up.elapsed().as_secs_f64() * 1e3;
    let mut samples = Samples::default();
    let mut peak = live_bytes();
    drive(&mut editor, &mut screen, scale, &mut samples, &mut peak);
    let anchor = Instant::now();
    screen.preview = None;
    editor.apply(FloatingCmd::Anchor).expect("anchor");
    screen.composite(&mut editor);
    let anchor_ms = anchor.elapsed().as_secs_f64() * 1e3;
    let zoom = if one_to_one { "at 1:1" } else { "fitted" };
    println!("{side} x {side} on {canvas} x {canvas}, {zoom}");
    println!("  {:<10}{:>9}{:>9}   (ms)", "", "median", "p99");
    row("move", &mut samples.moving);
    row("render", &mut samples.render);
    row("blit", &mut samples.blit);
    row("total", &mut samples.total);
    let mb = |bytes: usize| bytes as f64 / 1_048_576.0;
    println!("  pick up {pick_up_ms:.1} ms, apply {anchor_ms:.1} ms");
    println!(
        "  memory: {:.1} MB idle, {:.1} MB peak while dragging",
        mb(idle),
        mb(peak)
    );
}

/// Prints the study for every size.
pub fn run(previews: bool, scale: bool) {
    for (side, canvas, one_to_one) in CASES {
        study(side, canvas, one_to_one, previews, scale);
    }
}
