//! Every way the save state changes, and how each is shown (B1, section 13, requirement 1).

use super::*;
use crate::test_app::*;
use pikslar_platform::Window;
use std::fs;
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-state-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

fn shown(a: &App) -> Option<(String, bool)> {
    a.save_label().map(|l| (l.text, l.warning))
}

fn plain(text: &str) -> Option<(String, bool)> {
    Some((text.to_string(), false))
}

fn warning(text: &str) -> Option<(String, bool)> {
    Some((text.to_string(), true))
}

/// "Saved HH:MM" for a time of day, the way the clock gives it.
fn saved_text(text: &str) -> bool {
    let Some(time) = text.strip_prefix("Saved ") else {
        return false;
    };
    let b = time.as_bytes();
    time.len() == 5 && b[2] == b':' && time.chars().filter(char::is_ascii_digit).count() == 4
}

#[test]
fn a_new_document_says_nothing_about_saving() {
    let a = app(1);
    assert_eq!(shown(&a), None);
    assert_eq!(a.document_label(), "Untitled");
}

#[test]
fn an_edit_says_unsaved_changes_in_the_title_and_the_status_bar() {
    let mut a = app(1);
    dot(&mut a, 2, 2);
    assert_eq!(shown(&a), plain("Unsaved changes"));
    assert!(a.document_label().ends_with(" *"));
}

#[test]
fn saving_is_shown_while_it_is_under_way() {
    let mut a = app(1);
    dot(&mut a, 2, 2);
    a.save_started();
    assert_eq!(shown(&a), plain("Saving..."));
}

#[test]
fn a_save_says_saved_with_the_time_only_once_the_file_is_in_place() {
    let dir = temp_dir("saved");
    let path = dir.join("art.piks");
    let mut a = app(1);
    dot(&mut a, 2, 2);
    assert!(a.save_to(Window::detached(), path.clone()), "saved");
    assert!(fs::metadata(&path).is_ok(), "the file is there");
    let (text, warn) = shown(&a).expect("a state");
    assert!(saved_text(&text), "{text}");
    assert!(!warn);
    assert_eq!(a.document_label(), "art.piks", "the title has no mark");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_edit_after_a_save_says_unsaved_changes_again() {
    let dir = temp_dir("again");
    let mut a = app(1);
    assert!(a.save_to(Window::detached(), dir.join("art.piks")));
    dot(&mut a, 3, 3);
    assert_eq!(shown(&a), plain("Unsaved changes"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_save_says_why_until_a_save_works() {
    let dir = temp_dir("failed");
    let target = dir.join("in the way.piks");
    fs::create_dir_all(&target).expect("a folder at the target's name");
    let mut a = app(1);
    dot(&mut a, 2, 2);
    assert!(!a.save_to(Window::detached(), target), "not saved");
    let (text, warn) = shown(&a).expect("a state");
    assert!(text.starts_with("Save failed: "), "{text}");
    assert!(warn, "a failure is a warning");
    assert!(
        a.document_label().ends_with(" *"),
        "the work is still unsaved"
    );
    assert!(a.save_to(Window::detached(), dir.join("art.piks")));
    assert!(saved_text(&shown(&a).expect("a state").0));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_blocked_save_says_what_to_do_and_a_save_clears_it() {
    let dir = temp_dir("blocked");
    let mut a = app(1);
    dot(&mut a, 2, 2);
    a.save_blocked("close the Export dialog first (Esc)".to_string());
    assert_eq!(
        shown(&a),
        warning("Not saved: close the Export dialog first (Esc)")
    );
    assert!(a.save_to(Window::detached(), dir.join("art.piks")));
    assert!(saved_text(&shown(&a).expect("a state").0));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_save_given_up_before_writing_goes_back_to_unsaved() {
    let mut a = app(1);
    dot(&mut a, 2, 2);
    a.save_started();
    a.save_cancelled();
    assert_eq!(shown(&a), plain("Unsaved changes"));
    a.save_failed("the disk is full".to_string());
    a.save_cancelled();
    assert_eq!(
        shown(&a),
        warning("Save failed: the disk is full"),
        "giving up a later save does not hide an earlier failure"
    );
}

#[test]
fn opening_another_document_forgets_when_the_last_one_was_saved() {
    let dir = temp_dir("forget");
    let mut a = app(1);
    assert!(a.save_to(Window::detached(), dir.join("art.piks")));
    let doc = pikslar_core::Document::new(8, 8, pikslar_core::ColorMode::Rgba).expect("doc");
    a.replace_document(doc, None);
    assert_eq!(shown(&a), None);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_state_is_drawn_beside_a_message_and_named_for_scripts() {
    let mut a = app(1);
    a.ui.record_names();
    dot(&mut a, 2, 2);
    a.notify("Copied".to_string());
    draw_whole(&mut a);
    draw_whole(&mut a);
    let layout = a.ui.layout().expect("names kept");
    let rect = |name: &str| {
        layout.lines().find_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let n = |i: usize| f.get(i).and_then(|v| v.parse::<i32>().ok());
            (f.get(5) == Some(&name)).then(|| (n(1).unwrap_or(0), n(3).unwrap_or(0)))
        })
    };
    let (save_x, _) = rect("Save Unsaved changes").expect("the state is drawn");
    let (status_x, status_w) = rect("Status Copied").expect("the message is drawn");
    assert!(status_x + status_w <= save_x, "the two never share pixels");
}

#[test]
fn a_save_that_cannot_put_moved_pixels_down_says_it_was_not_saved() {
    use pikslar_core::{FloatingCmd, Mask, SelectOp, SelectionCmd};
    let mut a = app(1);
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
    assert!(!a.save(Window::detached(), false), "not saved");
    let (text, warn) = shown(&a).expect("a state");
    assert_eq!(text, format!("Not saved: {PIXELS_NOT_PLACED}"));
    assert!(warn);
}

#[test]
fn moved_pixels_that_cannot_be_put_down_stay_said_while_a_dialog_comes_and_goes() {
    let mut a = app(1);
    a.run(pikslar_core::SelectionCmd::Select {
        shape: Box::new(pikslar_core::Mask::rect(0, 0, 4, 4)),
        op: pikslar_core::SelectOp::Replace,
    });
    a.run(pikslar_core::FloatingCmd::Paste {
        clip: std::sync::Arc::new(pikslar_core::Clip::from_image(1, 1, vec![0xff00_00ff])),
        x: 1,
        y: 1,
    });
    a.editor.set_memory_check(|_| false);
    assert!(!a.save(Window::detached(), false));
    let pixels = warning(&format!("Not saved: {PIXELS_NOT_PLACED}"));
    a.open_new_dialog();
    a.recheck_save_blocked();
    assert_eq!(shown(&a), pixels, "a dialog does not take it down");
    a.new_dialog = None;
    a.recheck_save_blocked();
    assert_eq!(shown(&a), pixels, "nor its going");
}

#[test]
fn a_failed_save_is_said_on_the_status_bar_before_its_message_box_opens() {
    let mut a = app(1);
    a.save_started();
    assert_eq!(shown(&a), plain("Saving..."));
    let io = std::io::Error::from_raw_os_error(5);
    let error = crate::files::FileError::from(pikslar_format::FormatError::from(io));
    a.save_ended_failed(std::path::Path::new("art.piks"), error);
    let (text, is_warning) = shown(&a).expect("said");
    assert!(text.starts_with("Save failed: ") && is_warning, "{text}");
    assert!(!text.contains("Saving"), "{text}");
    // Not before a frame saying so has been drawn.
    a.report_save_failure(Window::detached());
    assert!(a.save.to_report.is_some(), "the box waits for the frame");
    assert_eq!(
        a.report_wait(),
        Some(std::time::Duration::ZERO),
        "a frame comes at once"
    );
    a.save_failure_drawn();
    a.report_save_failure(Window::detached());
    assert!(a.save.to_report.is_none() && a.report_wait().is_none());
    assert!(
        !a.last_save_worked(),
        "a failed save is never taken for one that worked"
    );
}
