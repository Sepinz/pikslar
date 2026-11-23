//! Project saves at full path lengths around and past Windows' old 260 character limit
//! (`docs/b1-unattended-report.md`): a save over a project, a first save (as Save As makes),
//! Unicode names, a network share where one can be reached, two saves at once, a save stopped
//! part way, and a failure that must leave the original and its backup.

use super::*;
use crate::files;
use pikslar_format::publish::{backup_path, write_atomic_with, Published};
use std::fs;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;

fn doc(side: usize) -> pikslar_core::Document {
    pikslar_core::Document::new(side, side, pikslar_core::ColorMode::Rgba).expect("doc")
}

fn chars(path: &Path) -> usize {
    path.as_os_str().to_string_lossy().encode_utf16().count()
}

/// A fresh folder for one test.
fn root(label: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pikslar-lp-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).expect("a folder");
    d
}

/// A project path exactly `length` characters long under `root`, ending in `name`, made of
/// folders of at most 200 characters, which are made.
fn path_of_length(root: &Path, length: usize, name: &str) -> PathBuf {
    let mut dir = root.to_path_buf();
    loop {
        let left = length - chars(&dir) - 1 - name.encode_utf16().count();
        if left == 0 {
            break;
        }
        // A folder adds its name and a separator; never leave a gap of one, which no folder fits.
        let take = match left {
            0..=201 => left - 1,
            202 => 100,
            _ => 200,
        };
        assert!(take > 0, "no folder fits a gap of one character");
        dir = dir.join("d".repeat(take));
    }
    fs::create_dir_all(&dir).expect("the folders");
    let path = dir.join(name);
    assert_eq!(chars(&path), length, "{}", path.display());
    path
}

/// True if nothing but `keep` is in the project's folder.
fn only(path: &Path, keep: &[&str]) -> bool {
    let folder = path.parent().expect("a folder");
    fs::read_dir(folder).expect("listed").flatten().all(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        keep.contains(&name.as_str())
    })
}

#[test]
fn projects_save_at_and_past_the_old_limit() {
    let base = root("bound");
    for length in [237, 238, 239, 259, 260, 261, 300, 1_000, 4_000] {
        let start = chars(&base) + 1 + 8 + 2;
        if length < start {
            continue;
        }
        let case = base.join(format!("{length}"));
        let art = path_of_length(&case, length, "art.piks");
        // A first save, as Save As to a new name makes: nothing is replaced.
        files::save_traced(&art, &doc(4)).unwrap_or_else(|e| panic!("{length}: {}", e.error));
        // A save over the project, as Ctrl+S makes, checking it is the file this session saw.
        let seen = crate::outside::Seen::of(&art, None);
        let saved = files::save_checked(&art, &doc(8), seen);
        saved.unwrap_or_else(|e| panic!("{length}: {}", e.error));
        let opened = files::open(&art).unwrap_or_else(|e| panic!("{length}: {e}"));
        assert_eq!(opened.doc.width, 8, "{length}");
        assert!(
            only(&art, &["art.piks"]),
            "{length}: no temporary, backup or lock file"
        );
    }
    let _ = fs::remove_dir_all(base);
}

#[test]
fn unicode_projects_save_past_the_old_limit() {
    let base = root("unicode");
    for name in ["Ærø プロジェクト.piks", "art 🎨 ✏️.piks"] {
        let art = path_of_length(&base.join("u"), 300, name);
        files::save_traced(&art, &doc(4)).unwrap_or_else(|e| panic!("{name}: {}", e.error));
        files::save_traced(&art, &doc(8)).unwrap_or_else(|e| panic!("{name}: {}", e.error));
        assert_eq!(files::open(&art).expect("opens").doc.width, 8);
        assert!(only(&art, &[name]), "{name}");
    }
    let _ = fs::remove_dir_all(base);
}

#[test]
fn saving_another_project_as_a_long_path_replaces_it() {
    let base = root("saveas");
    let other = path_of_length(&base, 280, "other.piks");
    fs::write(&other, b"not a project yet").expect("a file there");
    files::save_traced(&other, &doc(6)).expect("saved over it");
    assert_eq!(files::open(&other).expect("opens").doc.width, 6);
    assert!(only(&other, &["other.piks"]));
    let _ = fs::remove_dir_all(base);
}

#[test]
fn a_long_path_on_a_network_share_saves_where_one_can_be_reached() {
    let local = root("share");
    let text = local.to_string_lossy().into_owned();
    // The drive's administrative share, as another computer would reach this folder.
    let Some((drive, rest)) = text.split_once(":\\") else {
        return;
    };
    let share = PathBuf::from(format!(r"\\localhost\{drive}$\{rest}"));
    if fs::metadata(&share).is_err() {
        eprintln!("no network share can be reached here; the share case was not run");
        let _ = fs::remove_dir_all(local);
        return;
    }
    let art = path_of_length(&share, 300, "art.piks");
    files::save_traced(&art, &doc(4)).expect("first save on the share");
    files::save_traced(&art, &doc(8)).expect("save over it on the share");
    assert_eq!(files::open(&art).expect("opens").doc.width, 8);
    assert!(only(&art, &["art.piks"]));
    let _ = fs::remove_dir_all(local);
}

#[test]
fn two_saves_at_once_of_a_long_path_are_one_after_the_other() {
    let base = root("together");
    let art = path_of_length(&base, 300, "art.piks");
    files::save_traced(&art, &doc(4)).expect("first");
    let saves: Vec<_> = [5usize, 7]
        .into_iter()
        .map(|side| {
            let art = art.clone();
            std::thread::spawn(move || {
                (0..5)
                    .map(|_| files::save_traced(&art, &doc(side)).map_err(|e| e.error.to_string()))
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    for save in saves {
        for result in save.join().expect("a saving thread") {
            result.expect("each save waits for the other, then saves");
        }
    }
    let width = files::open(&art).expect("whole").doc.width;
    assert!(width == 5 || width == 7, "{width}");
    assert!(only(&art, &["art.piks"]));
    let _ = fs::remove_dir_all(base);
}

/// The system's publish, stopped at `at` as if the program ended there.
struct StopsAt(Published);

impl Publish for StopsAt {
    fn replace(&mut self, target: &Path, temp: &Path, backup: &Path) -> io::Result<()> {
        SystemPublish::default().replace(target, temp, backup)
    }

    fn place_new(&mut self, temp: &Path, target: &Path) -> io::Result<()> {
        SystemPublish::default().place_new(temp, target)
    }

    fn reached(&mut self, step: Published, _target: &Path) -> io::Result<()> {
        match step == self.0 {
            true => Err(io::Error::other("stopped")),
            false => Ok(()),
        }
    }
}

#[test]
fn a_long_path_save_stopped_part_way_keeps_the_original_at_the_backup() {
    for step in [Published::Replaced, Published::ReadBack] {
        let base = root(&format!("stop-{step:?}"));
        let art = path_of_length(&base, 300, "art.piks");
        fs::write(&art, b"old").expect("old");
        let stopped = write_atomic_with(&art, b"new", &mut |_, _| Ok(()), &mut StopsAt(step));
        assert!(stopped.is_err(), "{step:?}");
        assert_eq!(fs::read(&art).expect("the new file"), b"new", "{step:?}");
        assert_eq!(fs::read(backup_path(&art)).expect("backup"), b"old");
        // The next save replaces the stale backup and leaves none.
        let next = &mut SystemPublish::default();
        write_atomic_with(&art, b"newer", &mut |_, _| Ok(()), next).expect("next");
        assert_eq!(fs::read(&art).expect("read"), b"newer");
        assert!(only(&art, &["art.piks"]), "{step:?}");
        let _ = fs::remove_dir_all(base);
    }
}

#[test]
fn a_failed_long_path_save_leaves_the_original_and_an_older_backup() {
    let base = root("fail");
    let art = path_of_length(&base, 300, "art.piks");
    files::save_traced(&art, &doc(4)).expect("first");
    let original = fs::read(&art).expect("read");
    let backup = backup_path(&art);
    fs::write(&backup, b"an older backup").expect("a backup already there");
    // Another program holds the project without letting it be replaced.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&art)
        .expect("held");
    let failed = files::save_traced(&art, &doc(8)).expect_err("cannot replace it");
    drop(held);
    let said = failed.error.to_string();
    assert!(!said.contains("cannot find the path"), "{said}");
    assert_eq!(
        fs::read(&art).expect("read"),
        original,
        "the original is whole"
    );
    assert_eq!(fs::read(&backup).expect("read"), b"an older backup");
    let _ = fs::remove_dir_all(base);
}

#[test]
fn a_name_with_no_room_is_refused_before_anything_is_written() {
    let base = root("noroom");
    let name = format!("{}.piks", "n".repeat(240));
    let art = base.join(&name);
    fs::write(&art, b"original").expect("an original with a long name");
    let failed = files::save_traced(&art, &doc(4)).expect_err("refused");
    let said = failed.error.to_string();
    assert!(
        said.contains("Nothing was changed") && said.contains("Save As"),
        "{said}"
    );
    assert_eq!(fs::read(&art).expect("read"), b"original");
    assert!(only(&art, &[name.as_str()]));
    let _ = fs::remove_dir_all(base);
}
