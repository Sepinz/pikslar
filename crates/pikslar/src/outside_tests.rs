//! A project another program changed: Save asks before writing over it, says nothing when
//! only the times changed, and the saving thread refuses a change made while it worked.

use super::*;
use crate::files;
use crate::test_app::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

const OTHER: &[u8] = b"written by another program";

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-outside-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

/// An app whose file is `art`, saved once, with an edit since.
fn saved_app(dir: &Path) -> (App, PathBuf) {
    let art = dir.join("art.piks");
    let mut a = app(1);
    a.ask_changed = |_, _| panic!("nothing changed, so nothing is asked");
    assert!(a.save_to(Window::detached(), art.clone()), "the first save");
    dot(&mut a, 2, 2);
    (a, art)
}

fn save(a: &mut App) -> bool {
    a.save(Window::detached(), false)
}

#[test]
fn an_unchanged_file_is_saved_over_without_a_question() {
    let dir = temp_dir("unchanged");
    let (mut a, art) = saved_app(&dir);
    assert!(save(&mut a));
    assert!(files::open(&art).is_ok());
    dot(&mut a, 3, 3);
    assert!(
        save(&mut a),
        "the stamp of the save just made is remembered"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_changed_file_asks_and_cancel_keeps_the_other_programs_bytes() {
    static ASKED: AtomicUsize = AtomicUsize::new(0);
    let dir = temp_dir("cancel");
    let (mut a, art) = saved_app(&dir);
    fs::write(&art, OTHER).expect("changed outside");
    a.ask_changed = |_, name| {
        assert_eq!(name, "art.piks");
        ASKED.fetch_add(1, Ordering::Relaxed);
        ChangedChoice::Cancel
    };
    assert!(!save(&mut a), "nothing was saved");
    assert_eq!(ASKED.load(Ordering::Relaxed), 1);
    assert_eq!(fs::read(&art).expect("read"), OTHER);
    assert!(a.is_modified());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn overwrite_saves_and_the_next_save_does_not_ask_again() {
    static ASKED: AtomicUsize = AtomicUsize::new(0);
    let dir = temp_dir("overwrite");
    let (mut a, art) = saved_app(&dir);
    fs::write(&art, OTHER).expect("changed outside");
    a.ask_changed = |_, _| {
        ASKED.fetch_add(1, Ordering::Relaxed);
        ChangedChoice::Overwrite
    };
    assert!(save(&mut a));
    assert!(files::open(&art).is_ok(), "this version is in place");
    dot(&mut a, 4, 4);
    assert!(save(&mut a));
    assert_eq!(ASKED.load(Ordering::Relaxed), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn save_as_goes_to_the_dialog_and_leaves_the_changed_file_alone() {
    let dir = temp_dir("save-as");
    let (mut a, art) = saved_app(&dir);
    fs::write(&art, OTHER).expect("changed outside");
    a.ask_changed = |_, _| ChangedChoice::SaveAs;
    // A detached window's dialog is cancelled, so nothing is written anywhere.
    assert!(!save(&mut a));
    assert_eq!(fs::read(&art).expect("read"), OTHER);
    assert_eq!(fs::read_dir(&dir).expect("dir").count(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_file_only_touched_is_saved_over_without_a_question() {
    let dir = temp_dir("touched");
    let (mut a, art) = saved_app(&dir);
    let later = SystemTime::now() + Duration::from_secs(60);
    let file = fs::OpenOptions::new().write(true).open(&art).expect("open");
    file.set_modified(later).expect("touched");
    drop(file);
    assert!(save(&mut a), "the same bytes under a new time");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_change_made_while_saving_stops_the_replace_and_the_next_save_asks() {
    static ASKED: AtomicUsize = AtomicUsize::new(0);
    let dir = temp_dir("during");
    let (mut a, art) = saved_app(&dir);
    a.save_delay = Some(Duration::from_millis(600));
    assert_eq!(
        a.start_save(Window::detached(), false),
        crate::save_run::Started::Now
    );
    fs::write(&art, OTHER).expect("changed while saving");
    finish_saves(&mut a);
    let Some(why) = &a.save.failed else {
        panic!("the save failed: {:?}", a.save);
    };
    assert!(
        why.contains("changed by another program while saving"),
        "{why}"
    );
    assert_eq!(fs::read(&art).expect("read"), OTHER, "the change is kept");
    assert!(a.is_modified());
    a.save_delay = None;
    a.ask_changed = |_, _| {
        ASKED.fetch_add(1, Ordering::Relaxed);
        ChangedChoice::Cancel
    };
    assert!(!save(&mut a));
    assert_eq!(ASKED.load(Ordering::Relaxed), 1, "the next save asks");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_file_opened_here_is_remembered_and_a_change_after_asks() {
    static ASKED: AtomicUsize = AtomicUsize::new(0);
    let dir = temp_dir("opened");
    let art = dir.join("art.piks");
    files::save(
        &art,
        &pikslar_core::Document::new(4, 4, pikslar_core::ColorMode::Rgba).expect("doc"),
    )
    .expect("written");
    let mut a = app(1);
    a.open_now(Window::detached(), art.clone());
    assert!(a.seen.is_some(), "the file's stamp and print are kept");
    dot(&mut a, 1, 1);
    fs::write(&art, OTHER).expect("changed outside");
    a.ask_changed = |_, _| {
        ASKED.fetch_add(1, Ordering::Relaxed);
        ChangedChoice::Cancel
    };
    assert!(!save(&mut a));
    assert_eq!(ASKED.load(Ordering::Relaxed), 1);
    assert_eq!(fs::read(&art).expect("read"), OTHER);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_save_whose_file_could_not_be_put_back_is_failed_and_not_taken_for_another_program() {
    use pikslar_format::publish_back::{Left, NotKept};
    let dir = temp_dir("not-put-back");
    let (mut a, art) = saved_app(&dir);
    // What a failed read back with a failed restore leaves: other bytes, the original kept.
    fs::write(&art, OTHER).expect("unverified bytes");
    let left = Left::BackupAt(pikslar_format::publish::backup_path(&art));
    let kept = NotKept::new("injected".to_string(), left);
    let io = std::io::Error::new(std::io::ErrorKind::InvalidData, kept);
    let error = files::FileError::from(pikslar_format::FormatError::from(io));
    a.save_ended_failed(&art, error);
    assert!(a.save.failed.is_some());
    a.ask_changed = |_, _| panic!("this save changed the file, not another program");
    assert!(save(&mut a), "the next save goes ahead");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_save_whose_stamp_was_not_held_still_is_compared_by_its_bytes() {
    let dir = temp_dir("no-stamp");
    let art = dir.join("art.piks");
    fs::write(&art, b"saved").expect("saved");
    let print = pikslar_rewind::hash128(b"saved");
    let seen = Seen::after_save(None, print);
    assert!(
        still_same(&art, &seen).is_some(),
        "its own bytes are still there"
    );
    fs::write(&art, OTHER).expect("another program");
    assert!(
        still_same(&art, &seen).is_none(),
        "another program's are seen"
    );
    let stamp = pikslar_format::publish::Stamp {
        modified: SystemTime::UNIX_EPOCH + Duration::from_secs(5),
        len: 3,
    };
    let seen = Seen::after_save(Some(stamp), print);
    assert_eq!(
        seen.stamp,
        (5_000_000_000, 3),
        "the stamp it read back with"
    );
    let _ = fs::remove_dir_all(&dir);
}
