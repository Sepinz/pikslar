//! The extended form, and Windows' own replace and moves at lengths past the old limit.

use super::*;
use crate::replace::{move_new, move_over, replace_file};
use std::ffi::OsString;
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

fn shown(units: &[u16]) -> String {
    let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
    OsString::from_wide(&units[..end])
        .to_string_lossy()
        .into_owned()
}

fn extended_text(path: &str) -> String {
    shown(&extended(Path::new(path)).expect("a path"))
}

#[test]
fn a_drive_path_gets_the_extended_start_and_is_tidied_first() {
    assert_eq!(extended_text(r"C:\a\b.piks"), r"\\?\C:\a\b.piks");
    assert_eq!(extended_text("C:/a/./c/../b.piks"), r"\\?\C:\a\b.piks");
}

#[test]
fn a_share_path_gets_the_extended_unc_start() {
    assert_eq!(
        extended_text(r"\\server\share\art\b.piks"),
        r"\\?\UNC\server\share\art\b.piks"
    );
    assert_eq!(
        extended_text("//server/share/art/b.piks"),
        r"\\?\UNC\server\share\art\b.piks"
    );
}

#[test]
fn an_extended_or_device_path_is_left_as_it_is() {
    assert_eq!(extended_text(r"\\?\C:\a\b.piks"), r"\\?\C:\a\b.piks");
    assert_eq!(
        extended_text(r"\\?\UNC\server\share\b.piks"),
        r"\\?\UNC\server\share\b.piks"
    );
    assert_eq!(extended_text(r"\\.\C:\a\b.piks"), r"\\.\C:\a\b.piks");
}

#[test]
fn a_relative_path_is_made_whole_from_the_current_folder() {
    let here = std::env::current_dir().expect("the current folder");
    let expected = format!(r"\\?\{}", here.join("b.piks").display());
    assert_eq!(extended_text("b.piks"), expected);
}

#[test]
fn a_nul_or_an_empty_path_is_refused() {
    assert!(extended(Path::new("a\0b.piks")).is_err());
    assert!(extended(Path::new("")).is_err());
}

#[test]
fn the_start_is_read_the_same_whichever_slash_is_used() {
    let read = |s: &str| start(&units(s));
    assert_eq!(read(r"C:\x"), Start::Drive);
    assert_eq!(read(r"\\s\x"), Start::Share);
    assert_eq!(read(r"\\?\C:\x"), Start::AsIs);
    assert_eq!(read(r"//?/C:/x"), Start::AsIs);
    assert_eq!(read(r"\x"), Start::Other);
}

/// A folder whose path is about `length` characters long, made through the standard library,
/// which takes long paths itself.
fn deep_folder(label: &str, length: usize) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("pikslar-long-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut dir = root.clone();
    while dir.as_os_str().len() + 60 < length {
        dir = dir.join("a-folder-of-forty-characters-in-length-x");
    }
    fs::create_dir_all(&dir).expect("a deep folder");
    (root, dir)
}

#[test]
fn replace_and_moves_work_past_the_old_limit() {
    for length in [300, 1_000, 4_000] {
        let (root, dir) = deep_folder(&format!("r{length}"), length);
        let target = dir.join("art.piks");
        let fresh = dir.join("art.piks.0123abcd.pikslar-tmp");
        let backup = dir.join("art.piks.pikslar-bak");
        assert!(fresh.as_os_str().len() > 260, "{length}");
        fs::write(&target, "old").expect("written");
        fs::write(&fresh, "new").expect("written");
        replace_file(&target, &fresh, &backup).expect("replaced");
        assert_eq!(fs::read(&target).expect("read"), b"new");
        assert_eq!(fs::read(&backup).expect("read"), b"old");
        assert!(!fresh.exists());
        let other = dir.join("other.piks");
        move_new(&backup, &other).expect("moved");
        assert!(move_new(&target, &other).is_err(), "never replaces");
        move_over(&target, &other).expect("moved over");
        assert_eq!(fs::read(&other).expect("read"), b"new");
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn unicode_names_are_replaced_past_the_old_limit() {
    let (root, dir) = deep_folder("unicode", 300);
    let target = dir.join("Ærø プロジェクト 🎨.piks");
    let fresh = dir.join("Ærø プロジェクト 🎨.piks.0123abcd.pikslar-tmp");
    let backup = dir.join("Ærø プロジェクト 🎨.piks.pikslar-bak");
    fs::write(&target, "old").expect("written");
    fs::write(&fresh, "new").expect("written");
    replace_file(&target, &fresh, &backup).expect("replaced");
    assert_eq!(fs::read(&target).expect("read"), b"new");
    assert_eq!(fs::read(&backup).expect("read"), b"old");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_failed_replace_past_the_old_limit_leaves_the_original() {
    let (root, dir) = deep_folder("fail", 300);
    let (target, backup) = (dir.join("a.piks"), dir.join("a.piks.pikslar-bak"));
    fs::write(&target, "old").expect("written");
    assert!(replace_file(&target, &dir.join("missing.tmp"), &backup).is_err());
    assert_eq!(fs::read(&target).expect("read"), b"old");
    assert!(!backup.exists());
    let _ = fs::remove_dir_all(root);
}
