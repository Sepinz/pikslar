//! The real `ReplaceFileW` and move under a project save: what it keeps, the backup's states if
//! the program stops part way, and a file held by another program.

use super::*;
use crate::files;
use pikslar_format::publish::{backup_path, write_atomic_with, Published};
use std::fs;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const FILE_SHARE_READ: u32 = 1;
const FILE_SHARE_WRITE: u32 = 2;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pikslar-syspub-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).expect("dir");
    d
}

/// True if no save left a temporary file in `d`.
fn no_temps(d: &Path) -> bool {
    fs::read_dir(d)
        .expect("listed")
        .flatten()
        .all(|e| !e.file_name().to_string_lossy().ends_with(".pikslar-tmp"))
}

fn doc() -> pikslar_core::Document {
    pikslar_core::Document::new(4, 4, pikslar_core::ColorMode::Rgba).expect("doc")
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
fn a_save_over_a_project_keeps_its_streams_and_leaves_no_backup() {
    let d = dir("streams");
    let art = d.join("art.piks");
    fs::write(&art, b"old").expect("old");
    let stream = d.join("art.piks:note");
    fs::write(&stream, "kept").expect("a named stream");
    files::save_traced(&art, &doc()).expect("saved");
    assert_eq!(
        fs::read(&stream).expect("the stream"),
        b"kept",
        "ReplaceFileW kept it"
    );
    assert!(files::open(&art).is_ok(), "the project opens");
    assert!(!backup_path(&art).exists() && no_temps(&d));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_first_save_is_moved_into_place_with_no_backup() {
    let d = dir("first");
    let art = d.join("art.piks");
    files::save_traced(&art, &doc()).expect("saved");
    assert!(files::open(&art).is_ok());
    assert!(!backup_path(&art).exists() && no_temps(&d));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn stopped_after_the_replace_the_old_project_is_the_backup_and_the_new_one_is_whole() {
    for step in [Published::Replaced, Published::ReadBack] {
        let d = dir(&format!("{step:?}"));
        let art = d.join("art.piks");
        fs::write(&art, b"old").expect("old");
        let stopped = write_atomic_with(&art, b"new", &mut |_, _| Ok(()), &mut StopsAt(step));
        assert!(stopped.is_err(), "{step:?}");
        assert_eq!(fs::read(&art).expect("the new file"), b"new", "{step:?}");
        assert_eq!(
            fs::read(backup_path(&art)).expect("backup"),
            b"old",
            "{step:?}"
        );
        // The next save replaces the stale backup, and leaves none.
        write_atomic_with(
            &art,
            b"newer",
            &mut |_, _| Ok(()),
            &mut SystemPublish::default(),
        )
        .expect("next");
        assert_eq!(fs::read(&art).expect("read"), b"newer");
        assert!(!backup_path(&art).exists(), "{step:?}");
        let _ = fs::remove_dir_all(&d);
    }
}

#[test]
fn a_project_held_open_without_delete_sharing_is_left_whole_and_the_reason_is_said() {
    let d = dir("held");
    let art = d.join("art.piks");
    fs::write(&art, b"old").expect("old");
    let holder = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&art)
        .expect("held, without delete sharing");
    let start = Instant::now();
    let failed = files::save_traced(&art, &doc()).expect_err("cannot replace it");
    let took = start.elapsed();
    drop(holder);
    assert!(
        took >= Duration::from_millis(1800),
        "tried again for about 2 s: {took:?}"
    );
    assert!(took < Duration::from_secs(5), "{took:?}");
    let said = failed.error.to_string();
    assert!(said.contains("open in another program"), "{said}");
    assert_eq!(failed.phase, "publish");
    assert_eq!(
        crate::diag::save_failure(&failed.error).1,
        Some(32),
        "the code is logged"
    );
    assert_eq!(
        fs::read(&art).expect("read"),
        b"old",
        "the old project is whole"
    );
    assert!(no_temps(&d) && !backup_path(&art).exists());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_read_only_project_is_refused_with_a_message() {
    let d = dir("read-only");
    let art = d.join("art.piks");
    fs::write(&art, b"old").expect("old");
    let mut perms = fs::metadata(&art).expect("meta").permissions();
    perms.set_readonly(true);
    fs::set_permissions(&art, perms.clone()).expect("read-only");
    let failed = files::save_traced(&art, &doc()).expect_err("refused");
    assert_eq!(failed.phase, "read-only");
    assert!(failed.error.to_string().contains("read-only"));
    assert_eq!(fs::read(&art).expect("read"), b"old");
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&art, perms).expect("writable");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_held_project_keeps_other_writers_out_and_can_still_be_replaced() {
    let d = dir("hold");
    let art = d.join("art.piks");
    let (fresh, backup) = (d.join("fresh"), d.join("bak"));
    fs::write(&art, b"old").expect("old");
    fs::write(&fresh, b"new").expect("new");
    let held = crate::outside::hold(&art).expect("held");
    let writer = fs::OpenOptions::new().write(true).open(&art);
    assert_eq!(
        writer.map(|_| ()).map_err(|e| e.raw_os_error()),
        Err(Some(32)),
        "a writer is kept out"
    );
    pikslar_platform::replace_file(&art, &fresh, &backup).expect("replaced while held");
    drop(held);
    assert_eq!(fs::read(&art).expect("read"), b"new");
    // A writer already in the file keeps the hold out instead, which is tried again.
    let writer = fs::OpenOptions::new()
        .write(true)
        .share_mode(7)
        .open(&art)
        .expect("writer");
    let hold = crate::outside::hold(&art).map(|_| ());
    assert_eq!(hold.map_err(|e| e.raw_os_error()), Err(Some(32)));
    drop(writer);
    let _ = fs::remove_dir_all(&d);
}

#[repr(C)]
#[derive(Default)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: isize,
}

#[link(name = "kernel32")]
extern "system" {
    fn LockFileEx(
        file: isize,
        flags: u32,
        reserved: u32,
        low: u32,
        high: u32,
        overlapped: *mut Overlapped,
    ) -> i32;
}

/// Another program holds an exclusive lock on part of the project, sharing everything else.
/// Whatever Windows allows, the outcome is honest: saved and reading back as the new project,
/// or refused with the old project whole and a reason given.
#[test]
fn a_project_with_a_locked_byte_range_is_saved_whole_or_refused_with_a_reason() {
    use std::os::windows::io::AsRawHandle;
    let d = dir("range-lock");
    let art = d.join("art.piks");
    fs::write(&art, b"old project bytes").expect("old");
    let holder = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(7)
        .open(&art)
        .expect("held, sharing all");
    let mut at = Overlapped::default();
    // SAFETY: the handle is open for this whole call and `at` outlives it; the call does not
    // keep the pointer (no event, no completion port).
    let locked = unsafe { LockFileEx(holder.as_raw_handle() as isize, 3, 0, 4, 0, &mut at) };
    assert_ne!(locked, 0, "the range is locked");
    let saved = files::save_checked(&art, &doc(), None);
    let backup_left = saved.as_ref().ok().and_then(|(_, p)| p.backup_left.clone());
    drop(holder);
    match saved {
        Ok((bytes, _)) => assert_eq!(fs::read(&art).expect("read"), bytes, "whole"),
        Err(failed) => {
            let said = failed.error.to_string();
            assert!(!said.is_empty(), "a reason is given");
            assert_eq!(fs::read(&art).expect("read"), b"old project bytes");
        }
    }
    if let Some(backup) = backup_left {
        assert!(backup.is_file(), "a backup said to be left is there");
    }
    assert!(no_temps(&d));
    let _ = fs::remove_dir_all(&d);
}
