//! Doing real work with a file that was accepted. Loading without a crash is not enough: a bad
//! file must also survive being drawn, edited, undone and saved.

use crate::go;
use crate::mutate::Rng;
use crate::same::first_difference;
use pikslar_core::{
    rgba, tick_ms, ticks, ColorMode, Damage, Document, Editor, Mask, SelectOp, Tool,
    MAX_DURATION_MS, MAX_FRAME_RATE, MIN_DURATION_MS,
};
use pikslar_core::{CelCmd, DrawCmd, FloatingCmd, SelectionCmd, Turn};
use pikslar_core::{PaletteCmd, StepCmd};
use pikslar_format::piks;
use pikslar_render::{Compositor, Rect};

/// The frame rates the random models choose from, and milliseconds, as the app's menu offers
/// them plus the ends of the range.
pub const RATE_CHOICES: [Option<u8>; 7] = [
    None,
    Some(1),
    Some(12),
    Some(24),
    Some(30),
    Some(60),
    Some(100),
];

/// What any accepted document must promise the rest of the program.
pub fn check_shape(doc: &Document) -> Result<(), String> {
    if doc.layers().is_empty() || doc.frames.is_empty() {
        return Err("a document with no layers or frames".to_owned());
    }
    let frames = doc.frames.len();
    for (index, layer) in doc.layers().iter().enumerate() {
        if layer.frame_count() != frames || layer.links().count() != frames {
            return Err(format!("layer {index} does not match the frame count"));
        }
    }
    if doc.tags.iter().any(|t| t.from > t.to || t.to >= frames) {
        return Err("a tag outside the frames".to_owned());
    }
    doc.check_links()
        .map_err(|fault| format!("links: {fault}"))?;
    let timing = MIN_DURATION_MS..=MAX_DURATION_MS;
    if doc.frames.iter().any(|f| !timing.contains(&f.duration_ms)) {
        return Err("a frame timing out of range".to_owned());
    }
    check_timing(doc)
}

/// Frame times in range, a frame rate in range, and times on whole ticks when timed by rate.
fn check_timing(doc: &Document) -> Result<(), String> {
    if doc
        .frame_rate
        .is_some_and(|fps| fps == 0 || fps > MAX_FRAME_RATE)
    {
        return Err("a frame rate out of range".to_owned());
    }
    if let Some(fps) = doc.frame_rate {
        for frame in &doc.frames {
            let ms = frame.duration_ms;
            if ms != tick_ms(ticks(ms, fps), fps) {
                return Err(format!(
                    "a frame of {ms} ms is not whole ticks at {fps} fps"
                ));
            }
        }
    }
    Ok(())
}

fn point(rng: &mut Rng, doc: &Document) -> (i32, i32) {
    (
        rng.between(-2, doc.width as i32 + 1),
        rng.between(-2, doc.height as i32 + 1),
    )
}

fn stroke(editor: &mut Editor, rng: &mut Rng, tool: Tool) {
    let (a, b) = (point(rng, editor.doc()), point(rng, editor.doc()));
    if editor
        .apply(DrawCmd::Begin {
            tool,
            x: a.0,
            y: a.1,
        })
        .is_ok()
    {
        go(editor, DrawCmd::To { x: b.0, y: b.1 });
        go(editor, DrawCmd::Commit);
    }
}

fn move_selection(editor: &mut Editor, rng: &mut Rng) {
    let (x, y) = point(rng, editor.doc());
    let shape = Mask::rect(x, y, 1 + rng.below(20) as i32, 1 + rng.below(20) as i32);
    go(
        editor,
        SelectionCmd::Select {
            shape: Box::new(shape),
            op: SelectOp::Replace,
        },
    );
    // A locked layer, a reference or a group refuses the lift, and nothing floats.
    let lifted = editor.apply(FloatingCmd::Lift {
        copy: rng.chance(2),
    });
    if lifted.is_ok_and(|outcome| outcome.changed) {
        go(
            editor,
            FloatingCmd::Move {
                dx: rng.between(-4, 4),
                dy: rng.between(-4, 4),
                join: true,
            },
        );
        match rng.below(4) {
            0 => go(editor, FloatingCmd::FlipHorizontal),
            1 => go(editor, FloatingCmd::FlipVertical),
            2 => go(
                editor,
                FloatingCmd::Rotate {
                    clockwise: rng.chance(2),
                },
            ),
            _ => {}
        }
        go(editor, FloatingCmd::Anchor);
    }
    go(editor, SelectionCmd::None);
}

/// Moves or turns a whole cel, carrying pixels past the canvas edge.
fn move_cel(editor: &mut Editor, rng: &mut Rng) {
    let (layer, frame) = (editor.layer_id(editor.layer).expect("layer"), editor.frame);
    if rng.chance(2) {
        let far = if rng.chance(4) { 30_000 } else { 40 };
        for _ in 0..1 + rng.below(3) {
            let (dx, dy) = (rng.between(-far, far), rng.between(-far, far));
            go(
                editor,
                CelCmd::Move {
                    layer,
                    frame,
                    dx,
                    dy,
                },
            );
        }
        if rng.chance(4) {
            go(editor, StepCmd::AbortJoin);
        }
        go(editor, StepCmd::EndDrag);
    } else {
        let turn = [
            Turn::FlipHorizontal,
            Turn::FlipVertical,
            Turn::Clockwise,
            Turn::CounterClockwise,
        ][rng.below(4)];
        go(editor, CelCmd::Turn { layer, frame, turn });
    }
}

fn change_structure(editor: &mut Editor, rng: &mut Rng) {
    let (layer, frame) = (editor.layer, editor.frame);
    let _ = match rng.below(6) {
        0 => editor.apply(pikslar_core::FrameCmd::Add {
            at: frame,
            copy: true,
        }),
        1 => editor.apply(pikslar_core::LayerCmd::Add {
            at: layer,
            mode: ColorMode::Rgba,
        }),
        2 => editor.apply(pikslar_core::FrameCmd::Remove {
            from: frame,
            to: frame,
        }),
        3 => editor.apply(pikslar_core::LayerCmd::Remove {
            layer: editor.layer_id(layer).expect("layer"),
        }),
        4 => editor.apply(pikslar_core::FrameCmd::HoldPrevious {
            layer: editor.layer_id(layer).expect("layer"),
            frame,
        }),
        _ => editor.apply(pikslar_core::TagCmd::Add {
            name: "t".into(),
            from: frame,
            to: frame,
        }),
    };
}

fn edit_once(editor: &mut Editor, rng: &mut Rng) {
    editor.layer = rng.below(editor.doc().layers().len());
    editor.frame = rng.below(editor.doc().frames.len());
    editor.color = rgba(
        rng.next_u64() as u8,
        rng.next_u64() as u8,
        rng.next_u64() as u8,
        rng.next_u64() as u8,
    );
    match rng.below(9) {
        8 => move_cel(editor, rng),
        0 => stroke(editor, rng, Tool::Pencil),
        1 => stroke(editor, rng, Tool::Eraser),
        2 => stroke(editor, rng, Tool::Line),
        3 => stroke(editor, rng, Tool::Rect),
        4 => stroke(editor, rng, Tool::Fill),
        5 => move_selection(editor, rng),
        6 => change_structure(editor, rng),
        _ => {
            let id = rng.below(editor.doc().colors.len() + 1) as u32;
            go(editor, StepCmd::EndDrag);
            let _ = editor.apply(PaletteCmd::Recolor {
                id,
                color: editor.color,
            });
        }
    }
}

fn draw(editor: &mut Editor, compositor: &mut Compositor, rects: &mut Vec<Rect>) {
    let damage = editor.take_damage();
    compositor.update(editor, damage, rects);
}

/// Every frame, drawn from nothing.
fn draw_all(editor: &mut Editor, compositor: &mut Compositor) {
    let keep = editor.frame;
    for frame in 0..editor.doc().frames.len() {
        editor.frame = frame;
        compositor.update(editor, Damage::All, &mut Vec::new());
    }
    editor.frame = keep;
}

fn check_saves(doc: &Document) -> Result<Vec<u8>, String> {
    let saved = piks::save(doc);
    let reopened = piks::load(&saved).map_err(|e| format!("its own save will not open: {e}"))?;
    check_shape(&reopened)?;
    if piks::save(&reopened) != saved {
        let what = first_difference(doc, &reopened).unwrap_or_else(|| "bytes only".to_owned());
        return Err(format!("saving a reopened document changed it: {what}"));
    }
    Ok(saved)
}

/// Draws, edits, undoes, redoes and saves. An `Err` or a panic is a bug in the program.
pub fn exercise(editor: &mut Editor, rng: &mut Rng) -> Result<(), String> {
    check_shape(editor.doc())?;
    let mut compositor = Compositor::new(editor.doc().width, editor.doc().height);
    let mut rects = Vec::new();
    draw_all(editor, &mut compositor);
    check_saves(editor.doc())?;
    let before = editor.doc().clone();
    for _ in 0..12 {
        edit_once(editor, rng);
        check_shape(editor.doc())?;
        if compositor_fits(&compositor, editor) {
            draw(editor, &mut compositor, &mut rects);
        }
    }
    let edited = check_saves(editor.doc())?;
    while editor.apply(StepCmd::Undo).expect("undo").changed {}
    if let Some(what) = first_difference(&before, editor.doc()) {
        return Err(format!("undoing everything left a difference: {what}"));
    }
    while editor.apply(StepCmd::Redo).expect("redo").changed {}
    if piks::save(editor.doc()) != edited {
        return Err("redoing everything did not restore the edits".to_owned());
    }
    draw_all(editor, &mut compositor);
    Ok(())
}

fn compositor_fits(compositor: &Compositor, editor: &Editor) -> bool {
    compositor.pixels().len() == editor.doc().width * editor.doc().height
}
