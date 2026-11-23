//! The document as shown, which export reads: floating pixels where they are, in a copy, and
//! nothing in the editor changed.

use crate::color::rgba;
use crate::document::{ColorMode, Document};
use crate::editor::{Editor, Tool};
use crate::mask::Mask;
use crate::selection::SelectOp;
use crate::{FloatingCmd, FrameCmd, LayerCmd};
use std::borrow::Cow;

const RED: u32 = rgba(255, 0, 0, 255);

fn dot(e: &mut Editor, x: i32, y: i32) {
    e.color = RED;
    e.begin(Tool::Pencil, x, y).expect("begin");
    e.commit();
}

/// A red pixel at (4, 4) lifted and moved right by `dx`, with a step undone behind it.
fn moved(mode: ColorMode, dx: i32) -> Editor {
    let mut e = Editor::new(Document::new(16, 16, mode).expect("valid"));
    e.pixel_perfect = false;
    dot(&mut e, 9, 9);
    e.undo();
    dot(&mut e, 4, 4);
    e.select(Mask::rect(4, 4, 1, 1), SelectOp::Replace);
    e.apply(FloatingCmd::Lift { copy: false }).expect("lift");
    e.apply(FloatingCmd::Move {
        dx,
        dy: 0,
        join: true,
    })
    .expect("move");
    e
}

fn at(doc: &Document, x: i32, y: i32) -> u32 {
    doc.layers()[0].cel(0).get(x, y)
}

/// What export must not change: revision, undo and redo, document, selection, floating stamp.
type Held = (
    u64,
    usize,
    usize,
    Document,
    Option<(i32, i32, i32, i32)>,
    u64,
);

fn held(e: &Editor) -> Held {
    (
        e.revision(),
        e.history().undo_depth(),
        e.history().redo_depth(),
        e.doc().clone(),
        e.selection().map(|s| s.bounds()),
        e.floating_stamp(),
    )
}

#[test]
fn without_floating_pixels_the_document_itself_is_shown() {
    let mut e = Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("valid"));
    dot(&mut e, 1, 1);
    assert!(matches!(e.shown_document(), Cow::Borrowed(_)));
}

#[test]
fn floating_pixels_are_shown_where_they_are_and_nothing_else_changes() {
    for mode in [ColorMode::Rgba, ColorMode::Palette] {
        let mut e = moved(mode, 3);
        let before = held(&e);
        let shown = e.shown_document().into_owned();
        let after = held(&e);
        assert_eq!(after.0, before.0, "{mode:?} revision");
        assert_eq!(
            after.1..=after.2,
            before.1..=before.2,
            "{mode:?} undo and redo"
        );
        assert_eq!(after.3, before.3, "{mode:?} the document");
        assert_eq!(
            (after.4, after.5),
            (before.4, before.5),
            "{mode:?} selection"
        );
        assert!(e.is_floating());
        assert_eq!(at(&shown, 4, 4), 0, "{mode:?} lifted from (4, 4)");
        assert_ne!(at(&shown, 7, 4), 0, "{mode:?} shown at (7, 4)");
        assert_eq!(
            at(e.doc(), 7, 4),
            0,
            "{mode:?} the editor's document has not got it"
        );
        e.apply(FloatingCmd::Anchor).expect("anchor");
        assert_eq!(
            e.doc().layers(),
            shown.layers(),
            "{mode:?} as anchoring would make it"
        );
    }
}

#[test]
fn floating_pixels_off_the_canvas_are_clipped() {
    let mut e = moved(ColorMode::Rgba, 40);
    let shown = e.shown_document().into_owned();
    assert!((0..16).all(|x| (0..16).all(|y| at(&shown, x, y) == 0)));
}

#[test]
fn a_held_drawing_shows_floating_pixels_in_every_frame_of_its_hold() {
    let mut e = moved(ColorMode::Rgba, 2);
    e.apply(FloatingCmd::Anchor).expect("anchor");
    e.apply(FrameCmd::Add { at: 1, copy: false })
        .expect("frame");
    let layer = e.layer_id(0).expect("layer");
    e.apply(FrameCmd::HoldPrevious { layer, frame: 1 })
        .expect("held");
    e.frame = 0;
    e.select(Mask::rect(6, 4, 1, 1), SelectOp::Replace);
    e.apply(FloatingCmd::Lift { copy: false }).expect("lift");
    e.apply(FloatingCmd::Move {
        dx: 0,
        dy: 5,
        join: true,
    })
    .expect("move");
    let shown = e.shown_document().into_owned();
    for frame in 0..2 {
        let cel = shown.layers()[0].cel(frame);
        assert_eq!((cel.get(6, 4), cel.get(6, 9)), (0, RED), "frame {frame}");
    }
    assert_eq!(
        e.doc().layers()[0].cel(1).get(6, 4),
        RED,
        "the editor's is as it was"
    );
}

#[test]
fn floating_pixels_are_shown_on_the_layer_they_were_lifted_from() {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("valid"));
    e.pixel_perfect = false;
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("layer");
    assert_eq!(e.layer, 1);
    dot(&mut e, 4, 4);
    e.select(Mask::rect(4, 4, 1, 1), SelectOp::Replace);
    e.apply(FloatingCmd::Lift { copy: false }).expect("lift");
    e.apply(FloatingCmd::Move {
        dx: 2,
        dy: 0,
        join: true,
    })
    .expect("move");
    let shown = e.shown_document().into_owned();
    let cel = |l: usize| shown.layers()[l].cel(0).get(6, 4);
    assert_eq!((cel(0), cel(1)), (0, RED));
}
