//! Opening on another thread: stopped with Esc, replaced by a newer request, and taken only
//! when it interrupts nothing, whatever the person did meanwhile.

use super::{Loading, Then, Wake};
use crate::app::{App, Gesture};
use crate::files::FileError;
use crate::session::Opened;
use crate::test_app::{app, dot};
use pikslar_format::FormatError;
use pikslar_platform::{Key, Mods, Window};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::ase_open_tests::ase_build::*;

fn win() -> Window {
    Window::detached()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-loading-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("folder");
    let pic = rgba_picture(2, 2, 10);
    let bytes = Ase::new(5, 3, 32)
        .frame(100, vec![plain("ink"), cel_raw(0, 1, 1, 2, 2, &pic)])
        .bytes();
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("written");
    path
}

/// Waits for the thread to end without taking its result, as the window does between frames.
fn until_read(a: &App) {
    let started = Instant::now();
    while !a.loading.as_ref().is_some_and(|l| l.is_done()) {
        assert!(started.elapsed() < Duration::from_secs(20), "the read ends");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_file_opens_in_the_background_and_is_taken_on_a_later_frame() {
    let mut a = app(1);
    let path = scratch("later.ase");
    a.begin_open(win(), path.clone(), Then::Nothing);
    assert_eq!(a.editor.doc().width, 32, "nothing changes yet");
    let said = a.status_text().expect("the status line says so");
    assert!(
        said.starts_with("Opening later.ase") && said.contains("Esc"),
        "{said}"
    );
    until_read(&a);
    a.collect_opened(win());
    assert!(a.loading.is_none());
    assert_eq!(a.editor.doc().width, 5);
    assert_eq!(a.imported_from.as_deref(), Some(path.as_path()));
    assert!(a.is_modified(), "an import is unsaved");
}

#[test]
fn esc_stops_opening_and_the_document_stays_as_it_was() {
    let mut a = app(1);
    let revision = a.editor.revision();
    a.begin_open(win(), scratch("stopped.ase"), Then::Nothing);
    a.key_down(Key::Escape, Mods::default(), false, win());
    assert!(a.loading.is_none());
    let said = a.status_text().expect("told");
    assert!(said.contains("Stopped opening stopped.ase"), "{said}");
    a.wait_for_stopped();
    a.collect_opened(win());
    assert_eq!(a.editor.doc().width, 32);
    assert_eq!(a.editor.revision(), revision, "the work is untouched");
    assert!(a.imported_from.is_none());
}

#[test]
fn esc_ends_a_gesture_before_it_stops_opening() {
    let mut a = app(1);
    a.begin_open(win(), scratch("gesture.ase"), Then::Nothing);
    a.gesture = Gesture::Pan { x: 0, y: 0 };
    a.key_down(Key::Escape, Mods::default(), false, win());
    assert_eq!(a.gesture, Gesture::None);
    assert!(a.loading.is_some(), "the first Esc was the gesture's");
    a.key_down(Key::Escape, Mods::default(), false, win());
    assert!(a.loading.is_none());
}

#[test]
fn a_result_waits_for_a_gesture_or_a_dialog_to_end() {
    let mut a = app(1);
    a.begin_open(win(), scratch("waits.ase"), Then::Nothing);
    until_read(&a);
    a.gesture = Gesture::Pan { x: 0, y: 0 };
    a.collect_opened(win());
    assert!(a.loading.is_some(), "a pan is not cut short");
    a.gesture = Gesture::None;
    a.open_new_dialog();
    a.collect_opened(win());
    assert!(
        a.loading.is_some(),
        "nor is a dialog replaced under the person"
    );
    a.close_new_dialog();
    a.collect_opened(win());
    assert_eq!(a.editor.doc().width, 5);
}

#[test]
fn the_newest_request_wins() {
    let mut a = app(1);
    a.begin_open(win(), scratch("first.ase"), Then::Nothing);
    let second = scratch("second.ase");
    a.begin_open(win(), second.clone(), Then::Nothing);
    assert_eq!(a.wait_for_open(win()), Some(Opened::Now));
    assert_eq!(a.imported_from.as_deref(), Some(second.as_path()));
    assert!(a.loading.is_none());
}

#[test]
fn a_file_that_cannot_be_read_is_reported_when_its_read_ends() {
    let mut a = app(1);
    let missing = std::env::temp_dir().join("pikslar-loading-missing.piks");
    a.recent = vec![missing.clone()];
    a.begin_open(win(), missing, Then::ForgetIfGone);
    assert_eq!(a.wait_for_open(win()), Some(Opened::Failed));
    assert!(
        a.recent.is_empty(),
        "a recent file that is gone leaves the list"
    );
    assert_eq!(a.editor.doc().width, 32);
}

#[test]
fn the_pointer_shows_work_going_on() {
    let mut a = app(1);
    assert_ne!(a.cursor(), pikslar_platform::Cursor::Busy);
    a.begin_open(win(), scratch("cursor.ase"), Then::Nothing);
    assert_eq!(a.cursor(), pikslar_platform::Cursor::Busy);
    a.wait_for_open(win());
    assert_ne!(a.cursor(), pikslar_platform::Cursor::Busy);
}

#[test]
fn unsaved_work_is_asked_about_before_the_read_not_when_it_ends() {
    let mut a = app(1);
    dot(&mut a, 1, 1);
    a.begin_open(win(), scratch("question.ase"), Then::Nothing);
    // A window that cannot answer keeps the work, and nothing is read.
    assert!(a.loading.is_none());
    assert!(a.status_text().is_none());
    assert_eq!(a.editor.doc().width, 32);
    assert!(a.is_modified());
}

#[test]
fn a_result_replaces_a_clean_document_without_asking() {
    let mut a = app(1);
    a.begin_open(win(), scratch("clean.ase"), Then::Nothing);
    until_read(&a);
    a.collect_opened(win());
    assert_eq!(a.editor.doc().width, 5);
}

#[test]
fn a_drawing_changed_while_opening_is_kept_and_the_result_dropped() {
    let mut a = app(1);
    a.begin_open(win(), scratch("changed.ase"), Then::Nothing);
    dot(&mut a, 2, 2);
    let revision = a.editor.revision();
    until_read(&a);
    a.collect_opened(win());
    assert!(a.loading.is_none());
    assert_eq!(a.editor.doc().width, 32, "the work stays");
    assert_eq!(a.editor.revision(), revision);
    assert!(a.is_modified());
    assert!(a.imported_from.is_none());
    let said = a.status_text().expect("told");
    assert!(
        said.starts_with("Opening changed.ase")
            && said.contains("was stopped because the drawing changed")
            && said.contains("Open it again to replace it"),
        "{said}"
    );
}

#[test]
fn the_result_is_in_place_before_the_window_is_woken() {
    let (woke, heard) = mpsc::channel();
    let (go, wait) = mpsc::channel::<()>();
    let wake: Wake = Box::new(move || {
        let _ = woke.send(());
        // Held here, the thread has not finished, as when the window wakes and looks.
        let _ = wait.recv();
    });
    let loading = Loading::start(
        scratch("wake.ase"),
        Then::Nothing,
        wake,
        None,
        (0, 0, 0),
        false,
    )
    .unwrap_or_else(|_| panic!("a thread starts"));
    heard.recv_timeout(Duration::from_secs(20)).expect("woken");
    assert!(
        loading.is_done(),
        "a frame run by the wake finds the result"
    );
    assert!(
        !loading.worker.is_finished(),
        "though the thread is still in wake"
    );
    drop(go);
    let (_, _, result) = loading.finish();
    assert!(result.is_ok());
}

#[test]
fn a_replaced_read_that_never_started_does_not_read_the_file() {
    let (release, held) = mpsc::channel::<()>();
    let earlier = std::thread::spawn(move || {
        let _ = held.recv();
    });
    let missing = std::env::temp_dir().join("pikslar-loading-never-read.ase");
    let loading = Loading::start(
        missing,
        Then::Nothing,
        Box::new(|| {}),
        Some(earlier),
        (0, 0, 0),
        false,
    )
    .unwrap_or_else(|_| panic!("a thread starts"));
    loading.stop.store(true, Ordering::Relaxed);
    drop(release);
    while !loading.is_done() {
        std::thread::sleep(Duration::from_millis(1));
    }
    let (_, _, result) = loading.finish();
    assert!(
        matches!(result, Err(FileError::Format(FormatError::Stopped))),
        "stopped, not a failure to find the file"
    );
}

#[test]
fn a_result_is_held_back_once_the_session_is_ending() {
    let mut a = app(1);
    a.begin_open(win(), scratch("ending.ase"), Then::Nothing);
    until_read(&a);
    assert!(a.end_session());
    a.collect_opened(win());
    assert!(a.loading.is_none());
    assert_eq!(a.editor.doc().width, 32, "nothing replaces the document");
    assert!(a.imported_from.is_none());
    a.wait_for_stopped();
}

#[test]
fn an_error_is_not_hidden_behind_the_opening_label() {
    let mut a = app(1);
    a.begin_open(win(), scratch("label.ase"), Then::Nothing);
    a.notify("Could not back up your work: disk full".to_string());
    let said = a.status_text().expect("says something");
    assert!(said.starts_with("Could not back up"), "{said}");
    a.wait_for_open(win());
}

#[test]
fn starting_a_read_clears_what_was_said_before_it() {
    let mut a = app(1);
    a.notify("Saved".to_string());
    a.begin_open(win(), scratch("fresh.ase"), Then::Nothing);
    let said = a.status_text().expect("says something");
    assert!(said.starts_with("Opening fresh.ase"), "{said}");
    a.wait_for_open(win());
}
