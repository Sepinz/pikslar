//! A stroke on a cel shared with another layer, as the canvas shows it while drawing and after.
//!
//! The stroke in progress is shown on the cel being drawn on only; the other layers' cels of
//! its link show it once it is committed. This is a known limit, recorded in
//! `docs/links-and-tween.md` section 12; the test pins it so a change is a decision.

use crate::compose::Compositor;
use crate::flatten;
use pikslar_core::{
    rgba, CelCmd, CelDrag, ColorMode, Document, DragKind, DrawCmd, Editor, Landing, Tool,
};

const INK: u32 = rgba(10, 200, 30, 255);

/// Two layers, the first cel of the bottom one linked to the top one, which sits 5 pixels to
/// the right.
fn linked() -> Editor {
    let mut doc = Document::new(16, 4, ColorMode::Rgba).expect("valid");
    doc.add_layer(1, ColorMode::Rgba).expect("room");
    let mut e = Editor::new(doc);
    e.apply(CelCmd::Drag(CelDrag {
        layers: (0, 0),
        frames: (0, 0),
        to_layer: 1,
        to_frame: 0,
        kind: DragKind::Link,
        landing: Landing::Overwrite,
    }))
    .expect("linked");
    let top = e.layer_id(1).expect("layer");
    e.apply(CelCmd::MoveOne {
        layer: top,
        frame: 0,
        dx: 5,
        dy: 0,
    })
    .expect("moved");
    (e.layer, e.frame) = (0, 0);
    e.pixel_perfect = false;
    e.color = INK;
    e
}

#[test]
fn a_stroke_shows_on_other_layers_members_once_committed() {
    let mut e = linked();
    let mut comp = Compositor::new(16, 4);
    let begin = DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 2,
        y: 1,
    };
    e.apply(begin).expect("begin");
    let at = |x: usize| 16 + x;
    let drawing = flatten(&mut e, 0, &mut comp);
    assert_eq!(drawing[at(2)], INK, "the stroke shows where it is drawn");
    assert_eq!(drawing[at(7)], 0, "the member shows it only once committed");
    e.apply(DrawCmd::Commit).expect("commit");
    let done = flatten(&mut e, 0, &mut comp);
    assert_eq!((done[at(2)], done[at(7)]), (INK, INK));
}
