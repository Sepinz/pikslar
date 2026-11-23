//! Scrubbing through history leaves the editor exactly as undoing or redoing one step at a time
//! does: the document, the place, the counters the program watches, the damage and the charge.

use crate::color::rgba;
use crate::document::{ColorMode, Document};
use crate::editor::{Damage, Editor, Tool};
use crate::mask::Mask;
use crate::{FrameCmd, LayerCmd, SelectionCmd, StepCmd};

/// Everything a scrub can change that someone outside the editor can see.
#[derive(Debug, PartialEq)]
struct Seen {
    doc: Document,
    place: (usize, usize),
    revision: u64,
    selection_version: u64,
    selection: Option<Mask>,
    damage: Damage,
    memory: usize,
    depths: (usize, usize),
}

fn seen(e: &mut Editor) -> Seen {
    Seen {
        doc: e.doc.clone(),
        place: (e.layer, e.frame),
        revision: e.revision(),
        selection_version: e.selection_version(),
        selection: e.selection().map(|m| (**m).clone()),
        damage: e.take_damage(),
        memory: e.history().memory_used(),
        depths: (e.history().undo_depth(), e.history().redo_depth()),
    }
}

fn dot(e: &mut Editor, (layer, frame): (usize, usize), x: i32, color: u32) {
    e.layer = layer;
    e.frame = frame;
    e.color = color;
    e.apply(crate::DrawCmd::Begin {
        tool: Tool::Pencil,
        x,
        y: 2,
    })
    .expect("begin");
    e.apply(crate::DrawCmd::Commit).expect("commit");
}

/// Strokes on several layers and frames, with structural steps and a selection among them.
fn busy_editor() -> Editor {
    let mut e = Editor::new(Document::new(40, 40, ColorMode::Rgba).expect("valid"));
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    for n in 0..12 {
        dot(
            &mut e,
            (n % 2, n / 2 % 2),
            n as i32 * 3,
            rgba(n as u8 * 20, 9, 9, 255),
        );
        if n == 5 {
            e.apply(SelectionCmd::All).expect("select");
        }
        if n == 8 {
            e.apply(FrameCmd::Add { at: 2, copy: true }).expect("room");
        }
    }
    e.take_damage();
    e
}

/// Undoes or redoes one step at a time until `depth` steps remain undoable.
fn step_to(e: &mut Editor, depth: usize) {
    while e.history().undo_depth() > depth {
        assert!(e.apply(StepCmd::Undo).expect("undo").changed);
    }
    while e.history().undo_depth() < depth {
        assert!(e.apply(StepCmd::Redo).expect("redo").changed);
    }
}

fn scrub_matches_stepping(make: impl Fn() -> Editor, depths: &[usize]) {
    let (mut scrubbed, mut stepped) = (make(), make());
    for &depth in depths {
        scrubbed.apply(StepCmd::ScrubTo { depth }).expect("scrub");
        step_to(&mut stepped, depth);
        assert_eq!(seen(&mut scrubbed), seen(&mut stepped), "at depth {depth}");
    }
}

#[test]
fn scrubbing_leaves_everything_as_stepping_does() {
    let top = busy_editor().history().undo_depth();
    scrub_matches_stepping(busy_editor, &[top - 3, 0, top, 4, 9, 1, top]);
}

#[test]
fn a_scrub_from_another_place_moves_to_the_steps_as_stepping_does() {
    let make = || {
        let mut e = busy_editor();
        e.layer = 1;
        e.frame = 2;
        e
    };
    let top = make().history().undo_depth();
    scrub_matches_stepping(make, &[top - 1, top - 6, 0, top]);
}

#[test]
fn a_scrub_ends_a_stroke_in_progress_as_stepping_does() {
    let make = || {
        let mut e = busy_editor();
        e.apply(crate::DrawCmd::Begin {
            tool: Tool::Pencil,
            x: 30,
            y: 30,
        })
        .expect("begin");
        e
    };
    let top = make().history().undo_depth();
    scrub_matches_stepping(make, &[top - 4, top]);
}

#[test]
fn a_scrub_of_a_value_in_progress_ends_it_as_stepping_does() {
    let make = || {
        let mut e = busy_editor();
        let layer = e.layer_id(0).expect("layer");
        for opacity in [200, 150, 90] {
            e.apply(LayerCmd::SetOpacity { layer, opacity })
                .expect("opacity");
        }
        e
    };
    let top = make().history().undo_depth();
    scrub_matches_stepping(make, &[top - 2, 0, top]);
}

#[test]
fn a_scrub_with_floating_pixels_steps_through_them_as_stepping_does() {
    let make = || {
        let mut e = busy_editor();
        e.layer = 0;
        e.frame = 0;
        e.apply(SelectionCmd::All).expect("select");
        e.apply(crate::FloatingCmd::Lift { copy: false })
            .expect("lift");
        for dx in [2, 3] {
            e.apply(crate::FloatingCmd::Move {
                dx,
                dy: 1,
                join: false,
            })
            .expect("move");
        }
        e
    };
    let top = make().history().undo_depth();
    scrub_matches_stepping(make, &[top.saturating_sub(5), 0, top]);
}
