//! Ids that never name another object: persisted layer ids are not given again in a session,
//! and frame keys follow their frames through every command, undo and redo.

use crate::command::{FrameCmd, LayerCmd, StepCmd};
use crate::document::{ColorMode, Document, FrameInfo, LimitError};
use crate::editor::Editor;
use crate::ids::FrameKey;

fn editor() -> Editor {
    Editor::new(Document::new(8, 8, ColorMode::Rgba).expect("a small canvas"))
}

fn layer_ids(e: &Editor) -> Vec<u32> {
    e.doc()
        .layers()
        .iter()
        .map(|l| l.persisted_id().get())
        .collect()
}

fn keys(e: &Editor) -> Vec<FrameKey> {
    e.doc().frames.iter().map(FrameInfo::key).collect()
}

fn add_top(e: &mut Editor) -> u32 {
    let at = e.doc().layers().len();
    e.apply(LayerCmd::Add {
        at,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    layer_ids(e)[at]
}

fn remove_top(e: &mut Editor) {
    let top = e.doc().layers().len() - 1;
    let layer = e.layer_id(top).expect("a layer");
    e.apply(LayerCmd::Remove { layer }).expect("removes");
}

fn step(e: &mut Editor, command: StepCmd) {
    e.apply(command).expect("history");
}

#[test]
fn a_removed_top_layer_does_not_give_its_id_to_the_next_new_layer() {
    let mut e = editor();
    let first = add_top(&mut e);
    remove_top(&mut e);
    let next = add_top(&mut e);
    assert_ne!(next, first);
    assert!(next > first);
}

#[test]
fn undo_brings_a_removed_layer_back_with_its_id_and_new_layers_still_take_new_ones() {
    let mut e = editor();
    let a = add_top(&mut e);
    remove_top(&mut e);
    let b = add_top(&mut e);
    step(&mut e, StepCmd::Undo);
    step(&mut e, StepCmd::Undo);
    assert_eq!(
        layer_ids(&e).last(),
        Some(&a),
        "the removed layer is back as itself"
    );
    step(&mut e, StepCmd::Redo);
    assert!(!layer_ids(&e).contains(&a));
    let c = add_top(&mut e);
    assert!(c != a && c != b, "{c} is new: neither {a} nor {b}");
}

#[test]
fn an_undone_add_does_not_give_its_id_again() {
    let mut e = editor();
    let a = add_top(&mut e);
    step(&mut e, StepCmd::Undo);
    let b = add_top(&mut e);
    assert_ne!(a, b);
}

#[test]
fn a_redone_add_brings_back_the_same_id() {
    let mut e = editor();
    let a = add_top(&mut e);
    step(&mut e, StepCmd::Undo);
    step(&mut e, StepCmd::Redo);
    assert_eq!(layer_ids(&e).last(), Some(&a));
}

#[test]
fn a_failed_transaction_still_uses_up_the_ids_it_gave() {
    let mut e = editor();
    let mut given = 0;
    let failed: Result<(), LimitError> = e.transaction(|scratch| {
        given = add_top(scratch);
        Err(LimitError::NoSuchItem)
    });
    assert!(failed.is_err());
    assert!(
        !layer_ids(&e).contains(&given),
        "nothing of the run is kept"
    );
    assert_ne!(add_top(&mut e), given);
}

#[test]
fn a_transaction_keeps_the_ids_it_used_up() {
    let mut e = editor();
    let mut given = 0;
    e.transaction(|scratch| -> Result<(), LimitError> {
        given = add_top(scratch);
        remove_top(scratch);
        Ok(())
    })
    .expect("runs");
    assert_ne!(add_top(&mut e), given);
}

#[test]
fn documents_equal_in_content_are_equal_whatever_ids_they_used_up() {
    let mut e = editor();
    let before = e.doc().clone();
    add_top(&mut e);
    step(&mut e, StepCmd::Undo);
    assert_eq!(e.doc(), &before);
}

#[test]
fn every_frame_has_a_key_of_its_own() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
    e.apply(FrameCmd::Add { at: 0, copy: false }).expect("room");
    e.apply(FrameCmd::Duplicate { from: 0, to: 2 })
        .expect("room");
    let mut all = keys(&e);
    assert_eq!(all.len(), 6);
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 6, "copies and duplicates take new keys");
}

#[test]
fn a_key_follows_its_frame_through_inserts_moves_and_timing() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    let second = keys(&e)[1];
    e.apply(FrameCmd::Add { at: 0, copy: false }).expect("room");
    assert_eq!(keys(&e)[2], second);
    // Timings of their own, so that a move changes what is seen and is kept.
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![100, 150, 200],
    })
    .expect("retimes");
    e.apply(StepCmd::EndDrag).expect("ends");
    assert_eq!(keys(&e)[2], second);
    e.apply(FrameCmd::Move { from: 2, to: 0 }).expect("moves");
    assert_eq!(keys(&e)[0], second);
    e.apply(FrameCmd::MoveRun {
        from: 0,
        to: 0,
        at: 1,
    })
    .expect("moves");
    assert_eq!(keys(&e)[1], second);
    e.apply(FrameCmd::Retime {
        from: 1,
        durations: vec![250],
    })
    .expect("retimes");
    assert_eq!(keys(&e)[1], second);
    assert_eq!(e.doc().frames[1].duration_ms, 250);
}

#[test]
fn undo_brings_a_removed_frame_back_with_its_key() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    let before = keys(&e);
    e.apply(FrameCmd::Remove { from: 1, to: 1 })
        .expect("removes");
    assert_eq!(keys(&e), before[..1]);
    step(&mut e, StepCmd::Undo);
    assert_eq!(keys(&e), before);
    step(&mut e, StepCmd::Redo);
    assert_eq!(keys(&e), before[..1]);
}

#[test]
fn undo_puts_back_the_order_of_frames_that_look_the_same() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    e.apply(FrameCmd::Add { at: 2, copy: false }).expect("room");
    e.apply(FrameCmd::Retime {
        from: 2,
        durations: vec![200],
    })
    .expect("retimes");
    e.apply(StepCmd::EndDrag).expect("ends");
    let before = keys(&e);
    e.apply(FrameCmd::Move { from: 0, to: 2 }).expect("moves");
    assert_eq!(keys(&e), vec![before[1], before[2], before[0]]);
    step(&mut e, StepCmd::Undo);
    assert_eq!(
        keys(&e),
        before,
        "the two blank frames of one timing go back as themselves"
    );
    step(&mut e, StepCmd::Redo);
    assert_eq!(keys(&e), vec![before[1], before[2], before[0]]);
}

#[test]
fn a_move_that_leaves_nothing_to_see_leaves_every_key_where_it_was() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    let before = keys(&e);
    e.apply(FrameCmd::Move { from: 0, to: 1 }).expect("moves");
    assert_eq!(
        keys(&e),
        before,
        "put back whole, as a change with nothing to see is"
    );
}

#[test]
fn a_document_from_parts_gives_copied_frames_keys_of_their_own() {
    let doc = Document::new(4, 4, ColorMode::Rgba).expect("size");
    let frames = vec![FrameInfo::new(100); 3];
    let layer = doc.layers()[0].clone().with_cels(vec![doc.blank_cel(); 3]);
    let parts = Document::from_parts(4, 4, doc.colors.clone(), vec![layer], frames);
    let made = parts.expect("whole");
    let mut all: Vec<FrameKey> = made.frames.iter().map(FrameInfo::key).collect();
    all.dedup();
    assert_eq!(all.len(), 3);
}

#[test]
fn frames_compare_by_timing_only() {
    assert_eq!(FrameInfo::new(100), FrameInfo::new(100));
    assert_ne!(FrameInfo::new(100).key(), FrameInfo::new(100).key());
    assert_ne!(FrameInfo::new(100), FrameInfo::new(120));
}
