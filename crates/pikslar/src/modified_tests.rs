//! Whether the project reads as modified: it does whenever what is shown differs from what was
//! saved, moved floating pixels included, and stops when the change is undone or put back.

use crate::app::App;
use crate::test_app::{app, dot};
use pikslar_core::{Editor, FloatingCmd, Mask, SelectOp, SelectionCmd};
use pikslar_platform::Window;

/// A saved project with one pixel at (1, 1), selected.
fn saved_with_a_pixel() -> App {
    let mut a = app(1);
    dot(&mut a, 1, 1);
    a.saved_revision = a.editor.revision();
    a.editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(1, 1, 1, 1)),
            op: SelectOp::Replace,
        })
        .expect("select");
    assert!(!a.is_modified());
    a
}

fn lift(a: &mut App, copy: bool) {
    a.editor.apply(FloatingCmd::Lift { copy }).expect("lift");
}

fn shift(a: &mut App, dx: i32) {
    a.editor
        .apply(FloatingCmd::Move {
            dx,
            dy: 0,
            join: false,
        })
        .expect("move");
}

#[test]
fn lifting_alone_is_not_a_change() {
    let mut a = saved_with_a_pixel();
    lift(&mut a, false);
    assert!(a.editor.is_floating());
    assert!(!a.is_modified());
    assert!(!a.document_label().ends_with('*'));
}

#[test]
fn moving_lifted_pixels_marks_the_project_modified_at_once() {
    let mut a = saved_with_a_pixel();
    lift(&mut a, false);
    shift(&mut a, 3);
    assert!(a.is_modified(), "moved, not yet placed");
    assert!(a.document_label().ends_with(" *"));
    assert!(a.has_unsaved_work());
}

#[test]
fn putting_moved_pixels_back_is_no_longer_a_change() {
    let mut a = saved_with_a_pixel();
    lift(&mut a, false);
    shift(&mut a, 3);
    a.editor.apply(FloatingCmd::Revert).expect("put back");
    assert!(!a.editor.is_floating());
    assert!(!a.is_modified());
}

#[test]
fn undo_puts_moved_pixels_back_and_is_no_longer_a_change() {
    let mut a = saved_with_a_pixel();
    lift(&mut a, false);
    shift(&mut a, 3);
    a.undo();
    assert!(
        !a.editor.is_floating(),
        "undo puts lifted pixels back, as before"
    );
    assert!(!a.is_modified());
    a.redo();
    assert!(!a.editor.is_floating(), "nothing was undone to redo");
    assert!(!a.is_modified());
}

#[test]
fn placing_moved_pixels_stays_a_change_until_saved() {
    let mut a = saved_with_a_pixel();
    lift(&mut a, false);
    shift(&mut a, 3);
    assert!(a.settle());
    assert!(a.is_modified());
    a.saved_revision = a.editor.revision();
    assert!(!a.is_modified());
}

#[test]
fn a_moved_copy_is_a_change() {
    let mut a = saved_with_a_pixel();
    lift(&mut a, true);
    shift(&mut a, 2);
    assert!(a.is_modified());
}

// Undo and redo advance the document's revision, so the revision alone cannot say that the
// picture is back where it was saved. The editor names the saved place in its history as well.

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-modified-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

/// An app saved to a file in `dir` after one dot at (1, 1).
fn saved_to_file(dir: &std::path::Path) -> App {
    let mut a = app(1);
    dot(&mut a, 1, 1);
    assert!(a.save_to(Window::detached(), dir.join("art.piks")), "saved");
    assert!(!a.is_modified());
    a
}

#[test]
fn a_stroke_and_its_undo_leave_the_project_unmodified() {
    let mut a = app(1);
    assert!(!a.is_modified());
    dot(&mut a, 3, 3);
    assert!(a.is_modified());
    a.undo();
    assert!(!a.is_modified(), "back at the opened picture");
    assert!(!a.document_label().ends_with('*'));
    a.redo();
    assert!(a.is_modified());
}

#[test]
fn undoing_thousands_of_edits_returns_to_the_opened_picture() {
    let mut a = app(1);
    for i in 0..2_000 {
        dot(&mut a, i % 32, (i / 32) % 32);
    }
    assert!(a.is_modified());
    while a.editor.history().can_undo() {
        a.undo();
    }
    assert!(!a.is_modified());
}

#[test]
fn save_undo_redo_is_clean_only_at_the_saved_place() {
    let dir = temp_dir("redo");
    let mut a = saved_to_file(&dir);
    a.undo();
    assert!(a.is_modified(), "the dot is gone from what was saved");
    a.redo();
    assert!(!a.is_modified(), "back at the saved picture");
    dot(&mut a, 2, 2);
    assert!(a.is_modified());
    a.undo();
    assert!(!a.is_modified(), "the edit after the save is undone");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_edit_after_undoing_past_the_save_can_never_read_as_saved() {
    let dir = temp_dir("branch");
    let mut a = saved_to_file(&dir);
    a.undo();
    dot(&mut a, 9, 9);
    assert!(a.is_modified());
    a.undo();
    assert!(
        a.is_modified(),
        "that is the picture before the save, not the saved one"
    );
    a.redo();
    assert!(a.is_modified());
    dot(&mut a, 1, 1);
    assert!(a.is_modified(), "the dropped branch is not reachable");
    while a.editor.history().can_undo() {
        a.undo();
        assert!(a.is_modified());
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_change_outside_the_history_keeps_the_project_modified_through_undo() {
    let dir = temp_dir("outside");
    let mut a = saved_to_file(&dir);
    let layer = a.editor.layer_id(0).expect("layer");
    a.run(pikslar_core::LayerCmd::SetVisible {
        layer,
        visible: false,
    });
    assert!(a.is_modified());
    dot(&mut a, 2, 2);
    a.undo();
    assert!(a.is_modified(), "hiding the layer is not in the history");
    a.undo();
    a.redo();
    assert!(a.is_modified());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_value_still_being_dragged_when_saved_stays_unsaved_as_it_goes_on() {
    let dir = temp_dir("drag");
    let mut a = saved_to_file(&dir);
    let layer = a.editor.layer_id(0).expect("layer");
    let set = |a: &mut App, opacity| a.run(pikslar_core::LayerCmd::SetOpacity { layer, opacity });
    set(&mut a, 40);
    assert!(a.save_to(Window::detached(), dir.join("art.piks")));
    assert!(!a.is_modified());
    set(&mut a, 90);
    assert!(a.is_modified());
    a.undo();
    a.redo();
    assert!(a.is_modified(), "the step now holds another value");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_saved_place_is_the_copys_not_the_one_at_the_end_of_the_save() {
    let dir = temp_dir("copy");
    let mut a = saved_to_file(&dir);
    a.save_delay = Some(std::time::Duration::from_millis(300));
    dot(&mut a, 2, 2);
    a.start_save_to(Window::detached(), dir.join("art.piks"));
    dot(&mut a, 3, 3);
    crate::test_app::finish_saves(&mut a);
    assert!(a.is_modified(), "the second dot is not in the file");
    a.undo();
    assert!(!a.is_modified(), "the picture of the copy");
    a.undo();
    assert!(a.is_modified());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_copy_that_was_undone_past_while_saving_never_reads_as_saved_again() {
    let dir = temp_dir("copy-branch");
    let mut a = saved_to_file(&dir);
    a.save_delay = Some(std::time::Duration::from_millis(300));
    dot(&mut a, 2, 2);
    a.start_save_to(Window::detached(), dir.join("art.piks"));
    a.undo();
    dot(&mut a, 7, 7);
    crate::test_app::finish_saves(&mut a);
    assert!(a.is_modified());
    a.undo();
    assert!(a.is_modified(), "the copy's picture is gone");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn undoing_past_what_the_history_kept_never_reads_as_the_opened_picture() {
    let mut a = app(1);
    let doc = a.editor.doc().clone();
    a.editor = Editor::with_undo_budget(doc, 4_000);
    a.editor.mark_saved();
    for i in 0..200 {
        dot(&mut a, i % 32, (i / 32) % 32);
    }
    while a.editor.history().can_undo() {
        a.undo();
    }
    assert!(a.editor.history().rewind_marks().below > 0, "steps went");
    assert!(
        a.is_modified(),
        "the oldest steps are gone, so it is not the opened picture"
    );
}

#[test]
fn a_placed_stamp_after_a_save_is_modified_and_stays_so_when_undone() {
    let dir = temp_dir("stamp");
    let mut a = saved_to_file(&dir);
    let depth = a.editor.history().undo_depth();
    a.editor
        .apply(SelectionCmd::Select {
            shape: Box::new(Mask::rect(1, 1, 1, 1)),
            op: SelectOp::Replace,
        })
        .expect("select");
    lift(&mut a, true);
    shift(&mut a, 3);
    assert!(a.is_modified());
    assert!(a.settle());
    assert!(a.is_modified());
    while a.editor.history().undo_depth() > depth {
        a.undo();
    }
    assert!(!a.is_modified(), "the saved picture again");
    let _ = std::fs::remove_dir_all(&dir);
}
