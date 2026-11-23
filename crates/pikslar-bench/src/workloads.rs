//! Workloads shaped like real editing, on repeatable sprite-like art.
//!
//! The art mimics what Phase 0 measured on real sheets: mostly-empty layers, flat color
//! regions with outlines, ramps and a little dithering. Downloaded art is not in the repo,
//! so the gate must not depend on it.

use crate::gate::{Kind, Metric};
use crate::{live_bytes, total_allocs};
use pikslar_core::DrawCmd;
use pikslar_core::PaletteCmd;
use pikslar_core::{rgba, BlendMode, ColorMode, Damage, Document, Editor, Grid, Tool};
use pikslar_format::{piks, png};
use pikslar_render::{
    blit_canvas, Compositor, DragPreview, Rect, Surface, View, PREVIEW_MIN_CELLS,
};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

const REPEATS: usize = 7;
const SCREEN: (i32, i32) = (1400, 900);

pub(crate) struct Rng(pub(crate) u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 8
    }

    pub(crate) fn range(&mut self, n: u32) -> i32 {
        (self.next() % n) as i32
    }
}

pub(crate) fn ramp(hue: u32, step: u32) -> u32 {
    let base = [(170, 60, 50), (60, 130, 80), (60, 90, 170), (190, 150, 70)][(hue % 4) as usize];
    let scale = 90 + step * 45;
    let channel = |c: u32| (c * scale / 180).min(255) as u8;
    rgba(channel(base.0), channel(base.1), channel(base.2), 255)
}

/// One layer of blobs: outlines, flat fills, shaded edges and checker dithering.
pub(crate) fn sprite_layer(w: usize, h: usize, rng: &mut Rng) -> Vec<u32> {
    let mut px = vec![0u32; w * h];
    let area = (w * h) as u32;
    for _ in 0..(area / 2_400).max(4) {
        let hue = rng.next();
        let (bw, bh) = (6 + rng.range(30), 6 + rng.range(30));
        let (x0, y0) = (rng.range(w as u32), rng.range(h as u32));
        for y in y0..(y0 + bh).min(h as i32) {
            for x in x0..(x0 + bw).min(w as i32) {
                let edge = x == x0 || y == y0 || x == x0 + bw - 1 || y == y0 + bh - 1;
                let step = if edge {
                    0
                } else {
                    1 + ((x - x0) * 3 / bw) as u32
                };
                let dither = !edge && (x + y) % 2 == 0 && (x - x0) * 3 % bw < 2;
                px[y as usize * w + x as usize] = ramp(hue, step + dither as u32);
            }
        }
    }
    px
}

/// What a layer of this mode stores for these colors.
pub(crate) fn indexed_or_plain(doc: &mut Document, mode: ColorMode, cells: Vec<u32>) -> Vec<u32> {
    match mode {
        ColorMode::Rgba => cells,
        ColorMode::Palette => {
            let colors = Arc::make_mut(&mut doc.colors);
            cells
                .iter()
                .map(|&c| colors.id_for(c).expect("few colors"))
                .collect()
        }
    }
}

/// Eight layers, alternating palette and RGBA, with one multiply layer.
pub fn document(size: usize) -> Document {
    let mut doc = Document::new(size, size, ColorMode::Palette).expect("valid size");
    let mut rng = Rng(42);
    let mut layers = Vec::new();
    for i in 0..8 {
        let mode = if i % 2 == 0 {
            ColorMode::Palette
        } else {
            ColorMode::Rgba
        };
        if i > 0 {
            doc.add_layer(i, mode).expect("room");
        }
        let cells = sprite_layer(size, size, &mut rng);
        let stored = indexed_or_plain(&mut doc, mode, cells);
        layers.push(Grid::from_cells(size, size, &stored));
    }
    for (layer, grid) in layers.into_iter().enumerate() {
        doc.set_cel(layer, 0, grid).expect("canvas size");
    }
    doc.layers_mut()[5].blend = BlendMode::Multiply;
    doc
}

/// Runs `f` several times and returns the fastest, which best excludes system noise.
pub(crate) fn best_of(mut f: impl FnMut()) -> f64 {
    (0..REPEATS)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1e6
        })
        .fold(f64::MAX, f64::min)
}

pub(crate) fn push(out: &mut Vec<Metric>, name: &str, kind: Kind, value: f64) {
    out.push(Metric {
        name: name.to_string(),
        kind,
        value,
    });
}

pub(crate) struct Canvas {
    comp: Compositor,
    fb: Vec<u32>,
    view: View,
    xmap: Vec<i32>,
    changed: Vec<Rect>,
}

impl Canvas {
    pub(crate) fn new(size: usize) -> Self {
        let screen = Rect::new(0, 0, SCREEN.0, SCREEN.1);
        Canvas {
            comp: Compositor::new(size, size),
            fb: vec![0; (SCREEN.0 * SCREEN.1) as usize],
            view: View::fit(size as i32, size as i32, screen),
            xmap: Vec::with_capacity(SCREEN.0 as usize),
            changed: Vec::with_capacity(64),
        }
    }

    /// One frame of a selection drag, deciding as the app does how to show it.
    pub(crate) fn present_drag(&mut self, editor: &mut Editor, preview: &mut Option<DragPreview>) {
        let screen = Rect::new(0, 0, SCREEN.0, SCREEN.1);
        let cells = editor.floating_view().map_or(0, |f| f.placed.len());
        if preview.is_none() && cells >= PREVIEW_MIN_CELLS && !editor.has_damage() {
            *preview = DragPreview::build(&mut self.comp, editor, self.view, screen);
        }
        let mut target = Surface {
            px: &mut self.fb,
            w: SCREEN.0,
            h: SCREEN.1,
        };
        if let Some(active) = preview {
            active.render(editor);
            active.blit(&mut target, &mut self.xmap);
            return;
        }
        self.changed.clear();
        let damage = editor.take_damage();
        self.comp.update(editor, damage, &mut self.changed);
        let (w, h) = (self.comp.width() as i32, self.comp.height() as i32);
        let px = self.comp.pixels();
        blit_canvas(px, w, h, self.view, &mut target, screen, &mut self.xmap);
    }

    /// One frame of dragging a corner handle over a preview.
    pub(crate) fn present_scale(
        &mut self,
        editor: &Editor,
        preview: &mut Option<DragPreview>,
        to: (i32, i32, i32, i32),
    ) {
        let mut target = Surface {
            px: &mut self.fb,
            w: SCREEN.0,
            h: SCREEN.1,
        };
        if let Some(active) = preview {
            active.render_scaled(editor, to);
            active.blit(&mut target, &mut self.xmap);
        }
    }

    /// Everything between an input event and pixels ready to present.
    pub(crate) fn present(&mut self, editor: &mut Editor) {
        self.changed.clear();
        let damage: Damage = editor.take_damage();
        self.comp.update(editor, damage, &mut self.changed);
        let (w, h) = (self.comp.width() as i32, self.comp.height() as i32);
        let mut target = Surface {
            px: &mut self.fb,
            w: SCREEN.0,
            h: SCREEN.1,
        };
        for r in &self.changed {
            let clip = self.view.to_screen(*r);
            blit_canvas(
                self.comp.pixels(),
                w,
                h,
                self.view,
                &mut target,
                clip,
                &mut self.xmap,
            );
        }
    }
}

pub(crate) fn walk(rng: &mut Rng, size: i32, points: &mut [(i32, i32)]) {
    let (mut x, mut y) = (size / 2, size / 2);
    for p in points {
        x = (x + rng.range(9) - 4).clamp(0, size - 1);
        y = (y + rng.range(9) - 4).clamp(0, size - 1);
        *p = (x, y);
    }
}

pub(crate) fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * p) as usize]
}

/// One commit is a single short sample, so it repeats the same stroke and keeps the fastest.
fn commit_time(doc: &Document, points: &[(i32, i32)]) -> f64 {
    let mut editor = Editor::new(doc.clone());
    editor.layer = 2;
    editor.color = rgba(240, 220, 120, 255);
    (0..REPEATS)
        .map(|_| {
            editor
                .apply(DrawCmd::Begin {
                    tool: Tool::Pencil,
                    x: points[0].0,
                    y: points[0].1,
                })
                .expect("begin");
            for &(x, y) in &points[1..] {
                editor.apply(DrawCmd::To { x, y }).expect("drag");
            }
            let start = Instant::now();
            editor.apply(DrawCmd::Commit).expect("commit");
            let elapsed = start.elapsed().as_secs_f64() * 1e6;
            editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
            editor.take_damage();
            elapsed
        })
        .fold(f64::MAX, f64::min)
}

/// The number that matters most: CPU time from a pointer event to a ready frame.
fn stroke_latency(doc: &Document, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(doc.clone());
    let mut canvas = Canvas::new(doc.width);
    canvas.present(&mut editor);
    editor.color = rgba(240, 220, 120, 255);
    let mut points = vec![(0, 0); 3000];
    walk(&mut Rng(7), doc.width as i32, &mut points);
    let mut times = Vec::with_capacity(points.len());
    editor.layer = 2;
    editor
        .apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x: points[0].0,
            y: points[0].1,
        })
        .expect("begin");
    canvas.present(&mut editor);
    let allocs_before = total_allocs();
    for &(x, y) in &points[1..] {
        let start = Instant::now();
        editor.apply(DrawCmd::To { x, y }).expect("drag");
        canvas.present(&mut editor);
        times.push(start.elapsed().as_secs_f64() * 1e6);
    }
    let allocs = total_allocs() - allocs_before;
    editor.apply(DrawCmd::Commit).expect("commit");
    push(
        out,
        "stroke_commit",
        Kind::Micros,
        commit_time(doc, &points),
    );
    times.sort_by(f64::total_cmp);
    push(
        out,
        "stroke_event_median",
        Kind::Micros,
        percentile(&times, 0.5),
    );
    push(
        out,
        "stroke_event_p99",
        Kind::Micros,
        percentile(&times, 0.99),
    );
    push(
        out,
        "stroke_allocs_per_1000_events",
        Kind::Allocs,
        allocs as f64 * 1000.0 / times.len() as f64,
    );
}

fn compositing(doc: &Document, label: &str, out: &mut Vec<Metric>) {
    let editor = Editor::new(doc.clone());
    let mut comp = Compositor::new(doc.width, doc.height);
    let mut changed = Vec::new();
    let time = best_of(|| {
        changed.clear();
        comp.update(&editor, Damage::All, &mut changed);
        black_box(comp.pixels());
    });
    push(out, &format!("composite_full_{label}"), Kind::Micros, time);
}

/// Changing frame with the onion skin showing `depth` neighbours each way.
fn onion_compositing(doc: &Document, depth: usize, out: &mut Vec<Metric>) {
    let mut doc = doc.clone();
    for at in 1..7 {
        doc.add_frame(at, true).expect("room for frames");
        for layer in 0..doc.layers().len() {
            let own = doc.layers()[layer].cel(at).with_own_image();
            doc.put_cel(layer, at, own).expect("frame exists");
        }
    }
    let mut editor = Editor::new(doc);
    editor.frame = 3;
    let (width, height) = (editor.doc().width, editor.doc().height);
    let mut comp = Compositor::new(width, height);
    comp.onion = true;
    comp.onion_depth = depth;
    let mut changed = Vec::new();
    let time = best_of(|| {
        changed.clear();
        comp.update(&editor, Damage::All, &mut changed);
        black_box(comp.pixels());
    });
    let name = format!("composite_onion_depth{depth}_1024");
    push(out, &name, Kind::Micros, time);
}

fn full_screen_blit(doc: &Document, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(doc.clone());
    let mut canvas = Canvas::new(doc.width);
    canvas.present(&mut editor);
    let (w, h) = (doc.width as i32, doc.height as i32);
    let screen = Rect::new(0, 0, SCREEN.0, SCREEN.1);
    let time = best_of(|| {
        let mut target = Surface {
            px: &mut canvas.fb,
            w: SCREEN.0,
            h: SCREEN.1,
        };
        blit_canvas(
            canvas.comp.pixels(),
            w,
            h,
            canvas.view,
            &mut target,
            screen,
            &mut canvas.xmap,
        );
    });
    push(out, "blit_full_screen", Kind::Micros, time);
}

fn history(doc: &Document, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(doc.clone());
    let before = live_bytes();
    draw_history_strokes(&mut editor);
    push(
        out,
        "history_2000_strokes_retained",
        Kind::Bytes,
        (live_bytes() - before) as f64,
    );
    let depth = editor.history().undo_depth();
    let time = best_of(|| {
        editor
            .apply(pikslar_core::StepCmd::ScrubTo { depth: 0 })
            .expect("scrub");
        editor
            .apply(pikslar_core::StepCmd::ScrubTo { depth })
            .expect("scrub");
    });
    push(out, "history_undo_redo_all_2000", Kind::Micros, time);
}

/// The 2,000 short pencil strokes, over the 8 layers in turn, that the history lines undo and
/// redo. `history-scrub` draws the same ones.
pub(crate) fn draw_history_strokes(editor: &mut Editor) {
    let size = editor.doc().width as i32;
    let mut rng = Rng(11);
    let mut points = vec![(0, 0); 24];
    for i in 0..2000 {
        walk(&mut rng, size, &mut points);
        editor.layer = i % 8;
        editor.color = ramp(rng.next(), 2);
        editor
            .apply(DrawCmd::Begin {
                tool: Tool::Pencil,
                x: points[0].0,
                y: points[0].1,
            })
            .expect("begin");
        for &(x, y) in &points[1..] {
            editor.apply(DrawCmd::To { x, y }).expect("drag");
        }
        editor.apply(DrawCmd::Commit).expect("commit");
        editor.take_damage();
    }
}

const COMMANDS_MEASURED: usize = 1000;

fn tools(doc: &Document, out: &mut Vec<Metric>) {
    let mut editor = Editor::new(doc.clone());
    editor.color = rgba(10, 200, 200, 255);
    let time = best_of(|| {
        editor
            .apply(DrawCmd::Begin {
                tool: Tool::Fill,
                x: 3,
                y: 3,
            })
            .expect("begin");
        editor.apply(pikslar_core::StepCmd::Undo).expect("undo");
    });
    push(out, "flood_fill_and_undo", Kind::Micros, time);

    let time = best_of(|| {
        for _ in 0..COMMANDS_MEASURED {
            let _ = std::hint::black_box(editor.apply(pikslar_core::StepCmd::EndDrag));
        }
    });
    push(out, "command_dispatch_1000", Kind::Micros, time);

    let mut canvas = Canvas::new(doc.width);
    let mut flip = false;
    let time = best_of(|| {
        flip = !flip;
        let color = if flip {
            rgba(200, 40, 40, 255)
        } else {
            rgba(40, 40, 200, 255)
        };
        editor
            .apply(PaletteCmd::Recolor { id: 1, color })
            .expect("recolor");
        canvas.present(&mut editor);
    });
    push(out, "live_palette_recolor_full_frame", Kind::Micros, time);
}

fn files(doc: &Document, out: &mut Vec<Metric>) {
    let bytes = piks::save(doc);
    push(out, "project_file_size", Kind::Bytes, bytes.len() as f64);
    push(
        out,
        "project_save",
        Kind::Micros,
        best_of(|| drop(black_box(piks::save(doc)))),
    );
    let load = best_of(|| drop(black_box(piks::load(&bytes).expect("valid"))));
    push(out, "project_load", Kind::Micros, load);

    let px = doc.layers()[1].cel(0).to_cells(doc.width, doc.height);
    let image = png::Image {
        width: doc.width,
        height: doc.height,
        px,
    };
    let encoded = png::encode(&image).expect("a whole image");
    push(out, "png_file_size", Kind::Bytes, encoded.len() as f64);
    push(
        out,
        "png_encode",
        Kind::Micros,
        best_of(|| drop(black_box(png::encode(&image)))),
    );
    let decode = best_of(|| drop(black_box(png::decode(&encoded).expect("valid"))));
    push(out, "png_decode", Kind::Micros, decode);
}

/// Runs every workload.
pub fn measure() -> Vec<Metric> {
    let mut out = Vec::new();
    let before = live_bytes();
    let doc = document(512);
    push(
        &mut out,
        "document_512_retained",
        Kind::Bytes,
        (live_bytes() - before) as f64,
    );
    stroke_latency(&doc, &mut out);
    full_screen_blit(&doc, &mut out);
    compositing(&doc, "512", &mut out);
    let large = document(1024);
    compositing(&large, "1024", &mut out);
    onion_compositing(&large, 1, &mut out);
    onion_compositing(&large, 3, &mut out);
    history(&doc, &mut out);
    tools(&doc, &mut out);
    files(&doc, &mut out);
    crate::workloads_edit::measure(&doc, &large, &mut out);
    crate::stress::measure(&mut out);
    out
}
