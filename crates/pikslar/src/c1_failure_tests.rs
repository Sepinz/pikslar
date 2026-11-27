//! Review finding C1 (RC3), end to end: a save whose new file did not read back and whose
//! original could not be put back, on a document with no change since its last save. The
//! saves run through the real save thread, with a misbehaving disk under the real atomic save;
//! the app then must not take the work as saved. Closing asks, a recovery copy is written and
//! survives, a crash restores the work, and reopening offers the kept original. Also an open
//! and a session end that meet such a save while it runs.

use crate::app::App;
use crate::files::{self, FileError};
use crate::recovery::Recovery;
use crate::system_publish::{clear_test_disk, set_test_disk, TestDisk};
use crate::test_app::*;
use pikslar_format::publish::{
    backup_path, read_back_stamped, write_atomic_with, Publish, ReadBack,
};
use pikslar_format::publish_back::{not_kept, Left};
use pikslar_format::upgrade::UpgradeError;
use pikslar_format::FormatError;
use pikslar_platform::Window;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pikslar-c1-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("dir");
    dir
}

/// How the disk misbehaves after it replaced the project.
#[derive(Clone, Copy, PartialEq)]
enum After {
    /// The new file is garbled; the original stays at its backup name.
    Garbled,
    /// As `Garbled`, and the backup is gone too.
    BackupLost,
    /// Another program writes the project just after the replace.
    OtherProgram,
    /// The replace fails after moving the original aside, and nothing moves it back.
    Vanished,
}

struct Disk(After);

impl Publish for Disk {
    fn replace(&mut self, target: &Path, temp: &Path, backup: &Path) -> io::Result<()> {
        fs::rename(target, backup)?;
        if self.0 == After::Vanished {
            return Err(io::Error::other(
                "injected: the replacement could not be moved in",
            ));
        }
        fs::rename(temp, target)?;
        if self.0 == After::OtherProgram {
            return Ok(());
        }
        fs::write(target, b"garbled")?;
        if self.0 == After::BackupLost {
            fs::remove_file(backup)?;
        }
        Ok(())
    }
    fn place_new(&mut self, temp: &Path, target: &Path) -> io::Result<()> {
        fs::rename(temp, target)
    }
    fn read_back(&mut self, target: &Path, bytes: &[u8]) -> io::Result<ReadBack> {
        if self.0 == After::OtherProgram {
            // After this save placed its file and took note of it, before the read back.
            std::thread::sleep(std::time::Duration::from_millis(20));
            fs::write(target, b"written by another program, longer than the first")?;
        }
        read_back_stamped(target, bytes)
    }
    fn restore(&mut self, _backup: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::other("injected: cannot put back"))
    }
}

fn disk_for(after: After) -> TestDisk {
    match after {
        After::Garbled => || Box::new(Disk(After::Garbled)),
        After::BackupLost => || Box::new(Disk(After::BackupLost)),
        After::OtherProgram => || Box::new(Disk(After::OtherProgram)),
        After::Vanished => || Box::new(Disk(After::Vanished)),
    }
}

/// The real atomic save of the document to `art`, through a disk that misbehaves: the error
/// it gives.
fn failed_save(a: &App, art: &Path, after: After) -> FormatError {
    let bytes = files::checked_bytes(a.editor.doc()).expect("encodes");
    write_atomic_with(art, &bytes, &mut |_, _| Ok(()), &mut Disk(after)).expect_err("caught")
}

/// Ctrl+S, through the save thread, onto a disk that misbehaves. False: nothing was saved.
fn save_onto(a: &mut App, art: &Path, after: After) -> bool {
    set_test_disk(art, disk_for(after));
    let saved = a.save(Window::detached(), false);
    clear_test_disk(art);
    saved
}

/// A saved app with recovery on, nothing changed since the save. Its work's bytes.
fn saved(dir: &Path) -> (App, PathBuf, Vec<u8>) {
    let art = dir.join("art.piks");
    let mut a = app(1);
    a.use_recovery(Recovery::in_dir(&dir.join("recovery")), false);
    dot(&mut a, 2, 2);
    assert!(a.save_to(Window::detached(), art.clone()), "the first save");
    let work = files::checked_bytes(a.editor.doc()).expect("encodes");
    assert!(!a.is_modified(), "nothing changed since the save");
    (a, art, work)
}

fn copy_now(a: &mut App) {
    a.tick_recovery();
    a.recovery.wait_for_writer();
}

/// True if the recovery folder holds a copy of exactly `work`.
fn copy_holds(dir: &Path, work: &[u8]) -> bool {
    let Ok(entries) = fs::read_dir(dir.join("recovery")) else {
        return false;
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "piks"))
        .filter_map(|p| files::open(&p).ok())
        .any(|opened| files::checked_bytes(&opened.doc).ok().as_deref() == Some(work))
}

/// After the failure: unsaved, said, copied, kept through a quit, and restored after a crash.
fn work_survives(mut a: App, dir: &Path, work: &[u8]) {
    assert!(a.is_modified(), "the file is not the work");
    assert!(a.has_unsaved_work(), "closing asks before discarding");
    let said = a.save.to_report.clone().unwrap_or_default();
    assert!(!said.is_empty(), "the failure is shown");
    assert!(!said.contains("  "), "no runs of spaces: {said}");
    copy_now(&mut a);
    assert!(copy_holds(dir, work), "a recovery copy holds the work");
    for _ in 0..3 {
        copy_now(&mut a);
    }
    assert!(copy_holds(dir, work), "later ticks never clear it");
    assert!(a.end_session(), "quitting keeps the copy");
    a.recovery.simulate_crash();
    drop(a);
    let mut next = app(1);
    next.use_recovery(Recovery::in_dir(&dir.join("recovery")), false);
    let orphan = next.recovery.find_orphan().expect("the work was left");
    next.restore_orphan(&orphan).expect("the copy is whole");
    let restored = files::checked_bytes(next.editor.doc()).expect("encodes");
    assert_eq!(restored, work, "the restored work is the work");
    assert!(next.is_modified(), "restored work reads as unsaved");
}

#[test]
fn a_garbled_save_with_the_original_kept_never_reads_as_saved() {
    let dir = temp_dir("garbled");
    let (mut a, art, work) = saved(&dir);
    let e = failed_save(&a, &art, After::Garbled);
    let backup = backup_path(&art);
    assert_eq!(
        not_kept(&e).map(|k| k.left.clone()),
        Some(Left::BackupAt(backup.clone()))
    );
    // That was the atomic save alone; put things back and fail the save itself.
    fs::write(&art, &work).expect("as before the probe");
    fs::remove_file(&backup).expect("the probe's backup");
    a.seen = crate::outside::Seen::of(&art, None);
    assert!(!save_onto(&mut a, &art, After::Garbled), "the save failed");
    let said = a.save.to_report.clone().unwrap_or_default();
    assert!(said.contains(&backup.display().to_string()), "{said}");
    // The kept original is not one a later save may replace unasked.
    a.ask_replace_backup = |_, _| None;
    dot(&mut a, 4, 4);
    assert!(!a.save(Window::detached(), false), "refused");
    assert_eq!(fs::read(&backup).expect("kept"), work);
    a.undo();
    work_survives(a, &dir, &work);
    // Reopening the project offers the kept original, which is the work.
    static OFFERED: AtomicUsize = AtomicUsize::new(0);
    let mut b = app(1);
    b.ask_backup = |_, _| {
        OFFERED.fetch_add(1, Ordering::Relaxed);
        Some(true)
    };
    b.open_now(Window::detached(), art.clone());
    assert_eq!(OFFERED.load(Ordering::Relaxed), 1, "offered once");
    let opened = files::checked_bytes(b.editor.doc()).expect("encodes");
    assert_eq!(opened, work, "the kept original is open");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_garbled_save_with_no_original_left_keeps_the_work_only_in_recovery() {
    let dir = temp_dir("lost");
    let (mut a, art, work) = saved(&dir);
    assert!(
        !save_onto(&mut a, &art, After::BackupLost),
        "the save failed"
    );
    assert!(!backup_path(&art).exists(), "no original is left");
    assert_eq!(
        fs::read(&art).expect("left"),
        b"garbled",
        "nothing was put back"
    );
    work_survives(a, &dir, &work);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_save_another_program_wrote_over_never_reads_as_saved() {
    let dir = temp_dir("other");
    let (mut a, art, work) = saved(&dir);
    assert!(
        !save_onto(&mut a, &art, After::OtherProgram),
        "the save failed"
    );
    let said = a.save.to_report.clone().unwrap_or_default();
    assert!(said.contains("another program"), "{said}");
    work_survives(a, &dir, &work);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_upgrade_save_that_left_other_bytes_never_reads_as_saved() {
    let dir = temp_dir("upgrade");
    let (mut a, art, work) = saved(&dir);
    let e = failed_save(&a, &art, After::Garbled);
    a.save_ended_failed(&art, FileError::Upgrade(UpgradeError::from(e)));
    let said = a.save.to_report.clone().unwrap_or_default();
    assert!(
        !said.contains("left as it was"),
        "never claims the file was kept: {said}"
    );
    work_survives(a, &dir, &work);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_save_that_put_the_original_back_leaves_a_saved_document_saved() {
    let dir = temp_dir("put-back");
    let (mut a, art, work) = saved(&dir);
    let kept = pikslar_format::publish_back::NotKept::new("injected".into(), Left::AsItWas);
    let io = io::Error::new(io::ErrorKind::InvalidData, kept);
    a.save_ended_failed(&art, FileError::from(FormatError::from(io)));
    assert!(!a.is_modified(), "the file is the work, as before the save");
    assert_eq!(fs::read(&art).expect("the file"), work);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_replace_that_left_no_file_never_reads_as_saved() {
    let dir = temp_dir("vanished");
    let (mut a, art, work) = saved(&dir);
    assert!(!save_onto(&mut a, &art, After::Vanished), "the save failed");
    assert!(!art.exists(), "the project is not at its name");
    assert_eq!(fs::read(backup_path(&art)).expect("kept"), work);
    work_survives(a, &dir, &work);
    let _ = fs::remove_dir_all(&dir);
}

/// Waits, collecting as frames do, until the save under way has ended.
fn pump_until_saved(a: &mut App) {
    let started = std::time::Instant::now();
    while a.save_job.is_some() {
        assert!(started.elapsed().as_secs() < 20, "the save ends");
        a.collect_opened(Window::detached());
        a.collect_save(Window::detached());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    a.collect_opened(Window::detached());
}

#[test]
fn an_open_under_way_never_replaces_work_a_failing_save_left_unsaved() {
    let dir = temp_dir("open-during");
    let (mut a, art, work) = saved(&dir);
    let other = dir.join("other.piks");
    let mut b = app(1);
    dot(&mut b, 7, 7);
    assert!(
        b.save_to(Window::detached(), other.clone()),
        "another project"
    );
    // Nothing unsaved, so the open asks nothing and starts reading.
    a.begin_open(Window::detached(), other, crate::loading::Then::Nothing);
    a.save_delay = Some(std::time::Duration::from_millis(300));
    set_test_disk(&art, disk_for(After::BackupLost));
    assert_eq!(
        a.start_save(Window::detached(), false),
        crate::save_run::Started::Now
    );
    pump_until_saved(&mut a);
    clear_test_disk(&art);
    for _ in 0..20 {
        a.collect_opened(Window::detached());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(
        a.path.as_deref(),
        Some(art.as_path()),
        "still the same project"
    );
    let shown = files::checked_bytes(a.editor.doc()).expect("encodes");
    assert_eq!(shown, work, "the work was not replaced");
    a.save_delay = None;
    work_survives(a, &dir, &work);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_session_end_during_a_save_copies_the_work_before_the_save_is_known_good() {
    let _guard = SESSION_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp_dir("end-during");
    let (mut a, art, work) = saved(&dir);
    a.save_delay = Some(std::time::Duration::from_millis(300));
    set_test_disk(&art, disk_for(After::BackupLost));
    assert_eq!(
        a.start_save(Window::detached(), false),
        crate::save_run::Started::Now
    );
    assert!(
        a.has_unsaved_work(),
        "a save under way is not yet the work saved"
    );
    a.session_over_within(true, std::time::Duration::from_secs(4));
    assert!(
        copy_holds(&dir, &work),
        "the work was copied as the session ended"
    );
    pump_until_saved(&mut a);
    clear_test_disk(&art);
    a.save_delay = None;
    assert!(copy_holds(&dir, &work), "the failed save left the copy");
    work_survives(a, &dir, &work);
    let _ = fs::remove_dir_all(&dir);
}
