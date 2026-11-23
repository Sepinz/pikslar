//! Windows ending the session: the copy made at the question, made again only if the work
//! changed since, a drawn dialog left as it is, and the saves the window waits for.

use super::*;
use crate::recovery::Recovery;
use crate::test_app::*;
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-end-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

/// The one backup copy in `dir`, as it is now.
fn copy_in(dir: &Path) -> Option<Vec<u8>> {
    let entry = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.path().extension().is_some_and(|x| x == "piks"))?;
    std::fs::read(entry.path()).ok()
}

fn app_in(dir: &Path) -> App {
    let mut a = app(1);
    a.use_recovery(Recovery::try_in_dir(dir).expect("usable"), false);
    dot(&mut a, 1, 1);
    a
}

#[test]
fn the_end_after_the_question_copies_again_only_work_changed_since() {
    let dir = temp_dir("again");
    let mut a = app_in(&dir);
    assert!(a.end_session(), "the question");
    let asked = copy_in(&dir).expect("a copy");
    a.session_over(true);
    assert_eq!(
        copy_in(&dir).expect("kept"),
        asked,
        "nothing changed: not written again"
    );
    dot(&mut a, 3, 3);
    a.session_over(true);
    assert_ne!(
        copy_in(&dir).expect("kept"),
        asked,
        "the later dot is in the copy"
    );
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_end_with_no_question_first_still_copies_the_work() {
    let dir = temp_dir("unasked");
    let mut a = app_in(&dir);
    a.session_over(true);
    assert!(copy_in(&dir).is_some());
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_called_off_end_forgets_the_question() {
    let dir = temp_dir("off");
    let mut a = app_in(&dir);
    assert!(a.end_session());
    a.session_over(false);
    assert!(a.session_kept.is_none());
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_drawn_dialog_open_at_the_end_is_left_open_and_the_work_is_copied() {
    let dir = temp_dir("dialog");
    let mut a = app_in(&dir);
    a.open_new_dialog();
    assert!(
        a.end_session(),
        "a dialog drawn in the window does not refuse the end"
    );
    assert!(copy_in(&dir).is_some(), "the work is copied");
    assert!(
        a.new_dialog.is_some(),
        "the dialog is as it was, should the end be called off"
    );
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_under_way_counts_as_a_write_windows_waits_for() {
    let dir = temp_dir("save");
    let mut a = app(1);
    a.save_delay = Some(Duration::from_millis(400));
    a.start_save_to(Window::detached(), dir.join("art.piks"));
    assert!(
        pikslar_platform::writes_pending() >= 1,
        "the save holds one"
    );
    finish_saves(&mut a);
    assert!(files::open(&dir.join("art.piks")).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_slow_write_that_ends_inside_the_cap_still_gets_the_end_its_current_copy() {
    let _lock = SESSION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir("slow-end");
    let mut a = app_in(&dir);
    let (release, gate) = std::sync::mpsc::channel();
    a.recovery.hold_counted_writer(gate);
    let ends = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(3500));
        let _ = release.send(());
    });
    a.session_over(true);
    ends.join().expect("released");
    let copy = copy_in(&dir).expect("the copy of the work as it is");
    assert!(!copy.is_empty());
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_write_that_never_ends_inside_the_budget_is_reported_and_leaves_nothing_broken() {
    let _lock = SESSION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir("stuck-end");
    let mut a = app_in(&dir);
    let (stuck, gate) = std::sync::mpsc::channel();
    a.recovery.hold_counted_writer(gate);
    let start = Instant::now();
    a.session_over_within(true, Duration::from_millis(300));
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "bounded by the budget"
    );
    assert!(copy_in(&dir).is_none(), "no copy, and no half of one");
    assert!(a.session_kept.is_none(), "the work is not claimed safe");
    drop(stuck);
    a.recovery.wait_for_writer();
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}
