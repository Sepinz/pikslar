//! A transaction: many commands kept as one undo step, or none at all.

use crate::color::rgba;
use crate::command::{DrawCmd, FrameCmd, LayerCmd, PaletteCmd, StepCmd};
use crate::document::{ColorMode, Document, LimitError};
use crate::editor::{Editor, Tool};
use crate::mask::Mask;
use crate::selection::SelectOp;

const INK: u32 = rgba(10, 200, 30, 255);

fn editor() -> Editor {
    let mut e = Editor::new(Document::new(16, 16, ColorMode::Rgba).expect("a small canvas"));
    e.color = INK;
    e.apply(DrawCmd::Stroke {
        tool: Tool::Pencil,
        path: vec![(1, 1)],
    })
    .expect("a dot");
    e
}

/// A script's worth of commands: a layer, a frame, pixels, a name and a swatch.
fn script(e: &mut Editor) -> Result<(), LimitError> {
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })?;
    let layer = e.layer_id(1).ok_or(LimitError::NoSuchItem)?;
    e.apply(LayerCmd::Rename {
        layer,
        name: "Ink".into(),
    })?;
    e.apply(FrameCmd::Add { at: 1, copy: true })?;
    e.layer = 1;
    e.frame = 1;
    e.apply(DrawCmd::Stroke {
        tool: Tool::Line,
        path: vec![(0, 0), (7, 7)],
    })?;
    e.apply(LayerCmd::SetOpacity { layer, opacity: 90 })?;
    e.apply(StepCmd::EndDrag)?;
    e.apply(PaletteCmd::AddSwatch {
        color: rgba(1, 2, 3, 255),
    })?;
    Ok(())
}

#[test]
fn every_command_of_a_transaction_is_one_undo_step() {
    let mut e = editor();
    let before = e.doc().clone();
    let depth = e.history().undo_depth();
    let revision = e.revision();
    e.transaction(script).expect("the script runs");
    let after = e.doc().clone();
    assert_ne!(after, before);
    assert_eq!(e.history().undo_depth(), depth + 1, "one step");
    assert!(e.revision() > revision, "the file counts as changed");
    assert!(e.undo());
    assert_eq!(e.doc(), &before, "undo takes all of it back");
    assert_eq!(e.history().undo_depth(), depth);
    assert!(e.redo());
    assert_eq!(e.doc(), &after, "redo puts all of it back");
}

#[test]
fn a_transaction_matches_the_same_commands_run_one_by_one() {
    let mut by_commands = editor();
    script(&mut by_commands).expect("the commands run");
    let mut by_transaction = editor();
    by_transaction.transaction(script).expect("the script runs");
    assert_eq!(by_transaction.doc(), by_commands.doc());
    let ids = |e: &Editor| -> Vec<u32> {
        e.doc()
            .layers()
            .iter()
            .map(|l| l.persisted_id().get())
            .collect()
    };
    assert_eq!(ids(&by_transaction), ids(&by_commands));
}

#[test]
fn a_transaction_that_fails_changes_nothing() {
    let mut e = editor();
    assert!(e.undo());
    let before = e.doc().clone();
    let (depth, redo, pushes, revision) = (
        e.history().undo_depth(),
        e.history().redo_depth(),
        e.history().pushes(),
        e.revision(),
    );
    let failed: Result<(), LimitError> = e.transaction(|s| {
        script(s)?;
        s.apply(FrameCmd::Remove { from: 9, to: 9 })?;
        Ok(())
    });
    assert_eq!(failed, Err(LimitError::NoSuchItem));
    assert_eq!(e.doc(), &before);
    assert_eq!(e.history().undo_depth(), depth);
    assert_eq!(e.history().redo_depth(), redo, "redo is kept");
    assert_eq!(e.history().pushes(), pushes);
    assert_eq!(e.revision(), revision);
}

#[test]
fn a_transaction_that_changes_nothing_records_nothing() {
    let mut e = editor();
    let (depth, revision) = (e.history().undo_depth(), e.revision());
    e.transaction(|s| {
        let layer = s.layer_id(0).ok_or(LimitError::NoSuchItem)?;
        s.apply(LayerCmd::SetOpacity { layer, opacity: 3 })?;
        s.apply(LayerCmd::SetOpacity {
            layer,
            opacity: 255,
        })?;
        s.apply(StepCmd::EndDrag)
    })
    .expect("runs");
    assert_eq!(e.history().undo_depth(), depth);
    assert_eq!(e.revision(), revision);
}

#[test]
fn removing_and_moving_layers_undo_exactly() {
    let mut e = editor();
    for at in 1..4 {
        e.apply(LayerCmd::Add {
            at,
            mode: ColorMode::Palette,
        })
        .expect("room");
    }
    let before = e.doc().clone();
    e.transaction(|s| {
        let top = s.layer_id(3).ok_or(LimitError::NoSuchItem)?;
        let second = s.layer_id(1).ok_or(LimitError::NoSuchItem)?;
        s.apply(LayerCmd::Move { layer: top, to: 0 })?;
        s.apply(LayerCmd::Remove { layer: second })?;
        s.apply(LayerCmd::Duplicate { layer: top })
    })
    .expect("runs");
    let after = e.doc().clone();
    assert_eq!(after.layers().len(), 4);
    assert!(e.undo());
    assert_eq!(e.doc(), &before);
    assert!(e.redo());
    assert_eq!(e.doc(), &after);
}

#[test]
fn visibility_set_by_a_transaction_counts_as_a_change_and_is_never_undone() {
    let mut e = editor();
    let revision = e.revision();
    e.transaction(|s| {
        let layer = s.layer_id(0).ok_or(LimitError::NoSuchItem)?;
        s.apply(LayerCmd::SetVisible {
            layer,
            visible: false,
        })
    })
    .expect("runs");
    assert!(!e.doc().layers()[0].visible);
    assert!(e.revision() > revision);
}

#[test]
fn a_transaction_waits_for_floating_pixels() {
    let mut e = editor();
    e.select(Mask::rect(0, 0, 4, 4), SelectOp::Replace);
    assert!(e.lift(false));
    let before = e.doc().clone();
    let result: Result<(), LimitError> = e.transaction(script);
    assert_eq!(result, Err(LimitError::Busy));
    assert_eq!(e.doc(), &before);
}

#[test]
fn a_transaction_waits_for_a_stroke_in_progress() {
    let mut e = editor();
    e.apply(DrawCmd::Begin {
        tool: Tool::Pencil,
        x: 3,
        y: 3,
    })
    .expect("pressed");
    let result: Result<(), LimitError> = e.transaction(script);
    assert_eq!(result, Err(LimitError::Busy));
    assert!(e.is_stroking(), "the stroke goes on");
}

#[test]
fn a_canvas_resize_in_a_transaction_clears_the_selection_and_undo_brings_it_back() {
    let mut e = editor();
    e.select(Mask::rect(0, 0, 4, 4), SelectOp::Replace);
    let selected = e.selection.clone();
    e.transaction(|s| {
        s.apply(crate::command::CanvasCmd::Resize {
            width: 20,
            height: 12,
            dx: 2,
            dy: 0,
        })
    })
    .expect("runs");
    assert_eq!((e.doc().width, e.doc().height), (20, 12));
    assert!(e.selection.is_none());
    assert!(e.undo());
    assert_eq!(e.selection, selected);
    assert_eq!((e.doc().width, e.doc().height), (16, 16));
}

#[test]
fn the_active_layer_follows_its_layer() {
    let mut e = editor();
    e.apply(LayerCmd::Add {
        at: 1,
        mode: ColorMode::Rgba,
    })
    .expect("room");
    e.layer = 1;
    let active = e.layer_id(1);
    e.transaction(|s| {
        s.apply(LayerCmd::Add {
            at: 0,
            mode: ColorMode::Rgba,
        })
    })
    .expect("runs");
    assert_eq!(e.layer_id(e.layer), active);
}

#[test]
fn holds_and_drawings_come_back_through_many_undos_and_redos() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 1, copy: true }).expect("room");
    let before = e.doc().clone();
    e.transaction(|s| {
        let layer = s.layer_id(0).ok_or(LimitError::NoSuchItem)?;
        s.apply(FrameCmd::Add { at: 2, copy: false })?;
        s.apply(FrameCmd::HoldPrevious { layer, frame: 1 })?;
        s.frame = 1;
        s.apply(DrawCmd::Stroke {
            tool: Tool::Pencil,
            path: vec![(5, 5), (9, 5)],
        })?;
        s.apply(FrameCmd::Release {
            layer,
            from: 0,
            to: 1,
        })
    })
    .expect("runs");
    let after = e.doc().clone();
    for _ in 0..3 {
        assert!(e.undo());
        assert_eq!(e.doc(), &before);
        assert!(e.redo());
        assert_eq!(e.doc(), &after);
    }
    assert!(e.history().memory_used() > 0);
}

/// The run's own editor never undoes, so its history keeps no more than one step however many
/// commands run: a long run's memory is the document's, not a growing log of what it did.
#[test]
fn a_long_transaction_keeps_no_history_of_its_own() {
    let mut e = editor();
    let kept = e
        .transaction(|s| {
            for _ in 0..200 {
                s.apply(FrameCmd::Add { at: 0, copy: true })?;
                s.apply(DrawCmd::Stroke {
                    tool: Tool::Line,
                    path: vec![(0, 0), (15, 15)],
                })?;
                s.apply(StepCmd::EndDrag)?;
                s.apply(FrameCmd::Remove { from: 0, to: 0 })?;
            }
            Ok::<_, LimitError>((s.history().undo_depth(), s.history().memory_used()))
        })
        .expect("runs");
    assert!(kept.0 <= 1, "{} steps kept", kept.0);
    assert!(kept.1 < 64 << 10, "{} bytes kept", kept.1);
}

fn keys(e: &Editor) -> Vec<u64> {
    e.doc().frames.iter().map(|f| f.key().get()).collect()
}

/// A run whose frames end with the same timing and pixels but other keys still changed the
/// document: the step is kept, so the keys it gave out name its frames afterwards.
#[test]
fn a_transaction_that_only_renews_frame_keys_is_kept() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    let before = keys(&e);
    let depth = e.history().undo_depth();
    let made = e
        .transaction(|s| {
            // A copy of the first frame in its place, then the first frame removed.
            s.apply(FrameCmd::Add { at: 1, copy: true })?;
            s.apply(FrameCmd::Remove { from: 0, to: 0 })?;
            Ok::<_, LimitError>(keys(s))
        })
        .expect("runs");
    assert_ne!(made, before, "the copy has a key of its own");
    assert_eq!(keys(&e), made, "the frames the run made are the document's");
    assert_eq!(e.history().undo_depth(), depth + 1);
    e.apply(StepCmd::Undo).expect("undoes");
    assert_eq!(keys(&e), before);
    e.apply(StepCmd::Redo).expect("redoes");
    assert_eq!(keys(&e), made);
}

/// In the interface, a change that leaves nothing to see is still put back keys and all.
#[test]
fn outside_a_transaction_nothing_to_see_still_records_nothing() {
    let mut e = editor();
    e.apply(FrameCmd::Add { at: 1, copy: false }).expect("room");
    let depth = e.history().undo_depth();
    e.apply(FrameCmd::Retime {
        from: 0,
        durations: vec![e.doc().frames[0].duration_ms],
    })
    .expect("retimes");
    assert_eq!(e.history().undo_depth(), depth);
}
