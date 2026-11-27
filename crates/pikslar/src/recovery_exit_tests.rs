//! A normal exit with a recovery write still under way (L5): the work was dealt with, so the
//! copy that write may still leave is never offered, and the next launch clears it.

use super::*;
use pikslar_core::ColorMode;
use std::sync::mpsc;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-exit-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn doc() -> Document {
    Document::new(8, 8, ColorMode::Palette).expect("valid")
}

#[test]
fn a_copy_still_being_written_at_exit_is_never_offered_and_goes_next_launch() {
    let dir = temp_dir("late");
    let mut first = Recovery::in_dir(&dir);
    let origin = PathBuf::from("C:/art/hero.piks");
    first
        .write_now(&doc(), None, Origin::file(&origin))
        .expect("write");
    let copy = first.copy_files().expect("recovery is on").project;
    let (_still_stuck, gate) = mpsc::channel();
    first.hold_writer(gate);
    drop(first);
    // The stuck write ends after the exit had already dealt with the work.
    fs::write(&copy, b"written late").expect("late copy");
    fs::write(copy.with_extension("origin"), "C:/art/hero.piks\n\n\n").expect("late sidecar");
    let second = Recovery::in_dir(&dir);
    assert_eq!(second.find_orphan(), None, "dealt with before the exit");
    second.sweep();
    assert!(!copy.exists(), "the next launch clears it");
    assert!(!copy.with_extension("origin").exists());
    drop(second);
    let left: Vec<_> = fs::read_dir(&dir).expect("dir").flatten().collect();
    assert!(left.is_empty(), "nothing is left: {left:?}");
    fs::remove_dir_all(&dir).expect("clean up");
}

/// Review finding C3 (RC3): a clear left waiting for a write, then a new write of changes made
/// since the save, started in the same tick the old one ended. The waiting clear must not take
/// the new copy with it.
#[test]
fn a_clear_waiting_on_an_old_write_never_removes_a_newer_copy() {
    let dir = temp_dir("clear-race");
    let mut session = Recovery::in_dir(&dir);
    let origin = PathBuf::from("C:/art/hero.piks");
    let (go, gate) = mpsc::channel();
    session.hold_writer(gate);
    assert!(!session.clear(), "the write under way holds the clear back");
    assert!(session.clear_waiting());
    go.send(()).expect("the old write ends");
    while !session.writer.as_ref().is_some_and(JoinHandle::is_finished) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let newer = Document::new(9, 8, ColorMode::Palette).expect("valid");
    assert!(session.write_in_background(newer, None, Origin::file(&origin)));
    while !session.writer.as_ref().is_some_and(JoinHandle::is_finished) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(session.take_error().is_none(), "the newer write worked");
    let copy = session.copy_files().expect("recovery is on").project;
    let kept = crate::files::open_recovery(&copy).expect("the newer copy is kept");
    assert_eq!(kept.doc.width, 9, "it is the newer work");
    session.set_keep(false);
    drop(session);
    let _ = fs::remove_dir_all(&dir);
}
