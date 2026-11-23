//! A backup left by an interrupted save: offered when its project is opened or found missing,
//! and never replaced by a later save until the person has decided about it.

use super::*;
use crate::test_app::*;
use pikslar_format::publish::backup_path;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-orphan-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

/// A project saved at `art`, with a dot, and the document's bytes.
fn saved(dir: &Path) -> (PathBuf, Vec<u8>) {
    let art = dir.join("art.piks");
    let mut a = app(1);
    dot(&mut a, 3, 3);
    assert!(a.save_to(Window::detached(), art.clone()));
    let bytes = fs::read(&art).expect("saved");
    (art, bytes)
}

#[test]
fn a_kill_between_the_renames_leaves_only_the_backup_and_opening_offers_it() {
    static ASKED: AtomicUsize = AtomicUsize::new(0);
    let dir = temp_dir("only-backup");
    let (art, bytes) = saved(&dir);
    // ReplaceFileW moved the original aside and the process died before the new file moved in.
    fs::rename(&art, backup_path(&art)).expect("only the backup");
    let mut a = app(1);
    a.ask_backup = |_, name| {
        assert!(name.ends_with(".pikslar-bak"), "{name}");
        ASKED.fetch_add(1, Ordering::Relaxed);
        Some(true)
    };
    a.open_now(Window::detached(), art.clone());
    assert_eq!(ASKED.load(Ordering::Relaxed), 1, "offered once");
    let shown = files::checked_bytes(a.editor.doc()).expect("encodes");
    assert_eq!(shown, bytes, "the backup's work is open");
    assert!(a.path.is_none(), "saved under a name of its own");
    assert_eq!(fs::read(backup_path(&art)).expect("kept"), bytes);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_save_never_replaces_a_backup_nobody_decided_about() {
    let dir = temp_dir("refused");
    let (art, _) = saved(&dir);
    fs::write(backup_path(&art), b"the only good copy").expect("a backup");
    let mut a = app(1);
    a.ask_backup = |_, _| None;
    a.open_now(Window::detached(), art.clone());
    dot(&mut a, 5, 5);
    assert!(!a.save(Window::detached(), false), "refused");
    let why = a.save.failed.clone().unwrap_or_default();
    assert!(why.contains(".pikslar-bak"), "the message names it: {why}");
    assert_eq!(
        fs::read(backup_path(&art)).expect("kept"),
        b"the only good copy"
    );
    a.ask_backup = |_, _| Some(false);
    a.offer_orphan_backup(Window::detached(), &art);
    assert!(
        a.save(Window::detached(), false),
        "ignored: the save goes ahead"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_recent_file_with_a_backup_offers_it() {
    static ASKED: AtomicUsize = AtomicUsize::new(0);
    let dir = temp_dir("recent");
    let (art, _) = saved(&dir);
    fs::rename(&art, backup_path(&art)).expect("only the backup");
    let mut a = app(1);
    a.ask_backup = |_, _| {
        ASKED.fetch_add(1, Ordering::Relaxed);
        Some(false)
    };
    a.begin_open(
        Window::detached(),
        art.clone(),
        crate::loading::Then::ForgetIfGone,
    );
    a.wait_for_open(Window::detached());
    assert_eq!(ASKED.load(Ordering::Relaxed), 1);
    let _ = fs::remove_dir_all(&dir);
}

/// An app on `art` (saved once) with a backup nobody decided about, and an edit since.
fn app_with_undecided_backup(dir: &Path) -> (App, PathBuf) {
    let (art, _) = saved(dir);
    fs::write(backup_path(&art), b"the only good copy").expect("a backup");
    let mut a = app(1);
    a.ask_backup = |_, _| None;
    a.open_now(Window::detached(), art.clone());
    dot(&mut a, 5, 5);
    (a, art)
}

#[test]
fn save_over_an_undecided_backup_asks_there_and_then_and_never_says_to_reopen() {
    let dir = temp_dir("in-place-no");
    let (mut a, art) = app_with_undecided_backup(&dir);
    a.ask_replace_backup = |_, _| Some(false);
    assert!(!a.save(Window::detached(), false), "kept, so not saved");
    let why = a.save.failed.clone().unwrap_or_default();
    assert!(!why.contains("Open the project again"), "{why}");
    assert!(why.contains(".pikslar-bak"), "{why}");
    assert_eq!(
        fs::read(backup_path(&art)).expect("kept"),
        b"the only good copy"
    );
    assert!(a.is_modified(), "the work is still unsaved, and still here");
    a.ask_replace_backup = |_, _| Some(true);
    assert!(a.save(Window::detached(), false), "replace it, then save");
    assert!(!a.is_modified());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn nobody_answering_the_backup_question_keeps_the_backup_and_the_work() {
    let dir = temp_dir("in-place-none");
    let (mut a, art) = app_with_undecided_backup(&dir);
    a.ask_replace_backup = |_, _| None;
    assert!(!a.save(Window::detached(), false));
    assert_eq!(
        fs::read(backup_path(&art)).expect("kept"),
        b"the only good copy"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_backup_a_later_failed_save_leaves_as_the_only_copy_is_not_replaceable_again() {
    use pikslar_format::publish_back::{Left, NotKept};
    let dir = temp_dir("backup-at");
    let (art, _) = saved(&dir);
    let mut a = app(1);
    a.path = Some(art.clone());
    let backup = backup_path(&art);
    a.backup_known_old(backup.clone());
    let kept = NotKept::new("injected".to_string(), Left::BackupAt(backup.clone()));
    let io = std::io::Error::new(std::io::ErrorKind::InvalidData, kept);
    let error = files::FileError::from(pikslar_format::FormatError::from(io));
    a.save_ended_failed(&art, error);
    assert!(!a.backup_known(&backup), "it may hold the only good copy");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn known_backups_are_compared_ignoring_case() {
    let mut a = app(1);
    a.backup_known_old(PathBuf::from(r"C:\Work\Art.piks.pikslar-bak"));
    assert!(a.backup_known(Path::new(r"c:\work\ART.piks.pikslar-bak")));
    assert!(!a.backup_known(Path::new(r"c:\work\other.piks.pikslar-bak")));
}
