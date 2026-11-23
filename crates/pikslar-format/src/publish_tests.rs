use super::*;
use std::time::Instant;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pikslar-publish-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).expect("temp dir");
    d
}

/// Does what `ReplaceFileW` with a backup does, by renames, and can be told to fail.
#[derive(Default)]
struct Fake {
    /// Errors to give before a replace or a move works.
    busy: Vec<i32>,
    tries: usize,
    stop_at: Option<Published>,
    /// Written over the target after the replace, as a disk that lies would.
    garble: bool,
    new_placed: bool,
    /// Another program writes the target just after it read back.
    written_after: bool,
    /// The read back of the target fails with this Windows error.
    read_error: Option<i32>,
    /// Putting the original back fails.
    restore_fails: bool,
    /// A look at the target finds this session may not write it.
    denied: bool,
    /// The backup cannot be removed once the save is done.
    backup_stuck: bool,
    /// Another save replaces the target just after this one did; true if it then removes the
    /// backup, as a finished save does.
    interloper: Option<bool>,
    /// The backup goes missing after the replace.
    lose_backup: bool,
    /// Told when the replace starts, then waits to be told to go on.
    pause: Option<(
        std::sync::mpsc::Sender<PathBuf>,
        std::sync::mpsc::Receiver<()>,
    )>,
}

impl Fake {
    /// What a second process saving the same file does between this one's replace and read back.
    fn other_save(&mut self, target: &Path) -> io::Result<()> {
        std::thread::sleep(Duration::from_millis(20));
        let backup = backup_path(target);
        if self.lose_backup {
            fs::remove_file(&backup)?;
        }
        let Some(removes) = self.interloper else {
            return Ok(());
        };
        let temp = target.with_extension("other-temp");
        fs::write(&temp, b"the other save")?;
        fs::rename(target, &backup)?;
        fs::rename(&temp, target)?;
        if removes {
            fs::remove_file(&backup)?;
        }
        Ok(())
    }

    fn next_busy(&mut self) -> io::Result<()> {
        self.tries += 1;
        match self.busy.pop() {
            Some(code) => Err(io::Error::from_raw_os_error(code)),
            None => Ok(()),
        }
    }
}

impl Publish for Fake {
    fn replace(&mut self, target: &Path, temp: &Path, backup: &Path) -> io::Result<()> {
        if let Some((told, go)) = &self.pause {
            told.send(temp.to_path_buf()).expect("told");
            go.recv().expect("go on");
        }
        self.next_busy()?;
        fs::rename(target, backup)?;
        fs::rename(temp, target)?;
        if self.garble {
            fs::write(target, b"not what was saved")?;
        }
        Ok(())
    }

    fn place_new(&mut self, temp: &Path, target: &Path) -> io::Result<()> {
        self.next_busy()?;
        if target.exists() {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        self.new_placed = true;
        fs::rename(temp, target)
    }

    fn reached(&mut self, step: Published, target: &Path) -> io::Result<()> {
        if step == Published::Replaced {
            self.other_save(target)?;
        }
        if step == Published::ReadBack && self.written_after {
            std::thread::sleep(Duration::from_millis(20));
            fs::write(target, b"written by another program")?;
        }
        match self.stop_at == Some(step) {
            true => Err(io::Error::other("the program stopped here")),
            false => Ok(()),
        }
    }

    fn read_back(&mut self, target: &Path, bytes: &[u8]) -> io::Result<ReadBack> {
        match self.read_error.take() {
            Some(code) => Err(io::Error::from_raw_os_error(code)),
            None => read_back_stamped(target, bytes),
        }
    }

    fn denied(&mut self, _target: &Path) -> bool {
        self.denied
    }

    fn remove_backup(&mut self, backup: &Path) -> io::Result<()> {
        match self.backup_stuck {
            true => Err(io::Error::from_raw_os_error(32)),
            false => fs::remove_file(backup),
        }
    }

    fn restore(&mut self, backup: &Path, target: &Path) -> io::Result<()> {
        match self.restore_fails {
            true => Err(io::Error::from_raw_os_error(32)),
            false => fs::rename(backup, target),
        }
    }
}

fn save(path: &Path, bytes: &[u8], fake: &mut Fake) -> Result<Placed, FormatError> {
    write_atomic_with(path, bytes, &mut |_, _| Ok(()), fake)
}

#[test]
fn a_save_over_a_file_leaves_only_the_new_one() {
    let d = dir("good");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    save(&path, b"new", &mut Fake::default()).expect("saved");
    assert_eq!(fs::read(&path).expect("read"), b"new");
    assert!(
        !backup_path(&path).exists(),
        "the backup goes once the file reads back"
    );
    assert!(temps_beside(&path).is_empty());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn stopped_after_the_replace_the_old_file_is_the_backup_and_the_new_one_is_whole() {
    for step in [Published::Replaced, Published::ReadBack] {
        let d = dir(&format!("{step:?}"));
        let path = d.join("art.piks");
        fs::write(&path, b"old").expect("old");
        let mut fake = Fake {
            stop_at: Some(step),
            ..Fake::default()
        };
        assert!(save(&path, b"new", &mut fake).is_err(), "{step:?}");
        assert_eq!(fs::read(&path).expect("read"), b"new", "{step:?}");
        let backup = fs::read(backup_path(&path)).expect("the backup");
        assert_eq!(backup, b"old", "{step:?}: the old file is whole beside it");
        let _ = fs::remove_dir_all(&d);
    }
}

#[test]
fn a_first_save_is_moved_into_place_and_makes_no_backup() {
    let d = dir("first");
    let path = d.join("art.piks");
    let mut fake = Fake::default();
    save(&path, b"new", &mut fake).expect("saved");
    assert!(fake.new_placed, "a plain move, never a replace");
    assert_eq!(fs::read(&path).expect("read"), b"new");
    assert!(!backup_path(&path).exists());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_file_held_for_a_moment_is_tried_again_until_it_is_free() {
    let d = dir("held-moment");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        busy: vec![32, 5, 32],
        ..Fake::default()
    };
    save(&path, b"new", &mut fake).expect("saved once free");
    assert_eq!(fake.tries, 4);
    assert_eq!(fs::read(&path).expect("read"), b"new");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_file_held_throughout_is_left_alone_and_the_message_says_why_after_about_two_seconds() {
    let d = dir("held");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        busy: vec![32; 20],
        ..Fake::default()
    };
    let start = Instant::now();
    let e = save(&path, b"new", &mut fake).expect_err("held");
    let took = start.elapsed();
    assert!(
        took >= Duration::from_millis(1800) && took < Duration::from_secs(4),
        "{took:?}"
    );
    let said = e.to_string();
    assert!(
        said.contains("art.piks is open in another program"),
        "{said}"
    );
    assert!(said.contains("error 32"), "{said}");
    let FormatError::Io(io) = &e else {
        panic!("an io error: {e}");
    };
    let in_use = io.get_ref().and_then(|i| i.downcast_ref::<InUse>());
    assert_eq!(
        in_use.map(|i| i.code),
        Some(32),
        "the code is kept for the log"
    );
    assert_eq!(
        fs::read(&path).expect("read"),
        b"old",
        "the old file is whole"
    );
    assert!(temps_beside(&path).is_empty());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn another_failure_is_not_tried_again() {
    let d = dir("other");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        busy: vec![2],
        ..Fake::default()
    };
    assert!(save(&path, b"new", &mut fake).is_err());
    assert_eq!(fake.tries, 1);
    assert_eq!(fs::read(&path).expect("read"), b"old");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_read_only_target_is_refused_with_a_message() {
    let d = dir("read-only");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut perms = fs::metadata(&path).expect("meta").permissions();
    perms.set_readonly(true);
    fs::set_permissions(&path, perms.clone()).expect("read-only");
    let said = save(&path, b"new", &mut Fake::default())
        .expect_err("refused")
        .to_string();
    assert!(said.contains("read-only"), "{said}");
    assert_eq!(fs::read(&path).expect("read"), b"old");
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&path, perms).expect("writable");
    let _ = fs::remove_dir_all(&d);
}
fn left_by(e: &FormatError) -> Option<crate::publish_back::Left> {
    crate::publish_back::not_kept(e).map(|n| n.left.clone())
}

#[test]
fn a_target_that_reads_back_wrong_is_put_back_as_it_was() {
    let d = dir("garbled");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        garble: true,
        ..Fake::default()
    };
    let e = save(&path, b"new", &mut fake).expect_err("caught");
    let said = e.to_string();
    assert!(said.contains("did not read back correctly"), "{said}");
    assert!(said.contains("put back as it was"), "{said}");
    assert_eq!(left_by(&e), Some(crate::publish_back::Left::AsItWas));
    assert_eq!(fs::read(&path).expect("read"), b"old", "the file as it was");
    assert!(
        !backup_path(&path).exists(),
        "the backup went back in place"
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_read_back_that_fails_with_an_io_error_puts_the_original_back() {
    let d = dir("read-error");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        read_error: Some(23),
        ..Fake::default()
    };
    let e = save(&path, b"new", &mut fake).expect_err("caught");
    assert_eq!(left_by(&e), Some(crate::publish_back::Left::AsItWas));
    assert!(!crate::publish_back::not_kept(&e)
        .expect("kept")
        .target_changed());
    assert_eq!(fs::read(&path).expect("read"), b"old");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn an_original_that_cannot_be_put_back_is_named_by_its_backup() {
    let d = dir("no-restore");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        garble: true,
        restore_fails: true,
        ..Fake::default()
    };
    let e = save(&path, b"new", &mut fake).expect_err("caught");
    let backup = backup_path(&path);
    let said = e.to_string();
    assert!(said.contains(&backup.display().to_string()), "{said}");
    assert!(said.contains("could not be put back"), "{said}");
    let kept = crate::publish_back::not_kept(&e).expect("kept");
    assert!(kept.target_changed(), "the target holds other bytes now");
    assert_eq!(fs::read(&backup).expect("kept"), b"old");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_first_save_that_reads_back_wrong_leaves_no_file() {
    let d = dir("first-garbled");
    let path = d.join("art.piks");
    let mut fake = Fake {
        read_error: Some(23),
        ..Fake::default()
    };
    let e = save(&path, b"new", &mut fake).expect_err("caught");
    assert_eq!(left_by(&e), Some(crate::publish_back::Left::Removed));
    assert!(!path.exists(), "as it was: no file");
    let _ = fs::remove_dir_all(&d);
}

/// Every temporary file left beside `path`.
fn temps_beside(path: &Path) -> Vec<PathBuf> {
    let name = path
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    let mut found: Vec<PathBuf> = fs::read_dir(path.parent().expect("folder"))
        .expect("listed")
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().into_owned());
            n.is_some_and(|n| n.starts_with(&name) && n.ends_with(crate::atomic::TEMP_SUFFIX))
        })
        .collect();
    found.sort();
    found
}

#[test]
fn a_permission_denial_is_said_as_one_and_not_tried_again() {
    let d = dir("denied");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        busy: vec![5; 20],
        denied: true,
        ..Fake::default()
    };
    let start = Instant::now();
    let said = save(&path, b"new", &mut fake)
        .expect_err("denied")
        .to_string();
    assert_eq!(fake.tries, 1, "not tried again");
    assert!(start.elapsed() < Duration::from_millis(500));
    assert!(said.contains("permission denied"), "{said}");
    assert!(!said.contains("open in another program"), "{said}");
    assert_eq!(fs::read(&path).expect("read"), b"old");
    assert!(temps_beside(&path).is_empty());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_locked_or_mapped_file_is_tried_again_and_said_honestly() {
    for (code, words) in [(33, "locked part of"), (1224, "maps it into memory")] {
        let d = dir(&format!("held-{code}"));
        let path = d.join("art.piks");
        fs::write(&path, b"old").expect("old");
        let mut fake = Fake {
            busy: vec![code, code],
            ..Fake::default()
        };
        save(&path, b"new", &mut fake).expect("saved once free");
        assert_eq!(fake.tries, 3, "{code}");
        let mut fake = Fake {
            busy: vec![code; 20],
            ..Fake::default()
        };
        let said = save(&path, b"newer", &mut fake)
            .expect_err("held")
            .to_string();
        assert!(said.contains(words), "{code}: {said}");
        assert_eq!(fs::read(&path).expect("read"), b"new", "{code}");
        let _ = fs::remove_dir_all(&d);
    }
}

#[test]
fn a_backup_that_cannot_be_removed_is_named() {
    let d = dir("backup-stuck");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        backup_stuck: true,
        ..Fake::default()
    };
    let placed = save(&path, b"new", &mut fake).expect("saved");
    assert_eq!(placed.backup_left, Some(backup_path(&path)));
    assert_eq!(fs::read(&path).expect("read"), b"new");
    let placed = save(&path, b"newer", &mut Fake::default()).expect("saved");
    assert_eq!(placed.backup_left, None, "the next save cleans it up");
    assert!(!backup_path(&path).exists());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_replace_kept_out_by_a_backup_already_there_names_it() {
    let d = dir("backup-in-way");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    fs::write(backup_path(&path), b"older").expect("a backup already there");
    let mut fake = Fake {
        busy: vec![1175],
        ..Fake::default()
    };
    let said = save(&path, b"new", &mut fake)
        .expect_err("kept out")
        .to_string();
    let backup = backup_path(&path).display().to_string();
    assert!(said.contains(&backup), "{said}");
    assert_eq!(fs::read(&path).expect("read"), b"old");
    assert_eq!(fs::read(backup_path(&path)).expect("kept"), b"older");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_full_disk_leaves_the_old_file_whole_and_says_so() {
    for code in [112, 39] {
        let d = dir(&format!("full-{code}"));
        let path = d.join("art.piks");
        fs::write(&path, b"old").expect("old");
        let mut full = |step, _: &Path| match step {
            crate::atomic::SaveStep::HalfWritten => Err(io::Error::from_raw_os_error(code)),
            _ => Ok(()),
        };
        let e = write_atomic_with(&path, b"new", &mut full, &mut Fake::default());
        let said = e.expect_err("full").to_string();
        assert!(said.contains("disk is full"), "{code}: {said}");
        assert_eq!(fs::read(&path).expect("read"), b"old");
        assert!(temps_beside(&path).is_empty(), "{code}");
        let _ = fs::remove_dir_all(&d);
    }
}

#[test]
fn only_stale_temporary_files_are_cleaned_up() {
    let d = dir("stale");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let stale = crate::atomic::run_temp_path(&path, "0000dead");
    let fresh = crate::atomic::run_temp_path(&path, "0000beef");
    let legacy = crate::atomic::temp_path(&path);
    for temp in [&stale, &fresh, &legacy] {
        fs::write(temp, b"left").expect("left");
    }
    let old = std::time::SystemTime::now() - Duration::from_secs(3600);
    for temp in [&stale, &legacy] {
        let file = fs::OpenOptions::new().write(true).open(temp).expect("open");
        file.set_modified(old).expect("aged");
    }
    save(&path, b"new", &mut Fake::default()).expect("saved");
    assert!(!stale.exists() && !legacy.exists(), "stale ones go");
    assert!(fresh.exists(), "one that may be another save's stays");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn the_stamp_of_a_save_is_the_one_its_bytes_read_back_with() {
    let d = dir("stamp");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        written_after: true,
        ..Fake::default()
    };
    let placed = save(&path, b"new", &mut fake).expect("saved");
    let stamp = placed.stamp.expect("a stamp");
    assert_eq!(stamp.len, 3, "the saved bytes, not the later write");
    let now = fs::metadata(&path).expect("meta");
    assert_ne!(
        Some(stamp.len),
        Some(now.len()),
        "which changed the file since"
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_second_save_that_replaced_the_file_is_never_deleted_by_the_first() {
    use crate::publish_back::Left;
    let d = dir("race-gone-backup");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        interloper: Some(true),
        ..Fake::default()
    };
    let e = save(&path, b"the first", &mut fake).expect_err("caught");
    assert_eq!(fs::read(&path).expect("read"), b"the other save");
    assert_eq!(left_by(&e), Some(Left::Overwritten(None)));
    let said = e.to_string();
    assert!(said.contains("written by another"), "{said}");
    assert!(!said.contains("put back as it was"), "{said}");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_second_save_is_not_renamed_over_and_both_files_are_named() {
    use crate::publish_back::Left;
    let d = dir("race-backup-there");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        interloper: Some(false),
        ..Fake::default()
    };
    let e = save(&path, b"the first", &mut fake).expect_err("caught");
    assert_eq!(fs::read(&path).expect("read"), b"the other save");
    let backup = backup_path(&path);
    assert_eq!(left_by(&e), Some(Left::Overwritten(Some(backup.clone()))));
    assert!(e.to_string().contains(&backup.display().to_string()));
    assert_eq!(fs::read(&backup).expect("kept"), b"the first");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn an_existing_file_is_not_deleted_when_its_original_is_not_to_be_found() {
    use crate::publish_back::Left;
    let d = dir("no-original");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let mut fake = Fake {
        garble: true,
        lose_backup: true,
        ..Fake::default()
    };
    let e = save(&path, b"new", &mut fake).expect_err("caught");
    assert_eq!(left_by(&e), Some(Left::Unknown));
    assert!(path.exists(), "an existing project is never deleted");
    let said = e.to_string();
    assert!(said.contains("do not rely on this file"), "{said}");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_second_save_of_one_file_waits_for_the_first_then_says_so() {
    let d = dir("lock");
    let path = d.join("art.piks");
    fs::write(&path, b"old").expect("old");
    let (told, temps) = std::sync::mpsc::channel();
    let (go, wait) = std::sync::mpsc::channel();
    let first = {
        let path = path.clone();
        std::thread::spawn(move || {
            let mut fake = Fake {
                pause: Some((told, wait)),
                ..Fake::default()
            };
            save(&path, b"the first bytes", &mut fake).map(|_| ())
        })
    };
    let first_temp = temps.recv().expect("the first is replacing");
    let e = save(&path, b"the second bytes", &mut Fake::default()).expect_err("kept out");
    let said = e.to_string();
    assert!(said.contains("another save of art.piks"), "{said}");
    assert_eq!(
        fs::read(&first_temp).expect("the first temporary file is its own"),
        b"the first bytes"
    );
    go.send(()).expect("go on");
    first.join().expect("joined").expect("the first is saved");
    assert_eq!(fs::read(&path).expect("read"), b"the first bytes");
    assert!(temps_beside(&path).is_empty());
    save(&path, b"the third", &mut Fake::default()).expect("free again");
    assert_eq!(fs::read(&path).expect("read"), b"the third");
    assert!(!d.join("art.piks.pikslar-lock").exists());
    let _ = fs::remove_dir_all(&d);
}
