//! `Command::may_paint` says which commands the paint check can refuse, so that `Editor::apply`
//! skips the check for the rest. These tests hold every kind of command to its claim, on
//! documents where the check refuses all it can: every layer locked, a reference or a group
//! active, and a drawing shared with a locked layer.

use super::{
    CanvasCmd, CelCmd, Command, DrawCmd, FloatingCmd, FrameCmd, LayerCmd, PaletteCmd, SelectionCmd,
    SliceCmd, StepCmd, TagCmd,
};
use crate::cel_drag::{CelDrag, DragKind, Landing};
use crate::color::rgba;
use crate::document::{BlendMode, ColorMode, Document};
use crate::editor::{Editor, Tool};
use crate::floating::Clip;
use crate::image::Image;
use crate::layer::{LayerKind, Turn};
use crate::mask::Mask;
use crate::selection::SelectOp;
use crate::slice::{Slice, SliceRect};
use crate::tween::Ease;
use std::collections::HashSet;
use std::sync::Arc;

const INK: u32 = rgba(10, 200, 30, 255);
/// One of each kind of command, as `every_kind` gives them.
const KINDS: usize = 74;

/// Two paint layers of three frames, each cel with a dot of its own at the top left and one at
/// (20, 20), frame 2 of layer 0 held
/// with frame 1, and the first cel of layer 0 linked to the second of layer 1.
fn drawn() -> Editor {
    let mut doc = Document::new(32, 32, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("room");
    let mut e = Editor::new(doc);
    for at in 1..3 {
        e.apply(FrameCmd::Add { at, copy: false }).expect("frame");
    }
    e.color = INK;
    for layer in 0..2 {
        for frame in 0..3 {
            (e.layer, e.frame) = (layer, frame);
            let x = layer as i32 * 4 + frame as i32;
            for (x, y) in [(x, 3), (20, 20)] {
                e.begin(Tool::Pencil, x, y).expect("begin");
                e.commit();
            }
        }
    }
    let layer = e.layer_id(0).expect("layer");
    e.apply(FrameCmd::HoldPrevious { layer, frame: 2 })
        .expect("hold");
    e.apply(CelCmd::Drag(CelDrag {
        layers: (0, 0),
        frames: (0, 0),
        to_layer: 1,
        to_frame: 1,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    }))
    .expect("link");
    (e.layer, e.frame) = (0, 0);
    e
}

/// The same document in a new editor, so no step of the setup can be undone.
fn fresh(e: &Editor) -> Editor {
    let mut out = Editor::new(e.doc.clone());
    (out.layer, out.frame) = (e.layer, e.frame);
    out
}

/// Documents on which the paint check refuses whatever it can.
fn refusing() -> Vec<(&'static str, Editor)> {
    let mut locked = drawn();
    locked.doc.layers.iter_mut().for_each(|l| l.locked = true);

    let mut reference = drawn();
    let image = Arc::new(crate::offset_tests::image_of(
        &[((1, 1), INK)].into_iter().collect(),
    ));
    let name = "Sketch".into();
    reference
        .apply(LayerCmd::PlaceReference {
            below: 0,
            image,
            name,
        })
        .expect("placed");
    assert_eq!(
        reference.doc.layers[reference.layer].kind,
        LayerKind::Reference
    );

    let mut group = drawn();
    let layer = group.layer_id(0).expect("layer");
    group.apply(LayerCmd::Group { layer }).expect("grouped");
    assert_eq!(group.doc.layers[group.layer].kind, LayerKind::Group);

    let mut shared = drawn();
    shared.doc.layers[1].locked = true;
    assert!(
        shared.shared_refusal(0, 0).is_some(),
        "linked to a locked cel"
    );

    vec![
        ("every layer locked", fresh(&locked)),
        ("a reference active", fresh(&reference)),
        ("a group active", fresh(&group)),
        ("a drawing shared with a locked layer", fresh(&shared)),
    ]
}

fn slice() -> Box<Slice> {
    let bounds = SliceRect {
        x: 1,
        y: 1,
        width: 4,
        height: 4,
    };
    Box::new(Slice {
        name: "Hit".into(),
        color: INK,
        bounds,
        pivot: None,
        center: None,
    })
}

/// One command of every kind, naming the active layer where a command names a layer.
fn every_kind(e: &Editor) -> Vec<Command> {
    let mut all = canvas_and_layer(e);
    all.extend(frame_tag_slice_palette(e));
    all.extend(step_draw_selection());
    all.extend(floating());
    all.extend(cel(e));
    all
}

fn canvas_and_layer(e: &Editor) -> Vec<Command> {
    let layer = e.layer_id(e.layer).expect("layer");
    let (width, height, dx, dy) = (40, 40, 2, 2);
    let image = Arc::new(Image::default());
    let name = String::from("New");
    let layers = [
        LayerCmd::Add {
            at: 1,
            mode: ColorMode::Rgba,
        },
        LayerCmd::Remove { layer },
        LayerCmd::Move { layer, to: 0 },
        LayerCmd::SetOpacity { layer, opacity: 9 },
        LayerCmd::SetVisible {
            layer,
            visible: false,
        },
        LayerCmd::SetLocked {
            layer,
            locked: false,
        },
        LayerCmd::SetBlend {
            layer,
            blend: BlendMode::Multiply,
        },
        LayerCmd::SetMode {
            layer,
            mode: ColorMode::Palette,
        },
        LayerCmd::Rename {
            layer,
            name: name.clone(),
        },
        LayerCmd::Duplicate { layer },
        LayerCmd::MergeDown { layer },
        LayerCmd::Group { layer },
        LayerCmd::Ungroup { layer },
        LayerCmd::Step { layer, up: true },
        LayerCmd::SetFolded {
            layer,
            folded: true,
        },
        LayerCmd::PlaceReference {
            below: 0,
            image,
            name,
        },
    ];
    let canvas = CanvasCmd::Resize {
        width,
        height,
        dx,
        dy,
    };
    std::iter::once(canvas.into())
        .chain(layers.into_iter().map(Command::from))
        .collect()
}

fn frame_tag_slice_palette(e: &Editor) -> Vec<Command> {
    let layer = e.layer_id(e.layer).expect("layer");
    let frames = [
        FrameCmd::Add { at: 1, copy: true },
        FrameCmd::Remove { from: 1, to: 1 },
        FrameCmd::Move { from: 0, to: 2 },
        FrameCmd::Retime {
            from: 0,
            durations: vec![40, 50],
        },
        FrameCmd::HoldPrevious { layer, frame: 1 },
        FrameCmd::HoldAcross {
            layer,
            source: 0,
            from: 1,
            to: 2,
        },
        FrameCmd::Release {
            layer,
            from: 0,
            to: 2,
        },
        FrameCmd::Duplicate { from: 0, to: 1 },
        FrameCmd::MoveRun {
            from: 0,
            to: 1,
            at: 1,
        },
        FrameCmd::SetRate { fps: Some(12) },
    ];
    let tags = [
        TagCmd::Add {
            name: "Walk".into(),
            from: 0,
            to: 1,
        },
        TagCmd::SetName {
            tag: 0,
            name: "Run".into(),
        },
        TagCmd::Remove { tag: 0 },
        TagCmd::CycleDirection { tag: 0 },
        TagCmd::SetRepeat { tag: 0, repeat: 2 },
        TagCmd::SetRange {
            tag: 0,
            from: 1,
            to: 2,
        },
    ];
    let slices = [
        SliceCmd::Add { slice: slice() },
        SliceCmd::Set {
            index: 0,
            slice: slice(),
        },
        SliceCmd::Remove { index: 0 },
    ];
    let palette = [
        PaletteCmd::AddSwatch { color: INK },
        PaletteCmd::RemoveSwatch { id: 1 },
        PaletteCmd::Recolor { id: 1, color: INK },
    ];
    let frames = frames.into_iter().map(Command::from);
    let tags = tags.into_iter().map(Command::from);
    let slices = slices.into_iter().map(Command::from);
    frames
        .chain(tags)
        .chain(slices)
        .chain(palette.into_iter().map(Command::from))
        .collect()
}

fn step_draw_selection() -> Vec<Command> {
    let steps = [
        StepCmd::EndDrag,
        StepCmd::AbortJoin,
        StepCmd::Undo,
        StepCmd::Redo,
        StepCmd::ScrubTo { depth: 0 },
    ];
    let tool = Tool::Pencil;
    let draws = [
        DrawCmd::Begin { tool, x: 2, y: 2 },
        DrawCmd::To { x: 5, y: 2 },
        DrawCmd::Commit,
        DrawCmd::Cancel,
        DrawCmd::Stroke {
            tool,
            path: vec![(1, 1), (4, 1)],
        },
        DrawCmd::EraseCell { x: 0, y: 3 },
        DrawCmd::DeleteSelected,
    ];
    let shape = || Box::new(Mask::rect(0, 0, 8, 8));
    let selections = [
        SelectionCmd::Select {
            shape: shape(),
            op: SelectOp::Replace,
        },
        SelectionCmd::Temporary { shape: shape() },
        SelectionCmd::All,
        SelectionCmd::None,
        SelectionCmd::Invert,
    ];
    let steps = steps.into_iter().map(Command::from);
    let draws = draws.into_iter().map(Command::from);
    steps
        .chain(draws)
        .chain(selections.into_iter().map(Command::from))
        .collect()
}

fn floating() -> Vec<Command> {
    let clip = Arc::new(Clip::from_image(2, 1, vec![INK, INK]));
    let all = [
        FloatingCmd::Lift { copy: false },
        FloatingCmd::Move {
            dx: 1,
            dy: 0,
            join: false,
        },
        FloatingCmd::Scale {
            x: 0,
            y: 0,
            w: 4,
            h: 4,
        },
        FloatingCmd::FlipHorizontal,
        FloatingCmd::FlipVertical,
        FloatingCmd::Rotate { clockwise: true },
        FloatingCmd::Anchor,
        FloatingCmd::Revert,
        FloatingCmd::Paste {
            clip: clip.clone(),
            x: 3,
            y: 3,
        },
        FloatingCmd::ReplacePasted { clip },
        FloatingCmd::GestureBegin,
        FloatingCmd::GestureEnd,
        FloatingCmd::GestureCancel,
    ];
    all.into_iter().map(Command::from).collect()
}

fn cel(e: &Editor) -> Vec<Command> {
    let layer = e.layer_id(e.layer).expect("layer");
    let frame = 0;
    let drag = CelDrag {
        layers: (e.layer, e.layer),
        frames: (0, 0),
        to_layer: e.layer,
        to_frame: 1,
        kind: DragKind::Move,
        landing: Landing::Overwrite,
    };
    let all = [
        CelCmd::Move {
            layer,
            frame,
            dx: 1,
            dy: 0,
        },
        CelCmd::MoveOne {
            layer,
            frame,
            dx: 1,
            dy: 0,
        },
        CelCmd::Turn {
            layer,
            frame,
            turn: Turn::FlipHorizontal,
        },
        CelCmd::Drag(drag),
        CelCmd::Tween {
            layer,
            from: 0,
            to: 2,
            ease: Ease::Linear,
        },
    ];
    all.into_iter().map(Command::from).collect()
}

/// The kind of a command, as its debug text names it: `Layer(Add`, `Step(EndDrag`.
fn kind(command: &Command) -> String {
    let text = format!("{command:?}");
    let family = text.find('(').map_or(text.len(), |at| at + 1);
    let end = text[family..]
        .find(|c: char| !c.is_alphanumeric())
        .map_or(text.len(), |at| family + at);
    text[..end].to_string()
}

#[test]
fn every_kind_of_command_is_sampled_once() {
    let e = drawn();
    let kinds: Vec<String> = every_kind(&e).iter().map(kind).collect();
    let distinct: HashSet<&String> = kinds.iter().collect();
    assert_eq!(distinct.len(), kinds.len(), "a kind twice: {kinds:?}");
    assert_eq!(
        kinds.len(),
        KINDS,
        "a kind added to Command needs a sample here"
    );
}

#[test]
fn the_check_passes_every_command_that_claims_to_paint_nothing() {
    for (doc, e) in refusing() {
        for command in every_kind(&e) {
            if command.may_paint() {
                continue;
            }
            let shown = kind(&command);
            assert_eq!(
                e.painted_layers(&command),
                Ok(Vec::new()),
                "{shown} on {doc}"
            );
            assert_eq!(e.drawn_cel(&command), Ok(None), "{shown} on {doc}");
            assert_eq!(e.refuse_painting(&command), Ok(()), "{shown} on {doc}");
        }
    }
}

#[test]
fn every_command_the_check_can_refuse_claims_to_paint() {
    let mut refused = HashSet::new();
    for (doc, e) in refusing() {
        for command in every_kind(&e) {
            let checked = e.refuse_painting(&command);
            let named = !e.painted_layers(&command).is_ok_and(|rows| rows.is_empty())
                || !matches!(e.drawn_cel(&command), Ok(None));
            if checked.is_err() || named {
                assert!(command.may_paint(), "{} on {doc}", kind(&command));
                refused.insert(kind(&command));
            }
        }
    }
    // Every kind that paints was seen refused somewhere, so the documents test something.
    let painting: HashSet<String> = every_kind(&drawn())
        .iter()
        .filter(|c| c.may_paint())
        .map(kind)
        .collect();
    let unseen: Vec<&String> = painting.difference(&refused).collect();
    assert!(unseen.is_empty(), "never refused: {unseen:?}");
}

/// What a cel shows: where its drawing is, its size and its pixels. A canvas resize moves
/// every cel, so for it the place is left out.
type Drawing = Option<(i32, i32, i32, i32, Vec<u32>)>;

fn drawing(cel: &crate::layer::Cel, placed: bool) -> Drawing {
    let (x, y, w, h) = cel.drawn_bounds()?;
    let pixels = cel.window(x, y, w as usize, h as usize);
    let (x, y) = if placed { (x, y) } else { (0, 0) };
    Some((x, y, w, h, pixels))
}

/// Every drawing in the document, by the id of its layer.
fn drawings(doc: &Document, placed: bool) -> Vec<(u32, Drawing)> {
    let layers = doc.layers.iter();
    layers
        .flat_map(|l| l.cels().iter().map(move |c| (l.id(), drawing(c, placed))))
        .collect()
}

/// The layers of `before` that `after` shows a drawing on that they did not have. Frames
/// added, removed, moved or held show drawings the layer had, so they draw nothing new.
fn drawn_on(before: &Document, after: &Document, placed: bool) -> Vec<u32> {
    let had = drawings(before, placed);
    let old = |id: u32| had.iter().any(|(layer, _)| *layer == id);
    let shows = drawings(after, placed).into_iter();
    shows
        .filter(|(layer, shown)| {
            old(*layer) && shown.is_some() && !had.contains(&(*layer, shown.clone()))
        })
        .map(|(layer, _)| layer)
        .collect()
}

/// The layers a command draws on, with the top left of the canvas selected, both straight after it and
/// once what it leaves in progress or floating is put down.
fn drawn_by(base: &Editor, command: Command) -> Vec<u32> {
    let placed = !matches!(command, Command::Canvas(_));
    let mut e = fresh(base);
    let shape = Box::new(Mask::rect(0, 0, 8, 8));
    let op = SelectOp::Replace;
    e.apply(SelectionCmd::Select { shape, op }).expect("select");
    let before = e.doc.clone();
    let _ = e.apply(command);
    let mut drawn = drawn_on(&before, &e.doc, placed);
    let finish: [Command; 3] = [
        DrawCmd::Commit.into(),
        StepCmd::EndDrag.into(),
        FloatingCmd::Anchor.into(),
    ];
    for command in finish {
        let _ = e.apply(command);
    }
    drawn.extend(drawn_on(&before, &e.doc, placed));
    drawn
}

#[test]
fn a_command_that_claims_to_paint_nothing_draws_on_no_layer() {
    for (doc, base) in refusing() {
        for command in every_kind(&base) {
            if command.may_paint() {
                continue;
            }
            let shown = kind(&command);
            let drawn = drawn_by(&base, command);
            assert!(drawn.is_empty(), "{shown} on {doc} drew on {drawn:?}");
        }
    }
}

/// The kinds that paint whose drawing the check above sees, on a document they may draw on.
/// The rest move drawings between cels, which it takes for no change, or do nothing on this
/// document; the tests of the check itself cover them.
const SEEN_DRAWING: [&str; 12] = [
    "Layer(SetMode",
    "Draw(Begin",
    "Draw(Stroke",
    "Draw(EraseCell",
    "Draw(DeleteSelected",
    "Floating(FlipHorizontal",
    "Floating(FlipVertical",
    "Floating(Rotate",
    "Floating(Paste",
    "Cel(Move",
    "Cel(MoveOne",
    "Cel(Turn",
];

#[test]
fn the_drawing_check_sees_what_painting_commands_draw() {
    let base = fresh(&drawn());
    let seen: Vec<String> = every_kind(&base)
        .into_iter()
        .filter(|c| c.may_paint())
        .filter_map(|c| {
            let shown = kind(&c);
            (!drawn_by(&base, c).is_empty()).then_some(shown)
        })
        .collect();
    assert_eq!(seen, SEEN_DRAWING);
}
