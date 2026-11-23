//! `pikslar-bench history-study`: what one undo step costs, for each kind of command, on a few
//! documents. For each it reports what history says it keeps, what it really keeps alive, and
//! how long the command, its undo and its redo take. Outside the gate's baseline.

use crate::stress::stress_document;
use pikslar_core::{
    rgba, CelCmd, ColorMode, Command, Document, DrawCmd, Editor, FloatingCmd, FrameCmd, Grid,
    LayerCmd, Mask, PaletteCmd, SelectOp, SelectionCmd, StepCmd, TagCmd, Tool, Turn,
};
use pikslar_format::piks;
use std::time::Instant;

const REPEATS: usize = 9;

/// A long animation: few pixels on each of many frames, with holds.
pub(crate) fn long_document(layers: usize, frames: usize) -> Document {
    let side = 256;
    let mut doc = Document::new(side, side, ColorMode::Rgba).expect("valid size");
    for layer in 1..layers {
        doc.add_layer(layer, ColorMode::Rgba).expect("room");
    }
    for frame in 1..frames {
        doc.add_frame(frame, false).expect("room");
    }
    for layer in 0..layers {
        for frame in (0..frames).filter(|f| f % 4 == 0) {
            let mut px = vec![0u32; side * side];
            let at = (frame * 7 + layer * 31) % (side * side - 64);
            px[at..at + 24].fill(rgba(200, 40 + layer as u8, frame as u8, 255));
            doc.set_cel(layer, frame, Grid::from_cells(side, side, &px))
                .expect("canvas size");
        }
    }
    doc
}

/// Bytes the editor keeps beyond its document, measured by dropping it and keeping a copy of the
/// document, which shares every image.
pub(crate) fn kept_beyond_document(editor: Editor) -> usize {
    let with = crate::live_bytes();
    let doc = editor.doc().clone();
    drop(editor);
    let without = crate::live_bytes();
    drop(doc);
    with.saturating_sub(without)
}

fn micros(f: impl FnOnce()) -> f64 {
    let start = Instant::now();
    f();
    start.elapsed().as_secs_f64() * 1e6
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// The document as saved, so each measurement starts from its own copy and nothing outside the
/// editor keeps an old state alive, as in the program.
pub(crate) struct Saved<'a> {
    doc: &'a Document,
    bytes: Vec<u8>,
}

/// One command's cost. `setup` runs outside the measurement, as a selection before a lift does.
fn measure(saved: &Saved, name: &str, setup: &dyn Fn(&mut Editor), run: &dyn Fn(&mut Editor)) {
    let doc = saved.doc;
    let fresh = || {
        let mut e = Editor::new(piks::load(&saved.bytes).expect("own save"));
        e.layer = doc.layers().len() / 2;
        e.frame = doc.frames.len() / 2;
        setup(&mut e);
        e.apply(StepCmd::EndDrag).expect("end");
        e
    };
    let base = kept_beyond_document(fresh());
    let mut e = fresh();
    let said_before = e.history().memory_used();
    run(&mut e);
    e.apply(StepCmd::EndDrag).expect("end");
    let said = e.history().memory_used() - said_before;
    let all = kept_beyond_document(e).saturating_sub(base);
    let workspace = working_memory(name, &fresh, run, said_before, base);
    let kept = all.saturating_sub(workspace);
    let (mut apply, mut undo, mut redo) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..REPEATS {
        let mut e = fresh();
        apply.push(micros(|| run(&mut e)));
        e.apply(StepCmd::EndDrag).expect("end");
        undo.push(micros(|| {
            let _ = e.apply(StepCmd::Undo);
        }));
        redo.push(micros(|| {
            let _ = e.apply(StepCmd::Redo);
        }));
    }
    println!(
        "{name:<38} said {said:>9} B  kept {kept:>9} B  work {workspace:>7} B  apply {:>7.1} us  undo {:>6.1} us  redo {:>6.1} us",
        median(apply),
        median(undo),
        median(redo)
    );
}

/// What the editor keeps for its own work after `run`, such as the overlay's tiles: the same
/// again, with the step undone and then dropped from history by a new step.
fn working_memory(
    name: &str,
    fresh: &dyn Fn() -> Editor,
    run: &dyn Fn(&mut Editor),
    said_before: usize,
    base: usize,
) -> usize {
    let mut e = fresh();
    run(&mut e);
    e.apply(StepCmd::EndDrag).expect("end");
    let _ = e.apply(StepCmd::Undo);
    let (layer, frame) = (e.layer_id(0).expect("layer"), 0);
    go(
        &mut e,
        CelCmd::Move {
            layer,
            frame,
            dx: 1,
            dy: 0,
        },
    );
    // A cel move is a dragged value, which sets redo aside until it ends.
    e.apply(StepCmd::EndDrag).expect("end");
    assert!(!e.history().can_redo(), "{name}: the step is gone");
    // History's lists and counts keep their room once emptied, and the move that replaced the
    // step is history's too. What history says of those is exact, so it is taken off.
    let history = e.history().memory_used().saturating_sub(said_before);
    kept_beyond_document(e).saturating_sub(base + history)
}

fn go(e: &mut Editor, command: impl Into<Command>) {
    e.apply(command).expect("command applies");
}

fn nothing(_: &mut Editor) {}

fn selected(e: &mut Editor) {
    let shape = Box::new(Mask::rect(10, 10, 60, 60));
    go(
        e,
        SelectionCmd::Select {
            shape,
            op: SelectOp::Replace,
        },
    );
}

/// Every kind of command on one document.
fn commands(label: &str, doc: &Document) {
    let s = &Saved {
        doc,
        bytes: piks::save(doc),
    };
    println!(
        "--- {label}: {} layers, {} frames, {}x{}",
        doc.layers().len(),
        doc.frames.len(),
        doc.width,
        doc.height
    );
    let m = |s: &str| format!("{label}_{s}");
    pixel_commands(s, &m);
    frame_commands(s, &m);
    timing_commands(s, &m);
    layer_commands(s, &m);
    palette_commands(s, &m);
    cel_commands(s, &m);
}

/// Strokes and fills.
fn pixel_commands(s: &Saved, m: &dyn Fn(&str) -> String) {
    let n = &nothing;
    measure(s, &m("stroke"), n, &|e| {
        e.color = rgba(250, 10, 10, 255);
        let path = (0..40).map(|i| (10 + i, 20 + i / 2)).collect();
        go(
            e,
            DrawCmd::Stroke {
                tool: Tool::Pencil,
                path,
            },
        );
    });
    measure(s, &m("fill"), n, &|e| {
        go(
            e,
            DrawCmd::Begin {
                tool: Tool::Fill,
                x: 3,
                y: 3,
            },
        )
    });
}

/// Adding, removing and moving frames.
fn frame_commands(s: &Saved, m: &dyn Fn(&str) -> String) {
    let n = &nothing;
    let frames = s.doc.frames.len();
    measure(s, &m("frame_add_middle"), n, &|e| {
        go(
            e,
            FrameCmd::Add {
                at: frames / 2,
                copy: false,
            },
        )
    });
    measure(s, &m("frame_copy_middle"), n, &|e| {
        go(
            e,
            FrameCmd::Add {
                at: frames / 2,
                copy: true,
            },
        )
    });
    measure(s, &m("frame_remove_middle"), n, &|e| {
        go(
            e,
            FrameCmd::Remove {
                from: frames / 2,
                to: frames / 2,
            },
        )
    });
    measure(s, &m("frame_move_first_to_last"), n, &|e| {
        go(
            e,
            FrameCmd::Move {
                from: 0,
                to: frames - 1,
            },
        )
    });
    measure(s, &m("frame_move_by_one"), n, &|e| {
        go(e, FrameCmd::Move { from: 2, to: 3 })
    });
    // Frames 2 and 3 show nothing, so that move changes nothing. Frame 0 shows a drawing.
    measure(s, &m("frame_move_drawn_by_one"), n, &|e| {
        go(e, FrameCmd::Move { from: 0, to: 1 })
    });
}

/// Frame timing and holds.
fn timing_commands(s: &Saved, m: &dyn Fn(&str) -> String) {
    let n = &nothing;
    measure(s, &m("frame_retime"), n, &|e| {
        go(
            e,
            FrameCmd::Retime {
                from: 1,
                durations: vec![250],
            },
        )
    });
    measure(s, &m("hold_previous"), n, &|e| {
        let layer = e.layer_id(e.layer).expect("layer");
        go(
            e,
            FrameCmd::HoldPrevious {
                layer,
                frame: e.frame,
            },
        )
    });
}

/// Adding, removing and changing layers.
fn layer_commands(s: &Saved, m: &dyn Fn(&str) -> String) {
    let n = &nothing;
    let layers = s.doc.layers().len();
    measure(s, &m("layer_add"), n, &|e| {
        go(
            e,
            LayerCmd::Add {
                at: layers,
                mode: ColorMode::Rgba,
            },
        )
    });
    measure(s, &m("layer_remove"), n, &|e| {
        let layer = e.layer_id(0).expect("layer");
        go(e, LayerCmd::Remove { layer })
    });
    measure(s, &m("layer_opacity"), n, &|e| {
        let layer = e.layer_id(0).expect("layer");
        go(
            e,
            LayerCmd::SetOpacity {
                layer,
                opacity: 100,
            },
        )
    });
}

/// Palette and tags.
fn palette_commands(s: &Saved, m: &dyn Fn(&str) -> String) {
    let n = &nothing;
    measure(s, &m("swatch_add"), n, &|e| {
        go(
            e,
            PaletteCmd::AddSwatch {
                color: rgba(1, 2, 3, 255),
            },
        )
    });
    measure(s, &m("tag_add"), n, &|e| {
        go(
            e,
            TagCmd::Add {
                name: "walk".into(),
                from: 0,
                to: 1,
            },
        )
    });
}

/// Cels and floating pixels.
fn cel_commands(s: &Saved, m: &dyn Fn(&str) -> String) {
    let n = &nothing;
    measure(s, &m("cel_move"), n, &|e| {
        let layer = e.layer_id(e.layer).expect("layer");
        go(
            e,
            CelCmd::Move {
                layer,
                frame: e.frame,
                dx: 5,
                dy: -3,
            },
        )
    });
    measure(s, &m("cel_turn"), n, &|e| {
        let layer = e.layer_id(e.layer).expect("layer");
        go(
            e,
            CelCmd::Turn {
                layer,
                frame: e.frame,
                turn: Turn::Clockwise,
            },
        )
    });
    measure(s, &m("lift_move_anchor"), &selected, &|e| {
        go(e, FloatingCmd::Lift { copy: false });
        go(
            e,
            FloatingCmd::Move {
                dx: 4,
                dy: 4,
                join: false,
            },
        );
        go(e, FloatingCmd::Anchor);
    });
}

pub fn run() {
    commands("stress", &stress_document());
    commands("long_12x2000", &long_document(12, 2000));
    commands("long_4x4000", &long_document(4, 4000));
    crate::session_study::run();
}
