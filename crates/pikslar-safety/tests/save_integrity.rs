//! A save that fails at any step leaves the previous file exactly as it was.

use pikslar_format::atomic::{temp_path, write_atomic, write_atomic_checked, SaveStep};
use pikslar_format::publish::{backup_path, write_atomic_with, Publish, Published};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const OLD: &[u8] = b"the previous contents of the file";
const NEW: &[u8] = b"a replacement, long enough to be interrupted halfway through";

struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let dir = std::env::temp_dir().join(format!("pikslar-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("temp dir");
        Folder(dir)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        // Tests clear read-only flags themselves. Cleanup must never hide a failing assertion.
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fail_at(step: SaveStep) -> impl FnMut(SaveStep, &Path) -> io::Result<()> {
    move |now, _| {
        if now == step {
            Err(io::Error::other("injected failure"))
        } else {
            Ok(())
        }
    }
}

#[test]
fn a_failure_at_any_step_leaves_the_old_file_alone() {
    let folder = Folder::new("steps");
    for step in SaveStep::ALL {
        let path = folder.file("art.piks");
        fs::write(&path, OLD).expect("write");
        assert!(write_atomic_checked(&path, NEW, &mut fail_at(step)).is_err());
        assert_eq!(fs::read(&path).expect("read"), OLD, "{step:?}");
        assert!(!temp_path(&path).exists(), "{step:?} left a temporary file");
    }
}

#[test]
fn a_failed_first_save_creates_nothing() {
    let folder = Folder::new("first");
    for step in SaveStep::ALL {
        let path = folder.file("new.piks");
        assert!(write_atomic_checked(&path, NEW, &mut fail_at(step)).is_err());
        assert!(!path.exists(), "{step:?}");
        assert!(!temp_path(&path).exists(), "{step:?}");
    }
}

#[test]
fn every_step_is_visited_in_order_on_a_good_save() {
    let folder = Folder::new("order");
    let path = folder.file("art.piks");
    let mut seen = Vec::new();
    write_atomic_checked(&path, NEW, &mut |step, _| {
        seen.push(step);
        Ok(())
    })
    .expect("saves");
    assert_eq!(seen, SaveStep::ALL);
    assert_eq!(fs::read(&path).expect("read"), NEW);
}

#[test]
fn bytes_that_do_not_read_back_are_never_used() {
    let folder = Folder::new("readback");
    for damage in [
        &b"short"[..],
        &b"a replacement, long enough to be INTERRUPTED halfway through"[..],
    ] {
        let path = folder.file("art.piks");
        fs::write(&path, OLD).expect("write");
        let result = write_atomic_checked(&path, NEW, &mut |step, temp| {
            if step == SaveStep::Synced {
                fs::write(temp, damage)?;
            }
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).expect("read"), OLD);
    }
}

#[test]
fn a_read_only_file_is_not_replaced() {
    let folder = Folder::new("readonly");
    let path = folder.file("art.piks");
    fs::write(&path, OLD).expect("write");
    let mut permissions = fs::metadata(&path).expect("metadata").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions.clone()).expect("set read-only");
    let result = write_atomic(&path, NEW);
    let contents = fs::read(&path).expect("read");
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&path, permissions).expect("clear read-only");
    assert!(result.is_err(), "a read-only file was replaced");
    assert_eq!(contents, OLD);
}

/// The last step can fail too. Windows refuses a rename over a file another program holds.
#[cfg(windows)]
#[test]
fn a_file_held_open_by_another_program_is_left_alone() {
    use std::os::windows::fs::OpenOptionsExt;
    let folder = Folder::new("held");
    let path = folder.file("art.piks");
    fs::write(&path, OLD).expect("write");
    let holder = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .expect("held");
    let result = write_atomic(&path, NEW);
    drop(holder);
    assert!(result.is_err(), "a held file was replaced");
    assert_eq!(fs::read(&path).expect("read"), OLD);
    assert!(!temp_path(&path).exists(), "left a temporary file");
}

#[test]
fn a_file_merely_ending_in_tmp_is_left_alone() {
    let folder = Folder::new("tmpname");
    let (path, theirs) = (folder.file("art.piks"), folder.file("art.piks.tmp"));
    fs::write(&theirs, OLD).expect("write");
    write_atomic(&path, NEW).expect("saves");
    assert_eq!(fs::read(&theirs).expect("read"), OLD);
}

#[test]
fn a_leftover_from_a_crash_does_not_stop_the_next_save() {
    let folder = Folder::new("leftover");
    let path = folder.file("art.piks");
    fs::write(temp_path(&path), b"half a save").expect("write");
    write_atomic(&path, NEW).expect("saves");
    assert_eq!(fs::read(&path).expect("read"), NEW);
    assert!(!temp_path(&path).exists());
}

#[test]
fn saving_into_a_missing_folder_is_an_error_not_a_crash() {
    let folder = Folder::new("missing");
    assert!(write_atomic(&folder.file("nope/art.piks"), NEW).is_err());
}

#[test]
fn an_empty_save_works() {
    let folder = Folder::new("empty");
    let path = folder.file("art.piks");
    write_atomic(&path, b"").expect("saves");
    assert_eq!(fs::read(&path).expect("read"), b"");
}

/// Puts the file in place as `ReplaceFileW` with a backup does, by renames, and stops at
/// `stop`: the program's own publish, as far as the files on disk are concerned.
struct BackupAndStop(Option<Published>);

impl Publish for BackupAndStop {
    fn replace(&mut self, target: &Path, temp: &Path, backup: &Path) -> io::Result<()> {
        fs::rename(target, backup)?;
        fs::rename(temp, target)
    }

    fn place_new(&mut self, temp: &Path, target: &Path) -> io::Result<()> {
        fs::rename(temp, target)
    }

    fn reached(&mut self, step: Published, _target: &Path) -> io::Result<()> {
        match self.0 == Some(step) {
            true => Err(io::Error::other("injected failure")),
            false => Ok(()),
        }
    }
}

#[test]
fn with_a_backup_every_step_leaves_the_old_file_or_the_new_one_and_never_neither() {
    let folder = Folder::new("backup-steps");
    let path = folder.file("art.piks");
    let backup = backup_path(&path);
    for step in SaveStep::ALL {
        fs::write(&path, OLD).expect("write");
        let stopped = write_atomic_with(&path, NEW, &mut fail_at(step), &mut BackupAndStop(None));
        assert!(stopped.is_err());
        assert_eq!(fs::read(&path).expect("read"), OLD, "{step:?}");
        assert!(!backup.exists(), "{step:?}: no backup before the replace");
    }
    for step in [Published::Replaced, Published::ReadBack] {
        fs::write(&path, OLD).expect("write");
        let mut publish = BackupAndStop(Some(step));
        assert!(write_atomic_with(&path, NEW, &mut |_, _| Ok(()), &mut publish).is_err());
        assert_eq!(
            fs::read(&path).expect("read"),
            NEW,
            "{step:?}: the new file is whole"
        );
        assert_eq!(
            fs::read(&backup).expect("backup"),
            OLD,
            "{step:?}: the old one too"
        );
    }
    fs::write(&path, OLD).expect("write");
    write_atomic_with(&path, NEW, &mut |_, _| Ok(()), &mut BackupAndStop(None)).expect("saved");
    assert_eq!(fs::read(&path).expect("read"), NEW);
    assert!(
        !backup.exists(),
        "a good save leaves no backup, a stale one included"
    );
    let temps = fs::read_dir(&folder.0)
        .expect("listed")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".pikslar-tmp"))
        .count();
    assert_eq!(temps, 0, "no temporary file of any save is left");
}
