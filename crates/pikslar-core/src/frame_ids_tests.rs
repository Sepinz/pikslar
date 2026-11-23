//! Persisted frame ids: one per frame, distinct in a document, never given again while it is
//! open, and kept by undo and redo as the frames themselves are. See `docs/stable-ids.md`.

use crate::command::{FrameCmd, StepCmd};
use crate::document::{ColorMode, Document, FrameInfo, LimitError};
use crate::editor::Editor;
use crate::ids::MAX_FRAME_ID;
use crate::layer::Layer;
use std::sync::Arc;

fn editor() -> Editor {
    Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("a small canvas"))
}

fn ids(e: &Editor) -> Vec<u32> {
    e.doc().frames.iter().map(FrameInfo::id).collect()
}

fn add(e: &mut Editor, at: usize, copy: bool) -> u32 {
    e.apply(FrameCmd::Add { at, copy }).expect("room");
    ids(e)[at]
}

fn step(e: &mut Editor, command: StepCmd) {
    e.apply(command).expect("history");
}

fn parts(frames: usize) -> Document {
    let mut layer = Layer::new("a".into(), ColorMode::Rgba);
    (0..frames).for_each(|_| layer.push_cel(crate::layer::Cel::blank()));
    let table = Arc::new(crate::color::ColorTable::new());
    let infos = vec![FrameInfo::new(80); frames];
    Document::from_parts(8, 8, table, vec![layer], infos).expect("valid parts")
}

#[test]
fn a_new_document_numbers_its_frame_1() {
    let e = editor();
    assert_eq!(ids(&e), [1]);
    assert_eq!(e.doc().frame_floor(), 1);
}

#[test]
fn every_way_of_making_a_frame_gives_a_new_id() {
    let mut e = editor();
    add(&mut e, 1, true);
    add(&mut e, 0, false);
    e.apply(FrameCmd::Duplicate { from: 0, to: 2 })
        .expect("room");
    let mut seen = ids(&e);
    assert_eq!(seen.len(), 6);
    seen.sort_unstable();
    assert_eq!(seen, [1, 2, 3, 4, 5, 6], "distinct, and counted up");
}

#[test]
fn ids_follow_their_frames_through_moves() {
    let mut e = editor();
    let second = add(&mut e, 1, false);
    let third = add(&mut e, 2, false);
    // Frames that look alike are not moved: a move with nothing to see is put back whole.
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![10, 20, 30],
    })
    .expect("retimes");
    e.apply(FrameCmd::Move { from: 0, to: 2 }).expect("moves");
    assert_eq!(ids(&e), [second, third, 1]);
    e.apply(FrameCmd::MoveRun {
        from: 1,
        to: 2,
        at: 0,
    })
    .expect("moves");
    assert_eq!(ids(&e), [third, 1, second]);
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![40, 50, 60],
    })
    .expect("retimes");
    assert_eq!(ids(&e), [third, 1, second], "retiming keeps them");
}

#[test]
fn a_removed_frame_s_id_is_never_given_again() {
    let mut e = editor();
    let last = add(&mut e, 1, false);
    e.apply(FrameCmd::Remove { from: 1, to: 1 })
        .expect("removes");
    let next = add(&mut e, 1, false);
    assert!(next > last, "{next} after {last}");
}

#[test]
fn undo_and_redo_keep_ids_and_never_lower_the_floor() {
    let mut e = editor();
    let made = add(&mut e, 1, false);
    e.apply(FrameCmd::Remove { from: 0, to: 0 })
        .expect("removes");
    assert_eq!(ids(&e), [made]);
    step(&mut e, StepCmd::Undo);
    assert_eq!(ids(&e), [1, made], "undo brings the frame back as itself");
    step(&mut e, StepCmd::Undo);
    assert_eq!(ids(&e), [1]);
    step(&mut e, StepCmd::Redo);
    assert_eq!(ids(&e), [1, made], "redo gives back the same id");
    step(&mut e, StepCmd::Undo);
    assert_eq!(e.doc().frame_floor(), made, "undo leaves the floor alone");
    let again = add(&mut e, 1, false);
    assert!(again > made, "an undone add's id is not given again");
}

#[test]
fn a_failed_transaction_still_uses_up_the_frame_ids_it_gave() {
    let mut e = editor();
    let mut given = 0;
    let failed: Result<(), LimitError> = e.transaction(|scratch| {
        given = add(scratch, 1, false);
        Err(LimitError::Busy)
    });
    assert!(failed.is_err());
    assert_eq!(ids(&e), [1]);
    assert!(add(&mut e, 1, false) > given);
}

#[test]
fn a_reader_s_frames_are_numbered_from_1() {
    let doc = parts(3);
    let ids: Vec<u32> = doc.frames.iter().map(FrameInfo::id).collect();
    assert_eq!(ids, [1, 2, 3]);
    assert_eq!(doc.frame_floor(), 3);
}

#[test]
fn kept_ids_are_checked_and_raise_the_floor() {
    let mut doc = parts(3);
    for bad in [
        &[4, 5][..],
        &[4, 4, 5],
        &[0, 4, 5],
        &[4, 5, MAX_FRAME_ID + 1],
    ] {
        assert_eq!(doc.keep_frame_ids(bad), Err(LimitError::NoSuchItem));
        let kept: Vec<u32> = doc.frames.iter().map(FrameInfo::id).collect();
        assert_eq!(kept, [1, 2, 3], "{bad:?} changes nothing");
    }
    doc.keep_frame_ids(&[9, 2, 30]).expect("valid");
    let kept: Vec<u32> = doc.frames.iter().map(FrameInfo::id).collect();
    assert_eq!(kept, [9, 2, 30]);
    assert_eq!(doc.frame_floor(), 30);
    doc.raise_frame_floor(50);
    doc.raise_frame_floor(10);
    assert_eq!(doc.frame_floor(), 50, "never lowered");
}

#[test]
fn renumbering_gives_ids_above_every_one_given() {
    let mut doc = parts(3);
    doc.raise_frame_floor(40);
    doc.renumber_frames();
    let ids: Vec<u32> = doc.frames.iter().map(FrameInfo::id).collect();
    assert_eq!(ids, [41, 42, 43]);
    assert_eq!(doc.frame_floor(), 43);
}

#[test]
fn the_layer_floor_can_be_raised_but_not_past_the_largest_id() {
    let mut doc = parts(1);
    doc.raise_layer_floor(20);
    doc.raise_layer_floor(5);
    assert_eq!(doc.layer_floor(), 20);
    doc.raise_layer_floor(u32::MAX);
    assert_eq!(doc.layer_floor(), crate::PersistedLayerId::MAX);
}

#[test]
fn once_frame_ids_run_out_the_smallest_free_one_is_given() {
    let mut e = editor();
    let mut doc = e.doc().clone();
    doc.keep_frame_ids(&[MAX_FRAME_ID]).expect("valid");
    e = Editor::new(doc);
    assert_eq!(add(&mut e, 1, false), 1);
    assert_eq!(add(&mut e, 2, false), 2);
}

#[test]
fn documents_that_differ_only_in_frame_ids_are_equal() {
    let mut a = parts(2);
    let b = a.clone();
    a.raise_frame_floor(90);
    a.renumber_frames();
    assert_eq!(a, b);
}

#[test]
fn a_renumbered_layer_takes_an_id_above_every_one_given() {
    let mut doc = parts(1);
    doc.raise_layer_floor(20);
    assert_eq!(doc.renumber_layer(0), Ok(21));
    assert_eq!(doc.layers()[0].persisted_id().get(), 21);
    assert_eq!(doc.layer_floor(), 21);
    assert_eq!(doc.renumber_layer(1), Err(LimitError::NoSuchItem));
}

#[test]
fn kept_ids_may_leave_frames_to_be_given_new_ones_above_the_floor() {
    let mut doc = parts(4);
    doc.raise_frame_floor(20);
    for bad in [
        &[Some(4), None][..],
        &[Some(4), Some(4), None, None],
        &[Some(0), None, None, None],
        &[Some(MAX_FRAME_ID + 1), None, None, None],
    ] {
        assert_eq!(doc.keep_some_frame_ids(bad), Err(LimitError::NoSuchItem));
        let kept: Vec<u32> = doc.frames.iter().map(FrameInfo::id).collect();
        assert_eq!(kept, [1, 2, 3, 4], "{bad:?} changes nothing");
    }
    doc.keep_some_frame_ids(&[None, Some(7), None, Some(3)])
        .expect("valid");
    let kept: Vec<u32> = doc.frames.iter().map(FrameInfo::id).collect();
    assert_eq!(kept, [21, 7, 22, 3]);
    assert_eq!(doc.frame_floor(), 22);
}

/// A document of the most frames there may be, with the floor at the largest id: what a hostile
/// file could once ask for, which made opening and every new frame take a time that grew with
/// the cube and the square of the frame count.
fn full_at_the_cap() -> Document {
    let mut doc = parts(crate::document::MAX_FRAMES);
    doc.raise_frame_floor(MAX_FRAME_ID);
    doc
}

fn distinct(doc: &Document) -> bool {
    let mut ids: Vec<u32> = doc.frames.iter().map(FrameInfo::id).collect();
    ids.sort_unstable();
    ids.windows(2).all(|pair| pair[0] != pair[1]) && ids.first() != Some(&0)
}

#[test]
fn renumbering_the_most_frames_at_the_cap_is_quick() {
    let mut doc = full_at_the_cap();
    let started = std::time::Instant::now();
    doc.renumber_frames();
    let mut some: Vec<Option<u32>> = vec![None; doc.frames.len()];
    some[0] = Some(MAX_FRAME_ID);
    doc.keep_some_frame_ids(&some).expect("valid");
    // Linear work is a few milliseconds even unoptimised; the old way took minutes.
    assert!(started.elapsed().as_secs() < 3, "{:?}", started.elapsed());
    assert!(distinct(&doc));
}

#[test]
fn new_frames_at_the_cap_stay_quick() {
    let mut doc = full_at_the_cap();
    doc.renumber_frames();
    let started = std::time::Instant::now();
    // The last frame has the highest id, so the smallest free one is the last to look at.
    let last = doc.frames.len() - 1;
    for _ in 0..300 {
        doc.remove_frame(last).expect("removes");
        doc.add_frame(last, false).expect("room");
    }
    assert!(started.elapsed().as_secs() < 3, "{:?}", started.elapsed());
    assert!(distinct(&doc));
}
