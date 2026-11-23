//! Cases found by reviewing the command core: floating pixels caught between two actions, numbers
//! no pointer could produce, and estimates that must match what a command really needs.

use super::reach::REACH;
use super::*;
use crate::color::{rgba, MAX_COLORS};
use crate::document::Document;
use crate::Tool;
use std::sync::Arc;

fn palette_editor() -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Palette).expect("valid"));
    e.color = rgba(200, 10, 10, 255);
    e.begin(Tool::Pencil, 1, 1).expect("begin");
    e.commit();
    e
}

fn lifted() -> Editor {
    let mut e = palette_editor();
    e.select(Mask::rect(0, 0, 4, 4), SelectOp::Replace);
    e.lift(false);
    e
}

fn lifted_and_moved() -> Editor {
    let mut e = lifted();
    e.move_floating(2, 1);
    e
}

fn fill_color_table(e: &mut Editor) {
    let table = Arc::make_mut(&mut e.doc.colors);
    let mut value = 0x0100_0000;
    while table.len() < MAX_COLORS {
        table.id_for(value).expect("room");
        value += 1;
    }
}

#[test]
fn cancelling_the_name_of_a_new_tag_keeps_the_floating_pixels_applied() {
    let mut e = lifted_and_moved();
    let color = e.color;
    e.apply(TagCmd::Add {
        name: "walk".into(),
        from: 0,
        to: 0,
    })
    .expect("added");
    e.apply(StepCmd::AbortJoin).expect("aborted");
    assert!(e.doc.tags.is_empty());
    assert_eq!(e.color_at(3, 2), color, "the moved pixels are still there");
    assert!(!e.is_floating());
}

#[test]
fn a_tag_that_cannot_be_added_leaves_pixels_floating() {
    let mut e = lifted_and_moved();
    let result = e.apply(TagCmd::Add {
        name: "walk".into(),
        from: 0,
        to: 99,
    });
    assert_eq!(result, Err(LimitError::NoSuchItem));
    assert!(e.is_floating());
}

#[test]
fn a_full_color_table_refuses_a_stroke_before_touching_floating_pixels() {
    let mut e = lifted_and_moved();
    fill_color_table(&mut e);
    e.color = rgba(1, 2, 3, 255);
    let depth = e.history.undo_depth();
    let begin = e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 8,
        y: 8,
    });
    assert!(begin.is_err());
    assert!(e.is_floating(), "the refused stroke applied them");
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn a_full_color_table_refuses_a_paste_before_touching_floating_pixels() {
    let mut e = lifted_and_moved();
    fill_color_table(&mut e);
    let depth = e.history.undo_depth();
    let paste = e.apply(FloatingCmd::Paste {
        clip: std::sync::Arc::new(Clip::from_image(1, 1, vec![rgba(1, 2, 3, 255)])),
        x: 1,
        y: 1,
    });
    assert!(paste.is_err());
    assert!(e.is_floating(), "the refused paste applied them");
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn undoing_a_flip_applied_by_deselecting_brings_the_selection_back() {
    let mut e = lifted();
    e.apply(FloatingCmd::FlipHorizontal).expect("flip");
    e.apply(SelectionCmd::None).expect("deselect");
    assert!(e.selection().is_none());
    assert!(e.undo());
    assert_eq!(e.selection().map(|m| m.bounds()), Some((0, 0, 4, 4)));
}

#[test]
fn undoing_a_flip_applied_by_selecting_brings_the_first_selection_back() {
    let mut e = lifted();
    e.apply(FloatingCmd::FlipHorizontal).expect("flip");
    e.apply(SelectionCmd::Select {
        shape: Box::new(Mask::rect(8, 8, 2, 2)),
        op: SelectOp::Replace,
    })
    .expect("select");
    assert_eq!(e.selection().map(|m| m.bounds()), Some((8, 8, 2, 2)));
    assert!(e.undo());
    assert_eq!(e.selection().map(|m| m.bounds()), Some((0, 0, 4, 4)));
}

#[test]
fn a_failed_change_keeps_pixels_that_were_only_picked_up() {
    let mut e = lifted();
    let (depth, revision) = (e.history.undo_depth(), e.revision);
    let layer = e.layer_id(0).expect("layer");
    assert!(e.apply(LayerCmd::Remove { layer }).is_err());
    assert!(e.is_floating());
    assert_eq!(e.history.undo_depth(), depth);
    assert_eq!(e.revision, revision);
}

#[test]
fn pixels_only_picked_up_make_no_step_when_a_change_succeeds_with_no_effect() {
    let mut e = lifted();
    let depth = e.history.undo_depth();
    e.apply(FrameCmd::Move { from: 0, to: 0 }).expect("fine");
    assert_eq!(e.history.undo_depth(), depth);
}

#[test]
fn a_temporary_pick_up_put_back_untouched_leaves_no_selection() {
    let mut e = palette_editor();
    e.apply(SelectionCmd::Temporary {
        shape: Box::new(Mask::rect(0, 0, 4, 4)),
    })
    .expect("selected");
    e.apply(FloatingCmd::Lift { copy: false }).expect("lifted");
    e.apply(FloatingCmd::Anchor).expect("anchored");
    assert!(e.selection().is_none());
}

#[test]
fn positions_no_pointer_could_give_finish_at_once_and_harm_nothing() {
    let mut e = palette_editor();
    let far = [i32::MIN, i32::MAX];
    for (x, y) in [(far[0], far[1]), (far[1], far[0])] {
        e.apply(DrawCmd::Begin {
            tool: Tool::Pencil,
            x,
            y,
        })
        .expect("begin");
        e.apply(DrawCmd::To { x: y, y: x }).expect("to");
        e.apply(DrawCmd::Commit).expect("commit");
        e.apply(DrawCmd::EraseCell { x, y }).expect("erase");
        e.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(x, y), (y, x), (x, x)],
        })
        .expect("stroke");
    }
    assert!(e.doc.frames.len() == 1);
}

#[test]
fn floating_pixels_cannot_be_pushed_beyond_reach() {
    let mut e = lifted();
    for _ in 0..3 {
        e.apply(FloatingCmd::Move {
            dx: i32::MAX,
            dy: i32::MIN,
            join: false,
        })
        .expect("moved");
    }
    let (x, y, _, _) = *e
        .selection()
        .map(|m| m.bounds())
        .as_ref()
        .expect("selected");
    assert!(x <= REACH && y >= -REACH);
    e.apply(FloatingCmd::Scale {
        x: i32::MIN,
        y: i32::MAX,
        w: i32::MAX,
        h: i32::MIN,
    })
    .expect("scaled");
    e.apply(FloatingCmd::Anchor).expect("anchored");
    assert!(!e.is_floating());
}

#[test]
fn a_selection_outside_reach_is_refused() {
    let mut e = palette_editor();
    let result = e.apply(SelectionCmd::Select {
        shape: Box::new(Mask::rect(REACH + 1, 0, 2, 2)),
        op: SelectOp::Replace,
    });
    assert_eq!(result, Err(LimitError::Dimensions));
    assert!(e.selection().is_none());
}

#[test]
fn a_clip_whose_size_and_pixels_disagree_is_refused() {
    let mut e = palette_editor();
    e.select(Mask::rect(0, 0, 2, 2), SelectOp::Replace);
    let copied = e.copy_selected().expect("copied");
    let mut wider = copied.clone();
    wider.width += 1;
    wider.colors.extend([0, 0]);
    assert!(
        wider.ids.is_some(),
        "needs the case where the ids are stale"
    );
    for clip in [wider, Clip::from_image(2, 2, vec![0; 3])] {
        let paste = e.apply(FloatingCmd::Paste {
            clip: std::sync::Arc::new(clip),
            x: 0,
            y: 0,
        });
        assert_eq!(paste, Err(LimitError::Dimensions));
    }
    assert!(!e.is_floating());
}

#[test]
fn a_clip_too_large_to_measure_is_refused_without_overflowing() {
    let mut e = palette_editor();
    let paste = e.apply(FloatingCmd::Paste {
        clip: std::sync::Arc::new(Clip::from_image(usize::MAX, 2, Vec::new())),
        x: 0,
        y: 0,
    });
    assert_eq!(paste, Err(LimitError::Dimensions));
}

#[test]
fn a_move_with_no_distance_leaves_the_floating_undo_stack_alone() {
    let mut e = lifted_and_moved();
    e.move_floating(1, 0);
    let moved = |dx| FloatingCmd::Move {
        dx,
        dy: 0,
        join: false,
    };
    e.apply(moved(1)).expect("a stop to undo");
    let stamp = e.floating_stamp();
    e.apply(moved(0)).expect("nothing");
    assert_eq!(e.floating_stamp(), stamp);
    assert!(e.undo());
    assert!(e.is_floating());
    assert!(e.redo(), "the move can still be redone");
}

#[test]
fn a_value_dragged_back_over_a_paste_keeps_the_way_back_to_the_selection() {
    let mut e = palette_editor();
    e.select(Mask::rect(0, 0, 4, 4), SelectOp::Replace);
    let clip = e.copy_selected().expect("copied");
    let (x, y) = (clip.x, clip.y);
    e.apply(FloatingCmd::Paste {
        clip: std::sync::Arc::new(clip),
        x,
        y,
    })
    .expect("pasted");
    let layer = e.layer_id(0).expect("layer");
    let opacity = e.doc.layers[0].opacity;
    e.apply(LayerCmd::SetOpacity { layer, opacity: 3 })
        .expect("set");
    e.apply(LayerCmd::SetOpacity { layer, opacity })
        .expect("set");
    e.apply(StepCmd::EndDrag).expect("ended");
    assert!(e.undo());
    assert_eq!(e.selection().map(|m| m.bounds()), Some((0, 0, 4, 4)));
}

#[test]
fn a_rewound_revision_is_still_told_apart() {
    let mut e = palette_editor();
    e.apply(TagCmd::Add {
        name: "walk".into(),
        from: 0,
        to: 0,
    })
    .expect("added");
    let during = (e.revision, e.revision_rewinds());
    e.apply(StepCmd::AbortJoin).expect("aborted");
    e.begin(Tool::Pencil, 3, 3).expect("begin");
    e.commit();
    assert_eq!(e.revision, during.0, "the number really is used again");
    assert_ne!((e.revision, e.revision_rewinds()), during);
}

fn bytes_for_a_frame_added_at(at: usize) -> usize {
    let mut e = palette_editor();
    for _ in 0..40 {
        e.apply(FrameCmd::Add { at: 0, copy: false }).expect("room");
    }
    for frame in 0..41 {
        e.frame = frame;
        e.begin(Tool::Pencil, 2, 2).expect("begin");
        e.commit();
    }
    let used = e.history.memory_used();
    e.apply(FrameCmd::Add { at, copy: false }).expect("room");
    e.history.memory_used() - used
}

#[test]
fn adding_a_frame_in_the_middle_costs_the_same_as_adding_one_at_the_end() {
    assert_eq!(
        bytes_for_a_frame_added_at(20),
        bytes_for_a_frame_added_at(41)
    );
}

#[test]
fn moving_nothing_reports_no_change() {
    let mut e = palette_editor();
    let moved = e
        .apply(FloatingCmd::Move {
            dx: 1,
            dy: 0,
            join: false,
        })
        .expect("fine");
    assert!(!moved.changed);
}

#[test]
fn a_value_dragged_back_leaves_the_revision_where_it_was() {
    let mut e = palette_editor();
    let layer = e.layer_id(0).expect("layer");
    let (opacity, revision) = (e.doc.layers[0].opacity, e.revision);
    e.apply(LayerCmd::SetOpacity { layer, opacity: 3 })
        .expect("set");
    e.apply(LayerCmd::SetOpacity { layer, opacity })
        .expect("set");
    e.apply(StepCmd::EndDrag).expect("end");
    assert_eq!(e.revision, revision, "the file has not really changed");
}

#[test]
fn a_cancelled_new_tag_leaves_the_revision_where_it_was() {
    let mut e = palette_editor();
    let revision = e.revision;
    e.apply(TagCmd::Add {
        name: "walk".into(),
        from: 0,
        to: 0,
    })
    .expect("added");
    assert_ne!(e.revision, revision);
    e.apply(StepCmd::AbortJoin).expect("aborted");
    assert_eq!(e.revision, revision);
}

fn at_most_what_a_scale_can_use(bytes: usize) -> bool {
    bytes <= 400 << 20
}

#[test]
fn the_estimate_for_scaling_uses_the_size_it_will_really_be() {
    let mut e = lifted();
    e.set_memory_check(at_most_what_a_scale_can_use);
    let scaled = e.apply(FloatingCmd::Scale {
        x: 0,
        y: 0,
        w: i32::MAX,
        h: i32::MAX,
    });
    assert!(scaled.is_ok(), "{scaled:?}");
}

fn a_stroke_allowance_at_most(bytes: usize) -> bool {
    bytes <= 64 << 20
}

#[test]
fn a_fill_asks_for_room_for_the_whole_canvas() {
    let doc = Document::new(4096, 4096, ColorMode::Rgba).expect("valid");
    let mut e = Editor::new(doc);
    e.color = rgba(9, 9, 9, 255);
    e.set_memory_check(a_stroke_allowance_at_most);
    let (x, y) = (1, 1);
    let tool = Tool::Fill;
    assert_eq!(
        e.apply(DrawCmd::Begin { tool, x, y }),
        Err(LimitError::NoMemory)
    );
    let tool = Tool::Pencil;
    assert!(e.apply(DrawCmd::Begin { tool, x, y }).is_ok());
}

#[test]
fn cancelling_the_name_of_a_tag_added_while_another_is_renamed_takes_it_back() {
    let mut e = palette_editor();
    let walk = |name: &str| TagCmd::Add {
        name: name.into(),
        from: 0,
        to: 0,
    };
    e.apply(walk("walk")).expect("added");
    e.apply(StepCmd::EndDrag).expect("named");
    e.apply(TagCmd::SetName {
        tag: 0,
        name: "run".into(),
    })
    .expect("renamed");
    e.apply(walk("jump")).expect("added");
    e.apply(StepCmd::AbortJoin).expect("aborted");
    assert_eq!(e.doc.tags.len(), 1, "the new tag is taken back");
    assert_eq!(e.doc.tags[0].name, "run");
}
