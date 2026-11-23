//! Saving on its own thread: the window never waits for the disk, edits made meanwhile stay
//! unsaved, one more save queues, and closing waits for what is under way.

use super::*;
use crate::test_app::*;
use pikslar_platform::{Key, Mods};
use std::fs;

const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
/// Long enough that a key handler which waited for it would be caught.
const SLOW: Duration = Duration::from_millis(1500);

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-saverun-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

/// An app whose file is `art`, saved once, with an edit since and a slow saving thread.
fn slow_app(dir: &Path) -> (App, PathBuf) {
    let art = dir.join("art.piks");
    let mut a = app(1);
    assert!(a.save_to(Window::detached(), art.clone()), "the first save");
    dot(&mut a, 2, 2);
    a.save_delay = Some(SLOW);
    (a, art)
}

fn ctrl_s(a: &mut App) -> Duration {
    let start = Instant::now();
    a.key_down(Key::Char(b'S'), CTRL, false, Window::detached());
    start.elapsed()
}

fn shown(a: &App) -> String {
    a.save_label().map(|l| l.text).unwrap_or_default()
}

/// The bytes a save of `a`'s document as it is now writes.
fn bytes_now(a: &App) -> Vec<u8> {
    files::checked_bytes(a.editor.doc()).expect("encodes")
}

#[test]
fn ctrl_s_returns_at_once_and_says_saving_until_the_file_is_in_place() {
    let dir = temp_dir("at-once");
    let (mut a, art) = slow_app(&dir);
    let before = fs::read(&art).expect("the first file");
    let took = ctrl_s(&mut a);
    assert!(took < Duration::from_millis(500), "the key waited {took:?}");
    assert_eq!(shown(&a), "Saving...");
    assert!(a.document_label().ends_with(" *"), "not saved yet");
    assert_eq!(
        fs::read(&art).expect("still there"),
        before,
        "nothing in place yet"
    );
    finish_saves(&mut a);
    assert!(shown(&a).starts_with("Saved "), "{}", shown(&a));
    assert!(!a.is_modified());
    assert_ne!(fs::read(&art).expect("the new file"), before);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn edits_made_while_saving_stay_unsaved_and_the_file_holds_the_copy() {
    let dir = temp_dir("edits");
    let (mut a, art) = slow_app(&dir);
    let copied = a.editor.revision();
    let copy = bytes_now(&a);
    ctrl_s(&mut a);
    dot(&mut a, 5, 5);
    finish_saves(&mut a);
    assert_eq!(a.saved_revision, copied, "the saved revision is the copy's");
    assert!(a.is_modified(), "the later dot is unsaved");
    assert_eq!(shown(&a), "Unsaved changes");
    assert_eq!(fs::read(&art).expect("saved"), copy, "the file is the copy");
    assert_ne!(bytes_now(&a), copy, "which the later dot is not in");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn ctrl_s_while_saving_queues_one_more_and_never_a_second_writer() {
    let dir = temp_dir("queue");
    let (mut a, art) = slow_app(&dir);
    ctrl_s(&mut a);
    dot(&mut a, 5, 5);
    for _ in 0..3 {
        let took = ctrl_s(&mut a);
        assert!(took < Duration::from_millis(500), "{took:?}");
    }
    assert_eq!(a.save_next, Some(FollowUp::Save), "one save queued");
    assert!(a.save_job.is_some(), "one running");
    finish_saves(&mut a);
    assert!(a.save_job.is_none() && a.save_next.is_none());
    assert!(!a.is_modified(), "the queued save kept the later dot");
    assert_eq!(fs::read(&art).expect("saved"), bytes_now(&a));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_place_chosen_while_saving_is_saved_to_next_and_a_plain_save_does_not_replace_it() {
    let dir = temp_dir("save-as");
    let (mut a, art) = slow_app(&dir);
    let other = dir.join("other.piks");
    ctrl_s(&mut a);
    a.save_next = Some(FollowUp::To(other.clone()));
    assert_eq!(a.start_save(Window::detached(), false), Started::Queued);
    assert_eq!(a.save_next, Some(FollowUp::To(other.clone())));
    finish_saves(&mut a);
    assert!(art.exists() && other.exists());
    assert_eq!(
        a.path.as_deref(),
        Some(other.as_path()),
        "the new place is the file now"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn closing_while_saving_waits_until_the_file_is_in_place() {
    let dir = temp_dir("close");
    let (mut a, art) = slow_app(&dir);
    let before = fs::read(&art).expect("the first file");
    ctrl_s(&mut a);
    let start = Instant::now();
    assert!(
        a.confirm_discard(Window::detached()),
        "nothing unsaved after it"
    );
    assert!(start.elapsed() >= SLOW / 2, "it waited for the save");
    assert_ne!(
        fs::read(&art).expect("the new file"),
        before,
        "in place first"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_wait_for_a_save_ends_at_its_bound_and_leaves_the_save_running() {
    let dir = temp_dir("bound");
    let (mut a, _art) = slow_app(&dir);
    ctrl_s(&mut a);
    let start = Instant::now();
    assert!(!a.wait_for_saves(Window::detached(), Duration::from_millis(100)));
    assert!(start.elapsed() < SLOW, "the wait was bounded");
    assert!(a.save_job.is_some(), "and the save goes on");
    assert_eq!(shown(&a), "Saving...");
    finish_saves(&mut a);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_save_that_ends_after_the_document_was_replaced_marks_nothing_of_the_new_one() {
    let dir = temp_dir("replaced");
    let (mut a, art) = slow_app(&dir);
    ctrl_s(&mut a);
    let doc = pikslar_core::Document::new(8, 8, pikslar_core::ColorMode::Rgba).expect("doc");
    a.replace_document(doc, None);
    dot(&mut a, 1, 1);
    finish_saves(&mut a);
    assert!(art.exists());
    assert!(a.path.is_none(), "the new document has no file");
    assert!(a.is_modified(), "and its dot is unsaved");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_save_on_its_thread_says_so_and_keeps_the_work_unsaved() {
    let dir = temp_dir("fails");
    let target = dir.join("in the way.piks");
    fs::create_dir_all(&target).expect("a folder at its name");
    let mut a = app(1);
    dot(&mut a, 2, 2);
    assert_eq!(
        a.start_save_to(Window::detached(), target.clone()),
        Started::Now
    );
    finish_saves(&mut a);
    assert!(shown(&a).starts_with("Save failed: "), "{}", shown(&a));
    assert!(a.is_modified());
    assert!(a.path.is_none());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rewind_keeps_steps_only_for_a_save_of_the_revision_it_ends_at() {
    let dir = temp_dir("rewind");
    let art = dir.join("art.piks");
    let mut a = app(1);
    a.settings_saved.rewind = true;
    a.use_rewind(Some(dir.join("store")));
    dot(&mut a, 2, 2);
    a.path = Some(art.clone());
    a.save_delay = Some(SLOW);
    ctrl_s(&mut a);
    dot(&mut a, 5, 5);
    finish_saves(&mut a);
    assert!(art.exists());
    assert!(
        !a.rewind_writing(),
        "the steps after the copy are not this file's"
    );
    a.save_delay = None;
    assert!(a.save(Window::detached(), false));
    assert!(
        a.rewind_writing(),
        "a save of the revision shown keeps them"
    );
    a.finish_rewind();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_save_kept_back_while_one_runs_still_shows_saving_never_unsaved_or_saved() {
    let dir = temp_dir("blocked-saving");
    let (mut a, _art) = slow_app(&dir);
    ctrl_s(&mut a);
    a.open_new_dialog();
    ctrl_s(&mut a);
    let text = shown(&a);
    assert!(text.starts_with("Saving..."), "{text}");
    assert!(text.contains("next save waits"), "{text}");
    a.new_dialog = None;
    a.recheck_save_blocked();
    assert_eq!(shown(&a), "Saving...", "the dialog went, the save goes on");
    finish_saves(&mut a);
    assert!(shown(&a).starts_with("Saved "), "{}", shown(&a));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_queued_save_is_shown_as_queued() {
    let dir = temp_dir("queued-label");
    let (mut a, _art) = slow_app(&dir);
    ctrl_s(&mut a);
    dot(&mut a, 6, 6);
    ctrl_s(&mut a);
    assert_eq!(shown(&a), "Saving... (another save queued)");
    finish_saves(&mut a);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_queued_save_waits_for_a_gesture_to_end_and_says_so() {
    let dir = temp_dir("gesture");
    let (mut a, art) = slow_app(&dir);
    ctrl_s(&mut a);
    dot(&mut a, 6, 6);
    ctrl_s(&mut a);
    a.gesture = crate::app::Gesture::Stroke;
    finish_saves(&mut a);
    assert!(a.save_job.is_none(), "the first one ended");
    assert_eq!(a.save_next, Some(FollowUp::Save), "the next waits");
    assert_eq!(shown(&a), "Saving after you let go");
    a.gesture = crate::app::Gesture::None;
    a.save_delay = None;
    a.collect_save(Window::detached());
    finish_saves(&mut a);
    assert!(!a.is_modified(), "saved once the stroke ended");
    assert_eq!(fs::read(&art).expect("saved"), bytes_now(&a));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_failure_of_the_previous_documents_save_is_not_pinned_on_the_new_one() {
    let dir = temp_dir("old-doc-fails");
    let target = dir.join("in the way.piks");
    fs::create_dir_all(&target).expect("a folder at its name");
    let mut a = app(1);
    dot(&mut a, 2, 2);
    a.save_delay = Some(Duration::from_millis(300));
    assert_eq!(a.start_save_to(Window::detached(), target), Started::Now);
    let doc = pikslar_core::Document::new(8, 8, pikslar_core::ColorMode::Rgba).expect("doc");
    a.replace_document(doc, None);
    finish_saves(&mut a);
    assert!(
        a.save.failed.is_none(),
        "the new document's state is its own"
    );
    let said = said(&a);
    assert!(
        said.contains("in the way.piks"),
        "the notice names it: {said}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_failure_stays_shown_when_the_work_is_back_at_what_was_saved() {
    let dir = temp_dir("failed-unmodified");
    let target = dir.join("in the way.piks");
    fs::create_dir_all(&target).expect("a folder at its name");
    let (mut a, _art) = slow_app(&dir);
    a.save_delay = None;
    assert!(!a.save_to(Window::detached(), target), "fails");
    // Undone back to the file as saved: nothing unsaved, but the last save failed.
    a.saved_revision = a.editor.revision();
    assert!(!a.is_modified());
    assert!(shown(&a).starts_with("Save failed: "), "{}", shown(&a));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_place_chosen_while_saving_is_kept_when_the_moved_pixels_cannot_be_put_down() {
    use pikslar_core::{FloatingCmd, Mask, SelectOp, SelectionCmd};
    let dir = temp_dir("held-place");
    let (mut a, _art) = slow_app(&dir);
    let other = dir.join("other.piks");
    ctrl_s(&mut a);
    a.save_next = Some(FollowUp::To(other.clone()));
    a.run(SelectionCmd::Select {
        shape: Box::new(Mask::rect(0, 0, 4, 4)),
        op: SelectOp::Replace,
    });
    a.run(FloatingCmd::Paste {
        clip: std::sync::Arc::new(pikslar_core::Clip::from_image(1, 1, vec![0xff00_00ff])),
        x: 1,
        y: 1,
    });
    a.editor.set_memory_check(|_| false);
    finish_saves(&mut a);
    assert_eq!(a.save_next, Some(FollowUp::To(other.clone())), "kept");
    a.editor.set_memory_check(|_| true);
    a.save_delay = None;
    assert!(a.save(Window::detached(), false), "the next Save uses it");
    assert!(other.exists());
    assert_eq!(a.path.as_deref(), Some(other.as_path()));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_recovery_copy_survives_a_save_that_fails_on_its_thread() {
    let dir = temp_dir("keeps-recovery");
    let copies = dir.join("recovery");
    let target = dir.join("in the way.piks");
    fs::create_dir_all(&target).expect("a folder at its name");
    let mut a = app(1);
    a.use_recovery(crate::recovery::Recovery::in_dir(&copies), false);
    dot(&mut a, 2, 2);
    let floating = a.editor.floating_snapshot();
    a.recovery
        .write_now(a.editor.doc(), floating.as_ref(), Default::default())
        .expect("the work is copied");
    let copy = fs::read_dir(&copies)
        .expect("listed")
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "piks"))
        .expect("the copy");
    a.save_delay = Some(Duration::from_millis(300));
    assert_eq!(a.start_save_to(Window::detached(), target), Started::Now);
    a.tick_recovery();
    finish_saves(&mut a);
    a.tick_recovery();
    assert!(shown(&a).starts_with("Save failed: "), "{}", shown(&a));
    assert!(copy.is_file(), "the copy is kept while the work is unsaved");
    let kept = files::open(&copy).expect("the copy opens");
    assert_eq!(files::checked_bytes(&kept.doc).ok(), Some(bytes_now(&a)));
    a.recovery.simulate_crash();
    let next = crate::recovery::Recovery::in_dir(&copies);
    assert!(
        next.find_orphan().is_some(),
        "a crash now still leaves the work"
    );
    drop(next);
    let _ = fs::remove_dir_all(&dir);
}
