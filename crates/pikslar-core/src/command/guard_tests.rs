//! A locked layer refuses every command that would draw on it, and nothing else.

use super::{
    CanvasCmd, CelCmd, Command, DrawCmd, FloatingCmd, FrameCmd, LayerCmd, SelectionCmd, StepCmd,
};
use crate::cel_drag::{CelDrag, DragKind, Landing};
use crate::color::rgba;
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::{Editor, Tool};
use crate::floating::Clip;
use crate::layer::{Cel, Turn};
use crate::mask::Mask;
use crate::offset_tests::image_of;
use crate::selection::SelectOp;
use std::collections::HashMap;
use std::sync::Arc;

const RED: u32 = rgba(200, 10, 10, 255);

/// A 32x32 editor with two drawn layers and two frames, each with its own drawing.
fn editor() -> Editor {
    let mut e = Editor::new(Document::new(32, 32, ColorMode::Rgba).expect("valid"));
    e.doc.add_layer(1, ColorMode::Rgba).expect("layer");
    for layer in 0..2 {
        let pixels = HashMap::from([((3 + layer, 4), RED), ((20, 20), RED)]);
        e.doc.layers[layer as usize]
            .put_cel(0, Cel::from_image(image_of(&pixels)))
            .expect("frame");
    }
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("frame");
    e.frame = 0;
    e
}

fn lock(e: &mut Editor, index: usize) {
    let layer = e.layer_id(index).expect("layer");
    e.apply(LayerCmd::SetLocked {
        layer,
        locked: true,
    })
    .expect("lock");
}

fn select(e: &mut Editor) {
    let shape = Box::new(Mask::rect(0, 0, 8, 8));
    let op = SelectOp::Replace;
    e.apply(SelectionCmd::Select { shape, op }).expect("select");
}

fn stroke(tool: Tool) -> impl Fn(&mut Editor) -> Command {
    move |_| {
        DrawCmd::Stroke {
            tool,
            path: vec![(1, 4), (6, 4)],
        }
        .into()
    }
}

fn cel(e: &Editor, index: usize, command: impl Fn(super::LayerId) -> CelCmd) -> Command {
    command(e.layer_id(index).expect("layer")).into()
}

fn drag(layers: usize, to_layer: usize) -> impl Fn(&mut Editor) -> Command {
    move |_| {
        CelCmd::Drag(CelDrag {
            layers: (layers, layers),
            frames: (0, 0),
            to_layer,
            to_frame: 1,
            kind: DragKind::Move,
            landing: Landing::Overwrite,
        })
        .into()
    }
}

type Make = Box<dyn Fn(&mut Editor) -> Command>;

/// Each command that draws on layer 0, which is active, after any setup it needs.
fn painting() -> Vec<(&'static str, Make)> {
    vec![
        ("pencil", Box::new(stroke(Tool::Pencil))),
        ("eraser", Box::new(stroke(Tool::Eraser))),
        ("fill", Box::new(stroke(Tool::Fill))),
        ("line", Box::new(stroke(Tool::Line))),
        ("rectangle", Box::new(stroke(Tool::Rect))),
        (
            "a pressed tool",
            Box::new(|_| {
                DrawCmd::Begin {
                    tool: Tool::Pencil,
                    x: 2,
                    y: 2,
                }
                .into()
            }),
        ),
        (
            "a cell erased",
            Box::new(|_| DrawCmd::EraseCell { x: 3, y: 4 }.into()),
        ),
        (
            "delete",
            Box::new(|e| {
                select(e);
                DrawCmd::DeleteSelected.into()
            }),
        ),
        (
            "lift",
            Box::new(|e| {
                select(e);
                FloatingCmd::Lift { copy: false }.into()
            }),
        ),
        (
            "a selection flipped",
            Box::new(|e| {
                select(e);
                FloatingCmd::FlipHorizontal.into()
            }),
        ),
        (
            "a selection flipped upside down",
            Box::new(|e| {
                select(e);
                FloatingCmd::FlipVertical.into()
            }),
        ),
        (
            "a selection turned",
            Box::new(|e| {
                select(e);
                FloatingCmd::Rotate { clockwise: true }.into()
            }),
        ),
        (
            "paste",
            Box::new(|_| {
                let clip = Arc::new(Clip::from_image(2, 1, vec![RED, RED]));
                FloatingCmd::Paste { clip, x: 9, y: 9 }.into()
            }),
        ),
        (
            "a cel moved",
            Box::new(|e| {
                cel(e, 0, |layer| CelCmd::Move {
                    layer,
                    frame: 0,
                    dx: 1,
                    dy: 0,
                })
            }),
        ),
        (
            "a cel flipped",
            Box::new(|e| {
                cel(e, 0, |layer| CelCmd::Turn {
                    layer,
                    frame: 0,
                    turn: Turn::FlipHorizontal,
                })
            }),
        ),
        ("cels dragged onto it", Box::new(drag(1, 0))),
        ("cels dragged off it", Box::new(drag(0, 1))),
        (
            "a hold",
            Box::new(|e| {
                let layer = e.layer_id(0).expect("layer");
                FrameCmd::HoldPrevious { layer, frame: 1 }.into()
            }),
        ),
        (
            "a hold across frames",
            Box::new(|e| {
                let layer = e.layer_id(0).expect("layer");
                let (source, from, to) = (0, 1, 1);
                FrameCmd::HoldAcross {
                    layer,
                    source,
                    from,
                    to,
                }
                .into()
            }),
        ),
        (
            "a color mode",
            Box::new(|e| {
                let layer = e.layer_id(0).expect("layer");
                let mode = ColorMode::Palette;
                LayerCmd::SetMode { layer, mode }.into()
            }),
        ),
    ]
}

#[test]
fn every_drawing_command_is_refused_on_a_locked_layer_and_runs_on_an_unlocked_one() {
    for (name, make) in painting() {
        let mut free = editor();
        let command = make(&mut free);
        let before = free.doc.clone();
        let outcome = free.apply(command);
        free.apply(DrawCmd::Commit).expect("commit");
        free.apply(StepCmd::EndDrag).expect("end");
        assert!(
            outcome.is_ok() && (free.doc != before || free.is_floating()),
            "{name} does nothing on an unlocked layer: {outcome:?}"
        );

        let mut locked = editor();
        lock(&mut locked, 0);
        let command = make(&mut locked);
        let (before, revision) = (locked.doc.clone(), locked.revision());
        let pushes = locked.history().pushes();
        let outcome = locked.apply(command);
        assert_eq!(outcome, Err(LimitError::Locked), "{name}");
        assert!(
            locked.doc == before && !locked.is_floating(),
            "{name} changed it"
        );
        assert_eq!(
            (locked.revision(), locked.history().pushes()),
            (revision, pushes),
            "{name}"
        );
    }
}

#[test]
fn looking_and_changing_the_rest_of_the_document_are_not_refused() {
    let mut e = editor();
    lock(&mut e, 0);
    let layer = e.layer_id(0).expect("layer");
    let picker = DrawCmd::Stroke {
        tool: Tool::Picker,
        path: vec![(3, 4)],
    };
    let others: Vec<Command> = vec![
        picker.into(),
        LayerCmd::SetOpacity { layer, opacity: 9 }.into(),
        LayerCmd::SetVisible {
            layer,
            visible: false,
        }
        .into(),
        FrameCmd::Add { at: 2, copy: true }.into(),
        FrameCmd::Release {
            layer,
            from: 0,
            to: 1,
        }
        .into(),
        CanvasCmd::Resize {
            width: 40,
            height: 40,
            dx: 4,
            dy: 4,
        }
        .into(),
        CanvasCmd::SetBlending {
            blending: crate::Blending::Aseprite,
        }
        .into(),
        SelectionCmd::All.into(),
    ];
    for command in others {
        let shown = format!("{command:?}");
        assert!(e.apply(command).is_ok(), "{shown}");
    }
    e.layer = 1;
    assert!(
        {
            let c = stroke(Tool::Pencil)(&mut e);
            e.apply(c)
        }
        .is_ok(),
        "the other layer"
    );
}

#[test]
fn a_lock_is_saved_in_the_document_and_never_undone() {
    let mut e = editor();
    {
        let c = stroke(Tool::Pencil)(&mut e);
        e.apply(c)
    }
    .expect("stroke");
    let (pushes, revision) = (e.history().pushes(), e.revision());
    lock(&mut e, 0);
    assert!(e.doc.layers[0].locked);
    assert_eq!(e.history().pushes(), pushes, "no step");
    assert!(e.revision() > revision, "the file changed");
    let drawn = e.doc.layers[0].cels().to_vec();
    e.apply(StepCmd::Undo).expect("undo");
    assert!(e.doc.layers[0].locked, "still locked after undo");
    assert_ne!(
        e.doc.layers[0].cels(),
        &drawn[..],
        "undo still takes back pixels"
    );
    e.apply(StepCmd::Redo).expect("redo");
    assert_eq!(e.doc.layers[0].cels(), &drawn[..]);
    assert_eq!(e.paint_refusal(0), Some(LimitError::Locked));
    assert_eq!(e.paint_refusal(1), None);
    assert_eq!(e.paint_refusal(9), None, "no such layer");
}

#[test]
fn locking_the_active_layer_puts_down_what_is_being_drawn_first() {
    let mut e = editor();
    select(&mut e);
    e.apply(FloatingCmd::Lift { copy: false }).expect("lift");
    let moved = FloatingCmd::Move {
        dx: 5,
        dy: 0,
        join: false,
    };
    e.apply(moved).expect("move");
    lock(&mut e, 0);
    assert!(!e.is_floating(), "the floating pixels are down");
    let at = |e: &Editor, x, y| e.doc.layers[0].cel(0).get(x, y);
    assert_eq!(at(&e, 8, 4), RED, "where they were moved to");
    assert_eq!(at(&e, 3, 4), 0);

    let mut e = editor();
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 10,
        y: 10,
    })
    .expect("press");
    lock(&mut e, 0);
    assert_ne!(e.doc.layers[0].cel(0).get(10, 10), 0, "the stroke is in");
    e.apply(DrawCmd::To { x: 12, y: 10 })
        .expect("nothing to continue");
    assert_eq!(e.doc.layers[0].cel(0).get(12, 10), 0);
}
