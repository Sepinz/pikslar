//! The position tween as an artist runs it: one step over one hold, every refusal changing
//! nothing, and holds whose frames then sit apart staying correct under the other commands.

use super::{CelCmd, DrawCmd, FrameCmd, LayerCmd, StepCmd};
use crate::color::rgba;
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::{Editor, Tool};
use crate::layer::{Cel, Turn};
use crate::offset_tests::{image_of, world};
use crate::tween::Ease;
use std::collections::HashMap;

const RED: u32 = rgba(200, 10, 10, 255);
const BLUE: u32 = rgba(10, 10, 200, 255);

/// A 32x32 editor of `frames` frames of 100 ms on two layers. On the bottom layer one drawing
/// is held across every frame; the last frame alone is moved to `last`.
fn held(frames: usize, last: (i32, i32)) -> Editor {
    let mut e = Editor::new(Document::new(32, 32, ColorMode::Rgba).expect("valid"));
    let drawing = image_of(&HashMap::from([((3, 4), RED), ((-2, 30), RED)]));
    e.doc.layers[0]
        .put_cel(0, Cel::from_image(drawing))
        .expect("frame");
    for at in 1..frames {
        e.apply(FrameCmd::Add { at, copy: false }).expect("frame");
    }
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("layer");
    let layer = e.layer_id(0).expect("layer");
    e.apply(FrameCmd::HoldAcross {
        layer,
        source: 0,
        from: 0,
        to: frames - 1,
    })
    .expect("hold");
    place(&mut e, frames - 1, last);
    e
}

/// Puts one frame's cel at a place, keeping its hold: how the artist sets the last key.
fn place(e: &mut Editor, frame: usize, (x, y): (i32, i32)) {
    let layer = &mut e.doc.layers[0];
    let moved = layer.cel(frame).moved_to(x, y).expect("in range");
    layer.put_cel(frame, moved).expect("frame");
}

fn tween(e: &mut Editor, from: usize, to: usize, ease: Ease) -> Result<bool, LimitError> {
    let layer = e.layer_id(0).expect("layer");
    let command = CelCmd::Tween {
        layer,
        from,
        to,
        ease,
    };
    e.apply(command).map(|done| done.changed)
}

fn places(e: &Editor) -> Vec<(i32, i32)> {
    e.doc.layers[0]
        .cels()
        .iter()
        .map(|cel| (i32::from(cel.position().0), i32::from(cel.position().1)))
        .collect()
}

fn xs(e: &Editor) -> Vec<i32> {
    places(e).into_iter().map(|(x, _)| x).collect()
}

/// Everything about the bottom layer's cels: place, hold and which drawing they show.
fn cels(e: &Editor) -> Vec<((i16, i16), u32, usize)> {
    e.doc.layers[0]
        .cels()
        .iter()
        .map(|cel| (cel.position(), cel.link(), cel.image_id()))
        .collect()
}

#[test]
fn the_tables_of_the_plan_come_out_of_the_command() {
    let cases: [(i32, Ease, [i32; 5]); 6] = [
        (4, Ease::Linear, [0, 1, 2, 3, 4]),
        (3, Ease::Linear, [0, 1, 2, 2, 3]),
        (-3, Ease::Linear, [0, -1, -2, -2, -3]),
        (8, Ease::In, [0, 1, 2, 5, 8]),
        (8, Ease::Out, [0, 4, 6, 8, 8]),
        (8, Ease::InOut, [0, 1, 4, 7, 8]),
    ];
    for (to, ease, want) in cases {
        let mut e = held(5, (to, 0));
        assert_eq!(tween(&mut e, 0, 4, ease), Ok(true));
        assert_eq!(xs(&e), want, "0 to {to}, {ease:?}");
        assert!(places(&e).iter().all(|&(_, y)| y == 0));
    }
}

#[test]
fn both_axes_and_uneven_durations_follow_the_plan() {
    let mut e = held(5, (4, -2));
    tween(&mut e, 0, 4, Ease::Linear).expect("tween");
    assert_eq!(places(&e), [(0, 0), (1, -1), (2, -1), (3, -2), (4, -2)]);

    let mut e = held(4, (10, 0));
    let durations = vec![100, 300, 100, 100];
    e.apply(FrameCmd::Retime { from: 0, durations })
        .expect("retime");
    tween(&mut e, 0, 3, Ease::Linear).expect("tween");
    assert_eq!(xs(&e), [0, 2, 8, 10]);
}

#[test]
fn a_document_timed_by_frame_rate_eases_over_ticks() {
    // 42 and 83 ms are one and two ticks at 24 a second, but not one to two in milliseconds.
    let run = |fps: Option<u8>| {
        let mut e = held(4, (10, 0));
        let durations = vec![42, 83, 42, 42];
        e.apply(FrameCmd::Retime { from: 0, durations })
            .expect("retime");
        if fps.is_some() {
            e.apply(FrameCmd::SetRate { fps }).expect("rate");
            assert_eq!((0..4).map(|f| e.doc.frame_units(f)).sum::<u32>(), 5);
        }
        tween(&mut e, 0, 3, Ease::Linear).expect("tween");
        xs(&e)
    };
    assert_eq!(run(None), [0, 3, 7, 10]);
    assert_eq!(run(Some(24)), [0, 3, 8, 10]);
}

#[test]
fn only_the_frames_between_the_keys_change_and_they_keep_the_drawing() {
    let mut e = held(7, (0, 0));
    place(&mut e, 4, (6, 3));
    let before = cels(&e);
    let top = e.doc.layers[1].cels().to_vec();
    let durations = e.doc.frames.clone();
    assert_eq!(tween(&mut e, 1, 4, Ease::Linear), Ok(true));
    let after = cels(&e);
    assert_eq!(after[0], before[0], "before the run");
    assert_eq!(&after[4..], &before[4..], "the last key and after it");
    assert_eq!(after[1], before[1], "the first key");
    assert_eq!(places(&e)[1..5], [(0, 0), (2, 1), (4, 2), (6, 3)]);
    for (a, b) in after.iter().zip(&before) {
        assert_eq!((a.1, a.2), (b.1, b.2), "the hold and its one drawing stay");
    }
    assert_eq!(
        e.doc.layers[1].cels(),
        &top[..],
        "another layer is left alone"
    );
    assert_eq!(e.doc.frames, durations, "no duration changes");
}

#[test]
fn undo_and_redo_give_back_every_offset_in_one_step() {
    let mut e = held(6, (-9, 5));
    let before = cels(&e);
    let depth = e.history().undo_depth();
    tween(&mut e, 0, 5, Ease::InOut).expect("tween");
    let after = cels(&e);
    assert_ne!(after, before);
    assert_eq!(e.history().undo_depth(), depth + 1, "one step");
    e.apply(StepCmd::Undo).expect("undo");
    assert_eq!(cels(&e), before);
    e.apply(StepCmd::Redo).expect("redo");
    assert_eq!(cels(&e), after);
}

#[test]
fn a_tween_that_changes_nothing_makes_no_step() {
    let mut e = held(5, (4, 0));
    tween(&mut e, 0, 4, Ease::Linear).expect("tween");
    let depth = e.history().undo_depth();
    assert_eq!(tween(&mut e, 0, 4, Ease::Linear), Ok(false));
    assert_eq!(e.history().undo_depth(), depth);
}

#[test]
fn equal_keys_line_the_frames_up_again() {
    let mut e = held(5, (8, 0));
    tween(&mut e, 0, 4, Ease::Out).expect("tween");
    place(&mut e, 4, (0, 0));
    tween(&mut e, 0, 4, Ease::In).expect("tween");
    assert_eq!(places(&e), [(0, 0); 5]);
}

fn refused(e: &mut Editor, from: usize, to: usize, why: LimitError) {
    let (before, depth) = (cels(e), e.history().undo_depth());
    assert_eq!(tween(e, from, to, Ease::Linear), Err(why), "{from} to {to}");
    assert_eq!(cels(e), before, "a refusal changes nothing");
    assert_eq!(e.history().undo_depth(), depth, "and makes no step");
}

#[test]
fn a_run_shorter_than_three_frames_or_past_the_end_is_refused() {
    let mut e = held(5, (4, 0));
    refused(&mut e, 0, 1, LimitError::TweenTooShort);
    refused(&mut e, 2, 2, LimitError::TweenTooShort);
    refused(&mut e, 3, 1, LimitError::TweenTooShort);
    refused(&mut e, 2, 5, LimitError::TweenTooShort);
    refused(&mut e, 0, usize::MAX, LimitError::TweenTooShort);
    assert_eq!(
        LimitError::TweenTooShort.to_string(),
        "Pick at least three frames"
    );
}

#[test]
fn a_run_not_in_one_hold_is_refused() {
    let mut e = held(5, (4, 0));
    let layer = e.layer_id(0).expect("layer");
    e.apply(FrameCmd::Release {
        layer,
        from: 4,
        to: 4,
    })
    .expect("release");
    refused(&mut e, 0, 4, LimitError::TweenUnheld);
    // The top layer holds nothing at all.
    let top = e.layer_id(1).expect("layer");
    let command = CelCmd::Tween {
        layer: top,
        from: 0,
        to: 4,
        ease: Ease::Linear,
    };
    assert_eq!(e.apply(command), Err(LimitError::TweenUnheld));
    assert_eq!(
        LimitError::TweenUnheld.to_string(),
        "Hold one drawing across these frames first"
    );
}

#[test]
fn a_locked_layer_is_refused() {
    let mut e = held(5, (4, 0));
    let layer = e.layer_id(0).expect("layer");
    e.apply(LayerCmd::SetLocked {
        layer,
        locked: true,
    })
    .expect("lock");
    refused(&mut e, 0, 4, LimitError::Locked);
}

#[test]
fn moving_a_tweened_hold_moves_every_frame_by_the_same_amount() {
    let mut e = held(5, (4, 0));
    tween(&mut e, 0, 4, Ease::Linear).expect("tween");
    let layer = e.layer_id(0).expect("layer");
    let command = CelCmd::Move {
        layer,
        frame: 2,
        dx: 3,
        dy: -1,
    };
    e.apply(command).expect("move");
    e.apply(StepCmd::EndDrag).expect("end");
    assert_eq!(places(&e), [(3, -1), (4, -1), (5, -1), (6, -1), (7, -1)]);
}

#[test]
fn turning_a_tweened_hold_is_refused_and_changes_nothing() {
    let mut e = held(5, (4, 0));
    tween(&mut e, 0, 4, Ease::Linear).expect("tween");
    let before = cels(&e);
    let layer = e.layer_id(0).expect("layer");
    let command = CelCmd::Turn {
        layer,
        frame: 1,
        turn: Turn::Clockwise,
    };
    assert_eq!(e.apply(command), Err(LimitError::UnequalHold));
    assert_eq!(cels(&e), before);
}

#[test]
fn drawing_on_a_tweened_frame_shows_in_every_frame_at_its_own_place() {
    let mut e = held(5, (4, 0));
    tween(&mut e, 0, 4, Ease::Linear).expect("tween");
    (e.layer, e.frame) = (0, 2);
    e.color = BLUE;
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 10,
        y: 10,
    })
    .expect("begin");
    e.apply(DrawCmd::Commit).expect("commit");
    let layer = &e.doc.layers[0];
    for frame in 0..5 {
        let cel = layer.cel(frame);
        assert!(cel.same_image(layer.cel(2)), "one drawing");
        // Frame 2 sits at x 2, so the dot is at 8 in the drawing and shows at 8 + x.
        let x = i32::from(cel.position().0);
        assert_eq!(world(cel).get(&(8 + x, 10)), Some(&BLUE), "frame {frame}");
    }
}

#[test]
fn merging_a_tweened_hold_keeps_every_frame_where_it_was() {
    let mut e = held(5, (4, 2));
    tween(&mut e, 0, 4, Ease::Linear).expect("tween");
    let before: Vec<_> = e.doc.layers[0].cels().iter().map(world).collect();
    let top = e.layer_id(1).expect("layer");
    e.apply(LayerCmd::MergeDown { layer: top }).expect("merge");
    let after: Vec<_> = e.doc.layers[0].cels().iter().map(world).collect();
    assert_eq!(
        after, before,
        "every pixel of every frame stays where it was shown"
    );
    e.apply(StepCmd::Undo).expect("undo");
    let undone: Vec<_> = e.doc.layers[0].cels().iter().map(world).collect();
    assert_eq!(undone, before);
}
