//! What is refused before a save writes anything, and what is not.

use super::*;
use crate::atomic::{write_atomic, RUN_TEMP_EXTRA};
use crate::publish::{write_atomic_with, Rename};
use std::fs;
use std::path::PathBuf;

fn folder(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-room-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a folder");
    dir
}

fn named(units: usize) -> String {
    format!("{}.piks", "n".repeat(units - 5))
}

#[test]
fn a_name_with_room_for_the_temporary_file_is_allowed() {
    let most = LONGEST_NAME - RUN_TEMP_EXTRA;
    assert!(check_room(Path::new(&named(most)), RUN_TEMP_EXTRA).is_ok());
    assert!(matches!(
        check_room(Path::new(&named(most + 1)), RUN_TEMP_EXTRA),
        Err(FormatError::NameTooLong { most: m }) if m == most
    ));
}

#[test]
fn a_name_is_counted_in_windows_characters() {
    // Each of these takes two UTF-16 units, as Windows counts a name.
    let wide = format!("{}.piks", "🎨".repeat(115));
    assert_eq!(wide.encode_utf16().count(), 235);
    assert!(check_room(Path::new(&wide), RUN_TEMP_EXTRA).is_err());
    let fits = format!("{}.piks", "🎨".repeat(114));
    assert!(check_room(Path::new(&fits), RUN_TEMP_EXTRA).is_ok());
}

#[test]
fn a_whole_path_past_windows_limit_is_refused() {
    let deep: PathBuf = (0..200).map(|_| "f".repeat(200)).collect();
    let path = Path::new(r"C:\").join(deep).join("a.piks");
    assert!(matches!(
        check_room(&path, RUN_TEMP_EXTRA),
        Err(FormatError::PathTooLong)
    ));
}

#[test]
fn a_refused_name_writes_nothing_and_leaves_the_original() {
    let dir = folder("refused");
    let long = dir.join(named(LONGEST_NAME - 5));
    fs::write(&long, b"original").expect("an original with a long name");
    let saved = write_atomic_with(&long, b"new", &mut |_, _| Ok(()), &mut Rename);
    let told = saved.expect_err("refused").to_string();
    assert!(told.contains("Nothing was changed"), "{told}");
    assert!(told.contains("at most 234 characters"), "{told}");
    assert!(write_atomic(&long, b"new").is_err());
    assert_eq!(fs::read(&long).expect("read"), b"original");
    let left: Vec<_> = fs::read_dir(&dir).expect("listed").flatten().collect();
    assert_eq!(left.len(), 1, "no temporary, backup or lock file");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_message_never_says_the_path_was_not_found() {
    let told = FormatError::NameTooLong { most: 234 }.to_string();
    assert!(!told.contains("cannot find"), "{told}");
    assert!(FormatError::PathTooLong.to_string().contains("Save As"));
}
