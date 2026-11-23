//! Writing through a link: every member on every layer shows each change, at its own offset.

use crate::cel_drag::{CelDrag, DragKind, Landing};
use crate::color::rgba;
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::{Editor, Tool};
use crate::mask::Mask;
use crate::selection::SelectOp;
use crate::{FloatingCmd, FrameCmd};

const INK: u32 = rgba(10, 200, 30, 255);

/// Two layers of three frames. The first layer's first cel is linked to the second layer's
/// second cel, which sits two pixels to the right.
fn linked() -> Editor {
    let mut doc = Document::new(16, 16, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("room");
    let mut e = Editor::new(doc);
    for at in 1..3 {
        e.transact(|d| d.add_frame(at, false)).expect("room");
    }
    e.pixel_perfect = false;
    e.color = INK;
    e.transact(|d| {
        link_cel(d, (0, 0), (1, 1))?;
        d.layers[1].move_cels(&[1], 2, 0);
        Ok(())
    })
    .expect("cels");
    e
}

/// Links the cel at `to` to the cel at `from`, as an Alt drag of one cel does.
fn link_cel(d: &mut Document, from: (usize, usize), to: (usize, usize)) -> Result<(), LimitError> {
    d.drag_cels(&CelDrag {
        layers: (from.0, from.0),
        frames: (from.1, from.1),
        to_layer: to.0,
        to_frame: to.1,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    })
}

fn dot(e: &mut Editor, layer: usize, frame: usize, x: i32) {
    (e.layer, e.frame) = (layer, frame);
    e.begin(Tool::Pencil, x, 3).expect("begin");
    e.commit();
}

fn ink(doc: &Document, layer: usize, frame: usize, x: i32) -> bool {
    doc.layers[layer].cel(frame).get(x, 3) != 0
}

#[test]
fn a_stroke_on_one_member_shows_on_every_layer_at_its_own_offset() {
    let mut e = linked();
    dot(&mut e, 0, 0, 5);
    assert!(ink(&e.doc, 0, 0, 5));
    assert!(ink(&e.doc, 1, 1, 7), "two pixels to the right");
    assert!(!ink(&e.doc, 1, 1, 5) && !ink(&e.doc, 1, 0, 5) && !ink(&e.doc, 0, 1, 5));
    assert!(e.doc.layers[1].cel(1).same_image(e.doc.layers[0].cel(0)));
    let id = e.doc.layers[0].cel(0).image_id();
    dot(&mut e, 1, 1, 9);
    assert!(ink(&e.doc, 0, 0, 7), "and back the other way");
    assert_eq!(e.doc.layers[0].cel(0).image_id(), id, "edited in place");
    assert_eq!(e.doc.check_links(), Ok(()));
}

#[test]
fn undo_and_redo_reach_every_member() {
    let mut e = linked();
    let before = e.doc.clone();
    dot(&mut e, 0, 0, 5);
    let after = e.doc.clone();
    for _ in 0..2 {
        assert!(e.undo());
        assert_eq!(e.doc, before);
        assert!(!ink(&e.doc, 1, 1, 7));
        assert!(e.redo());
        assert_eq!(e.doc, after);
        assert!(ink(&e.doc, 1, 1, 7));
        assert!(e.doc.layers[1].cel(1).same_image(e.doc.layers[0].cel(0)));
    }
}

#[test]
fn undo_stays_on_a_member_of_another_layer() {
    let mut e = linked();
    dot(&mut e, 0, 0, 5);
    (e.layer, e.frame) = (1, 1);
    assert!(e.undo());
    assert_eq!(
        (e.layer, e.frame),
        (1, 1),
        "this cel already shows the change"
    );
}

/// Floating pixels lifted from the first layer's linked cel and moved down.
fn lifted(e: &mut Editor) {
    dot(e, 0, 0, 5);
    (e.layer, e.frame) = (0, 0);
    e.select(Mask::rect(5, 3, 1, 1), SelectOp::Replace);
    e.apply(FloatingCmd::Lift { copy: false }).expect("lift");
    e.apply(FloatingCmd::Move {
        dx: 0,
        dy: 4,
        join: true,
    })
    .expect("move");
}

#[test]
fn floating_pixels_put_down_inside_a_structural_step_reach_every_member() {
    let mut e = linked();
    let (start, depth) = (e.doc.clone(), e.history().undo_depth());
    lifted(&mut e);
    let shown = e.shown_document().into_owned();
    assert_ne!(
        shown.layers[1].cel(1).get(7, 7),
        0,
        "shown on the other layer too"
    );
    assert_eq!(
        e.doc.layers[1].cel(1).get(7, 7),
        0,
        "the editor's is as it was"
    );
    e.apply(FrameCmd::Add { at: 3, copy: false }).expect("room");
    let after = e.doc.clone();
    assert_ne!(after.layers[1].cel(1).get(7, 7), 0);
    assert_eq!(after.layers[1].cel(1).get(7, 3), 0);
    assert!(after.layers[1].cel(1).same_image(after.layers[0].cel(0)));
    assert_eq!(after.check_links(), Ok(()));
    e.scrub_to(depth);
    assert_eq!(e.doc, start);
    while e.redo() {}
    assert_eq!(e.doc, after);
}
