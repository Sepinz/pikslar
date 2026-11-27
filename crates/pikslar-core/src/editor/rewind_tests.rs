//! Walking history leaves the editor exactly as it was, positions follow every step, and steps
//! taken back in from outside undo to exactly the states the walk showed.

use super::rewind::{OlderStep, RewindRefusal, StepView};
use crate::color::rgba;
use crate::document::{ColorMode, Document};
use crate::editor::{Editor, Tool};
use crate::history::RewindMarks;
use crate::image::TileKey;
use crate::{FrameCmd, LayerCmd, SelectionCmd, StepCmd};

fn dot(e: &mut Editor, (layer, frame): (usize, usize), x: i32, color: u32) {
    (e.layer, e.frame, e.color) = (layer, frame, color);
    let begin = crate::DrawCmd::Begin {
        tool: Tool::Pencil,
        x,
        y: 2,
    };
    e.apply(begin).expect("begin");
    e.apply(crate::DrawCmd::Commit).expect("commit");
}

/// Strokes on two layers and frames, with structural steps and a selection among them. Also
/// every state it passed, oldest first.
fn busy() -> (Editor, Vec<Document>) {
    let mut e = Editor::new(Document::new(40, 40, ColorMode::Rgba).expect("valid"));
    let mut states = vec![e.doc().clone()];
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    states.push(e.doc().clone());
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    states.push(e.doc().clone());
    for n in 0..10 {
        dot(
            &mut e,
            (n % 2, n / 2 % 2),
            n as i32 * 3,
            rgba(n as u8 * 20, 9, 9, 255),
        );
        states.push(e.doc().clone());
        if n == 4 {
            e.apply(SelectionCmd::All).expect("select");
            states.push(e.doc().clone());
        }
    }
    (e, states)
}

/// What can be seen of an editor from outside.
fn seen(
    e: &Editor,
) -> (
    Document,
    (usize, usize),
    u64,
    usize,
    (usize, usize),
    RewindMarks,
) {
    let h = e.history();
    (
        e.doc().clone(),
        (e.layer, e.frame),
        e.revision(),
        h.memory_used(),
        (h.undo_depth(), h.redo_depth()),
        e.rewind_marks(),
    )
}

/// The walk's views, kept as steps to take back in.
fn walk(e: &mut Editor, to: u64) -> Vec<(u64, OlderStep)> {
    let mut steps = Vec::new();
    e.walk_back(to, &mut |at, view| {
        let step = match view {
            StepView::Unchanged => OlderStep::Unchanged,
            StepView::Pixels {
                layer,
                frame,
                before,
                shares,
            } => OlderStep::Pixels {
                layer,
                frame,
                before: before.to_vec(),
                shares,
            },
            StepView::Whole(doc) => OlderStep::Whole(Box::new(doc.clone())),
        };
        steps.push((at, step));
    })
    .expect("walks");
    steps
}

#[test]
fn a_walk_shows_every_step_newest_first_and_leaves_everything_as_it_was() {
    let (mut e, _) = busy();
    let before = seen(&e);
    // As the program does: each step is read as it passes, and nothing of it is kept.
    let mut passed = Vec::new();
    e.walk_back(0, &mut |at, view| {
        let kind = match view {
            StepView::Unchanged => 0,
            StepView::Pixels { .. } => 1,
            StepView::Whole(_) => 2,
        };
        passed.push((at, kind));
    })
    .expect("walks");
    assert_eq!(seen(&e), before, "charges and room included");
    let at: Vec<u64> = passed.iter().map(|(at, _)| *at).collect();
    let depth = e.history().undo_depth() as u64;
    assert_eq!(at, (1..=depth).rev().collect::<Vec<_>>());
    let kinds = |want: u8| passed.iter().filter(|(_, k)| *k == want).count();
    assert_eq!((kinds(0), kinds(1), kinds(2)), (1, 10, 2));
}

#[test]
fn positions_follow_new_steps_undo_and_trimming() {
    let (mut e, _) = busy();
    let top = e.history().undo_depth() as u64;
    let m = e.rewind_marks();
    assert_eq!((m.below, m.depth), (0, top));
    e.clear_rewind_rewrites();
    e.apply(StepCmd::Undo).expect("undo");
    e.apply(StepCmd::Undo).expect("undo");
    let m = e.rewind_marks();
    assert_eq!((m.depth, m.unchanged_up_to), (top - 2, u64::MAX - 1));
    dot(&mut e, (0, 0), 30, rgba(1, 2, 3, 255));
    let m = e.rewind_marks();
    assert_eq!(
        (m.depth, m.unchanged_up_to),
        (top - 1, top - 2),
        "the new step replaced one"
    );
    e.rebase_rewind(100);
    let m = e.rewind_marks();
    assert_eq!(
        (m.below, m.depth, m.unchanged_up_to),
        (100, top + 99, top + 98)
    );
    let mut small =
        Editor::with_undo_budget(Document::new(40, 40, ColorMode::Rgba).expect("ok"), 1);
    dot(&mut small, (0, 0), 1, rgba(1, 2, 3, 255));
    dot(&mut small, (0, 0), 5, rgba(1, 2, 3, 255));
    let m = small.rewind_marks();
    assert_eq!(
        (m.below, m.depth),
        (1, 2),
        "the oldest step was dropped for the budget"
    );
}

#[test]
fn steps_taken_back_in_undo_to_every_state_the_walk_passed_and_redo_back() {
    let (mut e, states) = busy();
    let depth = e.history().undo_depth() as u64;
    let steps = walk(&mut e, 0);
    // A new session on the saved document, which had `depth` steps before it.
    let mut reopened = Editor::new(e.doc().clone());
    reopened.rebase_rewind(depth);
    for (at, step) in steps {
        assert_eq!(reopened.rewind_marks().below, at);
        reopened.push_older(step).expect("taken");
        reopened.apply(StepCmd::Undo).expect("undo");
        assert_eq!(*reopened.doc(), states[at as usize - 1], "state {}", at - 1);
    }
    for at in 1..=depth {
        reopened.apply(StepCmd::Redo).expect("redo");
        assert_eq!(*reopened.doc(), states[at as usize], "redone to {at}");
    }
    assert_eq!(reopened.history().redo_depth(), 0);
}

#[test]
fn a_step_is_taken_only_at_the_bottom_and_only_if_it_fits() {
    let (mut e, _) = busy();
    let unchanged = || OlderStep::Unchanged;
    assert_eq!(e.push_older(unchanged()), Err(RewindRefusal::NotAtBottom));
    let mut fresh = Editor::new(Document::new(40, 40, ColorMode::Rgba).expect("ok"));
    assert_eq!(
        fresh.push_older(unchanged()),
        Err(RewindRefusal::NotAtBottom)
    );
    fresh.rebase_rewind(3);
    let far = OlderStep::Pixels {
        layer: 0,
        frame: 0,
        before: vec![(TileKey { x: 900, y: 0 }, None)],
        shares: None,
    };
    assert_eq!(fresh.push_older(far), Err(RewindRefusal::DoesNotFit));
    let twice = OlderStep::Pixels {
        layer: 0,
        frame: 0,
        before: vec![(TileKey { x: 0, y: 0 }, None); 2],
        shares: None,
    };
    assert_eq!(fresh.push_older(twice), Err(RewindRefusal::DoesNotFit));
    let no_layer = OlderStep::Pixels {
        layer: 5,
        frame: 0,
        before: vec![(TileKey { x: 0, y: 0 }, None)],
        shares: None,
    };
    assert_eq!(fresh.push_older(no_layer), Err(RewindRefusal::DoesNotFit));
    assert_eq!(fresh.history().undo_depth(), 0, "nothing was taken");
    fresh.push_older(unchanged()).expect("an empty step fits");
    assert_eq!(fresh.rewind_marks().below, 2);
}

#[test]
fn a_stored_whole_step_leaves_how_layers_are_shown_as_they_are_now() {
    let (mut e, _) = busy();
    let steps = walk(&mut e, 0);
    let mut reopened = Editor::new(e.doc().clone());
    reopened.rebase_rewind(e.history().undo_depth() as u64);
    let layer = reopened.layer_id(0).expect("layer");
    reopened
        .apply(LayerCmd::SetVisible {
            layer,
            visible: false,
        })
        .expect("hidden");
    for (_, step) in steps {
        reopened.push_older(step).expect("taken");
        reopened.apply(StepCmd::Undo).expect("undo");
        assert!(!reopened.doc().layers()[0].visible, "still hidden");
    }
}

#[test]
fn positions_never_overflow_however_far_a_document_is_rebased() {
    let (mut e, _) = busy();
    e.rebase_rewind(u64::MAX);
    dot(&mut e, (0, 0), 7, rgba(1, 2, 3, 255));
    e.apply(StepCmd::Undo).expect("undo");
    dot(&mut e, (0, 0), 8, rgba(1, 2, 3, 255));
    let m = e.rewind_marks();
    assert!(m.below <= m.depth && m.unchanged_up_to <= m.depth);
    let mut seen = 0;
    e.walk_back(m.depth, &mut |_, _| seen += 1)
        .expect("walks nothing");
    assert_eq!(seen, 0);
}

#[test]
fn the_tile_room_holds_every_key_a_stored_step_may_name_and_no_more() {
    let mut fresh = Editor::new(Document::new(40, 40, ColorMode::Rgba).expect("ok"));
    let room = fresh.rewind_tile_room();
    assert_eq!(room, 196);
    fresh.rebase_rewind(1);
    let keys = |xs: std::ops::RangeInclusive<i16>| {
        let tiles = xs
            .clone()
            .flat_map(|y| xs.clone().map(move |x| (TileKey { x, y }, None)));
        tiles.collect::<Vec<_>>()
    };
    let widest = OlderStep::Pixels {
        layer: 0,
        frame: 0,
        before: keys(-5..=7),
        shares: None,
    };
    assert!(keys(-5..=7).len() <= room);
    fresh
        .push_older(widest)
        .expect("as far as a step may reach");
}

#[test]
fn a_stored_canvas_resize_keeps_the_selection_and_undo_brings_back_none() {
    let mut e = Editor::new(Document::new(40, 40, ColorMode::Rgba).expect("valid"));
    let resize = crate::command::CanvasCmd::Resize {
        width: 60,
        height: 50,
        dx: 3,
        dy: 4,
    };
    e.apply(resize).expect("resized");
    let mut steps = walk(&mut e, 0).into_iter().map(|(_, step)| step);
    let mut reopened = Editor::new(e.doc().clone());
    reopened.rebase_rewind(1);
    // A selection of the canvas as it is now, left from steps no longer in memory.
    reopened.set_selection(Some(crate::mask::Mask::rect(0, 0, 60, 50)));
    let kept = reopened.selection().cloned().expect("selected");
    let resize = steps.next().expect("the resize");
    reopened.push_older(resize).expect("taken");
    let now = reopened.selection().expect("the selection is kept");
    assert!(std::sync::Arc::ptr_eq(now, &kept));
    reopened.apply(StepCmd::Undo).expect("undo");
    assert_eq!((reopened.doc().width, reopened.doc().height), (40, 40));
    assert!(reopened.selection().is_none(), "no mask of another canvas");
    reopened.apply(StepCmd::Redo).expect("redo");
    let back = reopened.selection().expect("back");
    assert!(std::sync::Arc::ptr_eq(back, &kept));
}

#[test]
fn a_stored_stroke_that_split_a_shared_drawing_shares_it_again_when_undone() {
    let mut e = Editor::new(Document::new(40, 40, ColorMode::Rgba).expect("valid"));
    dot(&mut e, (0, 0), 1, rgba(9, 9, 9, 255));
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
    dot(&mut e, (0, 1), 5, rgba(200, 9, 9, 255));
    let steps = walk(&mut e, 2);
    let mut reopened = Editor::new(e.doc().clone());
    reopened.rebase_rewind(3);
    let (_, stroke) = steps.into_iter().next().expect("the stroke");
    reopened.push_older(stroke).expect("taken");
    reopened.apply(StepCmd::Undo).expect("undo");
    let layer = &reopened.doc().layers()[0];
    let (first, second) = (layer.drawing(0), layer.drawing(1));
    let same = first
        .zip(second)
        .is_some_and(|(a, b)| std::sync::Arc::ptr_eq(a, b));
    assert!(same, "the two frames show one drawing again, as in memory");
}

/// Review finding M1 (RC3): undo that takes steps back from disk never trims redo, so it
/// stops once memory is over the budget, at most one step past it. Refused, nothing changes;
/// redo still reaches the newest state exactly.
#[test]
fn steps_are_taken_back_only_while_memory_is_within_the_budget() {
    let (mut e, states) = busy();
    let depth = e.history().undo_depth() as u64;
    let steps = walk(&mut e, 0);
    let mut reopened = Editor::with_undo_budget(e.doc().clone(), 1);
    reopened.rebase_rewind(depth);
    let mut taken = 0;
    for (_, step) in steps {
        let before = seen(&reopened);
        match reopened.push_older(step) {
            Ok(()) => {
                taken += 1;
                reopened.apply(StepCmd::Undo).expect("undo");
            }
            Err(refused) => {
                assert_eq!(refused, RewindRefusal::Full);
                assert_eq!(seen(&reopened), before, "a refusal changes nothing");
                break;
            }
        }
    }
    assert_eq!(taken, 1, "one step, then the budget is spent");
    assert!(reopened.history().is_over_budget());
    for _ in 0..taken {
        reopened.apply(StepCmd::Redo).expect("redo");
    }
    assert_eq!(
        *reopened.doc(),
        states[depth as usize],
        "back to the newest work"
    );
}
