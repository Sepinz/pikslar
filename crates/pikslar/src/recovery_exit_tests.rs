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
