//! Windows ending the session while the window is busy in a message box: the fallback writes
//! the work as of the last frame, without the app, and only once for the same work.

use crate::app::App;
use crate::recovery::Recovery;
use crate::test_app::*;
use std::path::{Path, PathBuf};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-standing-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn copy_in(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "piks"))
}

#[test]
fn the_fallback_writes_the_work_kept_at_the_last_frame_and_not_twice() {
    let _lock = SESSION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir("busy");
    let mut a = app(1);
    a.use_recovery(Recovery::try_in_dir(&dir).expect("usable"), false);
    a.keep_standing();
    assert!(
        pikslar_platform::run_session_fallback(pikslar_platform::QUERY_FALLBACK_WAIT),
        "nothing unsaved is safe"
    );
    assert!(copy_in(&dir).is_none(), "and nothing is written");
    dot(&mut a, 1, 1);
    a.keep_standing();
    // The window is now busy in a message box: only the fallback runs.
    assert!(pikslar_platform::run_session_fallback(
        pikslar_platform::QUERY_FALLBACK_WAIT
    ));
    let copy = copy_in(&dir).expect("the copy");
    let first = std::fs::metadata(&copy)
        .expect("meta")
        .modified()
        .expect("time");
    let restored = crate::files::open(&copy).expect("it opens");
    assert_eq!(restored.doc.width, a.editor.doc().width);
    std::thread::sleep(std::time::Duration::from_millis(30));
    assert!(pikslar_platform::run_session_fallback(
        pikslar_platform::QUERY_FALLBACK_WAIT
    ));
    let again = std::fs::metadata(&copy)
        .expect("meta")
        .modified()
        .expect("time");
    assert_eq!(first, again, "the same work is not written twice");
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_stroke_under_way_keeps_the_work_from_before_it() {
    let mut a = app(1);
    dot(&mut a, 1, 1);
    a.gesture = crate::app::Gesture::Stroke;
    a.keep_standing();
    assert!(
        a.standing.borrow().is_none(),
        "nothing from the middle of a stroke"
    );
}

#[test]
fn a_slow_save_does_not_hold_up_the_fallback_copy() {
    let _lock = SESSION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir("slow-save");
    let mut a = app(1);
    a.use_recovery(Recovery::try_in_dir(&dir).expect("usable"), false);
    dot(&mut a, 1, 1);
    // A save stuck on a slow disk for longer than Windows waits.
    a.save_delay = Some(std::time::Duration::from_secs(6));
    a.start_save_to(pikslar_platform::Window::detached(), dir.join("art.piks"));
    dot(&mut a, 2, 2);
    a.keep_standing();
    let start = std::time::Instant::now();
    assert!(
        pikslar_platform::run_session_fallback(pikslar_platform::QUERY_FALLBACK_WAIT),
        "the copy is written"
    );
    let took = start.elapsed();
    assert!(
        took < std::time::Duration::from_secs(2),
        "well inside the 5 s Windows gives: {took:?}"
    );
    assert!(copy_in(&dir).is_some());
    finish_saves(&mut a);
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A recovery write held for `after`, then let go: a slow disk that does finish.
fn slow_write(a: &mut App, after: std::time::Duration) -> std::thread::JoinHandle<()> {
    let (release, gate) = std::sync::mpsc::channel();
    a.recovery.hold_counted_writer(gate);
    std::thread::spawn(move || {
        std::thread::sleep(after);
        let _ = release.send(());
    })
}

#[test]
fn a_slow_write_that_ends_inside_the_cap_does_not_leave_the_end_without_the_latest_edits() {
    let _lock = SESSION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir("slow-write");
    let mut a = app(1);
    a.use_recovery(Recovery::try_in_dir(&dir).expect("usable"), false);
    dot(&mut a, 1, 1);
    a.keep_standing();
    let release = slow_write(&mut a, std::time::Duration::from_millis(1500));
    // Slower than the fallback's first second, inside the 4 s the end gets.
    assert!(
        pikslar_platform::run_session_fallback(pikslar_platform::END_WAIT),
        "the copy is written once the slow write ends"
    );
    let copy = copy_in(&dir).expect("a never-saved document still gets a copy");
    let restored = crate::files::open(&copy).expect("it opens");
    assert_eq!(
        restored.doc.layers()[0].cels()[0].image(),
        a.editor.doc().layers()[0].cels()[0].image(),
        "with the latest edits"
    );
    release.join().expect("released");
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_write_that_never_ends_inside_the_cap_leaves_the_older_copy_whole_and_says_so() {
    let _lock = SESSION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir("stuck-write");
    let mut a = app(1);
    a.use_recovery(Recovery::try_in_dir(&dir).expect("usable"), false);
    dot(&mut a, 1, 1);
    a.keep_standing();
    assert!(pikslar_platform::run_session_fallback(
        pikslar_platform::END_WAIT
    ));
    let older = copy_in(&dir).expect("an earlier copy");
    let older_bytes = std::fs::read(&older).expect("read");
    dot(&mut a, 4, 4);
    a.keep_standing();
    let (_stuck, gate) = std::sync::mpsc::channel();
    a.recovery.hold_counted_writer(gate);
    let start = std::time::Instant::now();
    let safe = pikslar_platform::run_session_fallback(std::time::Duration::from_millis(300));
    assert!(!safe, "no claim that the latest edits are safe");
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(std::fs::read(&older).expect("read"), older_bytes, "whole");
    assert!(crate::files::open(&older).is_ok(), "and it opens");
    drop(_stuck);
    a.recovery.wait_for_writer();
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_new_document_whose_only_write_never_ends_has_no_copy_and_the_end_is_not_safe() {
    let _lock = SESSION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir("stuck-new");
    let mut a = app(1);
    a.use_recovery(Recovery::try_in_dir(&dir).expect("usable"), false);
    dot(&mut a, 1, 1);
    a.keep_standing();
    let (_stuck, gate) = std::sync::mpsc::channel();
    a.recovery.hold_counted_writer(gate);
    assert!(!pikslar_platform::run_session_fallback(
        std::time::Duration::from_millis(200)
    ));
    assert!(copy_in(&dir).is_none(), "nothing half written");
    drop(_stuck);
    a.recovery.wait_for_writer();
    drop(a);
    let _ = std::fs::remove_dir_all(&dir);
}
