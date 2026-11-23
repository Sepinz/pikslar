//! Writing a file the user chose to save as, against a hostile disk: links and junctions where
//! the file goes, the place changed between the pick and the write, streams and device names,
//! read-only files, failed writes, and cancels. In every refused case the files are as they
//! were and no temporary file is left behind.

mod common;

use common::{file_symlink, junction, TempDir};
use pikslar_plugin::files::{
    Access, Disk, FileError, FileGrants, FileHandle, FilePicker, FileRequest, PlainDisk,
    MOST_FILE_BYTES,
};
use pikslar_plugin::{Capability, CapabilitySet};
use std::cell::Cell;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

/// Answers each dialog with the next path.
struct Choices(Vec<PathBuf>);

impl FilePicker for Choices {
    fn pick(&mut self, _: &str, _: &FileRequest) -> Option<PathBuf> {
        match self.0.is_empty() {
            true => None,
            false => Some(self.0.remove(0)),
        }
    }
}

/// Grants with each of `targets` picked to save as, and their handles.
fn picked(targets: &[PathBuf], disk: Box<dyn Disk>) -> (FileGrants, Vec<FileHandle>) {
    let caps = CapabilitySet::NONE.with(Capability::PickFileToWrite);
    let mut grants = FileGrants::for_run("Exporter", caps).with_disk(disk);
    let mut picker = Choices(targets.to_vec());
    let request = FileRequest::new(Access::Write, "Save the report", &["txt"]).expect("request");
    let handles = targets
        .iter()
        .map(|_| {
            grants
                .pick(&mut picker, &request)
                .expect("picked")
                .expect("chosen")
        })
        .collect();
    (grants, handles)
}

/// Claims each handle and commits `bytes` to all of them.
fn write_all(
    grants: &mut FileGrants,
    handles: &[FileHandle],
    bytes: &[u8],
) -> Result<(), FileError> {
    for &h in handles {
        grants.claim_write(h, bytes.len() as u64)?;
    }
    let writes: Vec<(FileHandle, &[u8])> = handles.iter().map(|&h| (h, bytes)).collect();
    grants.commit(&writes, &|| true)
}

fn write_one(target: &Path, bytes: &[u8]) -> Result<(), FileError> {
    let (mut grants, handles) = picked(&[target.to_path_buf()], Box::new(PlainDisk));
    write_all(&mut grants, &handles, bytes)
}

/// What a folder holds, by name.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("listed")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn is_refused(r: &Result<(), FileError>) -> bool {
    matches!(r, Err(FileError::Refused(_)))
}

#[test]
fn files_are_replaced_and_made_and_nothing_else_is_left() {
    let t = TempDir::new("w-ok");
    fs::write(t.join("old.txt"), "old").expect("written");
    let (mut grants, handles) =
        picked(&[t.join("old.txt"), t.join("new.txt")], Box::new(PlainDisk));
    write_all(&mut grants, &handles, b"report").expect("written");
    assert_eq!(fs::read(t.join("old.txt")).expect("read"), b"report");
    assert_eq!(fs::read(t.join("new.txt")).expect("read"), b"report");
    assert_eq!(names(t.path()), ["new.txt", "old.txt"]);
}

#[test]
fn a_junction_as_the_folder_is_refused_and_nothing_reaches_its_target() {
    let t = TempDir::new("w-junction");
    let real = t.join("real");
    fs::create_dir_all(&real).expect("a folder");
    let link = t.join("link");
    if !junction(&link, &real) {
        eprintln!("no junction made; skipped");
        return;
    }
    let r = write_one(&link.join("out.txt"), b"x");
    assert!(is_refused(&r), "{r:?}");
    assert!(names(&real).is_empty(), "{:?}", names(&real));
}

#[test]
fn a_link_as_the_target_is_refused_and_what_it_names_is_untouched() {
    let t = TempDir::new("w-symlink");
    fs::write(t.join("victim.txt"), "keep").expect("written");
    let link = t.join("out.txt");
    if !file_symlink(&link, &t.join("victim.txt")) {
        eprintln!("no symbolic link made (developer mode is off); skipped");
        return;
    }
    let r = write_one(&link, b"x");
    assert!(is_refused(&r), "{r:?}");
    assert_eq!(fs::read(t.join("victim.txt")).expect("read"), b"keep");
    assert!(fs::symlink_metadata(&link)
        .expect("still there")
        .file_type()
        .is_symlink());
    assert_eq!(names(t.path()), ["out.txt", "victim.txt"]);
}

#[test]
fn a_target_replaced_by_a_folder_after_the_pick_is_refused() {
    let t = TempDir::new("w-folder");
    let target = t.join("out.txt");
    fs::write(&target, "old").expect("written");
    let (mut grants, handles) = picked(std::slice::from_ref(&target), Box::new(PlainDisk));
    fs::remove_file(&target).expect("removed");
    fs::create_dir_all(target.join("inside")).expect("a folder in its place");
    let r = write_all(&mut grants, &handles, b"x");
    assert!(is_refused(&r), "{r:?}");
    assert!(target.join("inside").is_dir());
    assert_eq!(names(t.path()), ["out.txt"]);
}

#[test]
fn a_folder_chosen_as_the_target_is_refused() {
    let t = TempDir::new("w-dir");
    fs::create_dir_all(t.join("out.txt")).expect("a folder");
    let r = write_one(&t.join("out.txt"), b"x");
    assert!(is_refused(&r), "{r:?}");
    assert_eq!(names(t.path()), ["out.txt"]);
}

#[test]
fn a_file_that_appears_after_the_pick_is_not_overwritten() {
    let t = TempDir::new("w-appeared");
    let target = t.join("out.txt");
    let (mut grants, handles) = picked(std::slice::from_ref(&target), Box::new(PlainDisk));
    fs::write(&target, "theirs").expect("written");
    let r = write_all(&mut grants, &handles, b"x");
    assert!(is_refused(&r), "{r:?}");
    assert_eq!(fs::read(&target).expect("read"), b"theirs");
    assert_eq!(names(t.path()), ["out.txt"]);
}

#[test]
fn streams_and_device_names_are_refused() {
    let t = TempDir::new("w-names");
    fs::write(t.join("out.txt"), "old").expect("written");
    for name in [
        "out.txt:hidden",
        "out.txt::$DATA",
        "nul.txt",
        "COM1",
        "con .txt",
        "aux",
    ] {
        let r = write_one(&t.join(name), b"x");
        assert!(is_refused(&r), "{name}: {r:?}");
    }
    assert_eq!(fs::read(t.join("out.txt")).expect("read"), b"old");
    assert_eq!(names(t.path()), ["out.txt"]);
}

#[test]
fn a_read_only_file_is_refused_and_kept() {
    let t = TempDir::new("w-readonly");
    let target = t.join("out.txt");
    fs::write(&target, "old").expect("written");
    let mut perms = fs::metadata(&target).expect("meta").permissions();
    perms.set_readonly(true);
    fs::set_permissions(&target, perms.clone()).expect("read-only");
    let r = write_one(&target, b"x");
    assert!(is_refused(&r), "{r:?}");
    assert_eq!(fs::read(&target).expect("read"), b"old");
    assert_eq!(names(t.path()), ["out.txt"]);
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    let _ = fs::set_permissions(&target, perms);
}

/// A disk that fails where it is told to.
struct Failing {
    fill: bool,
    /// Fails the n-th replace or place, counting from 0.
    place_at: Option<usize>,
    placed: usize,
}

impl Failing {
    fn new(fill: bool, place_at: Option<usize>) -> Failing {
        Failing {
            fill,
            place_at,
            placed: 0,
        }
    }

    fn turn(&mut self) -> io::Result<()> {
        let n = self.placed;
        self.placed = n + 1;
        match self.place_at == Some(n) {
            true => Err(io::Error::new(io::ErrorKind::PermissionDenied, "in use")),
            false => Ok(()),
        }
    }
}

impl Disk for Failing {
    fn fill(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        match self.fill {
            true => {
                use std::io::Write;
                file.write_all(&bytes[..bytes.len() / 2])?;
                Err(io::Error::new(io::ErrorKind::StorageFull, "disk full"))
            }
            false => PlainDisk.fill(file, bytes),
        }
    }

    fn replace(&mut self, target: &Path, fresh: &Path, backup: &Path) -> io::Result<()> {
        self.turn()?;
        PlainDisk.replace(target, fresh, backup)
    }

    fn place_new(&mut self, fresh: &Path, target: &Path) -> io::Result<()> {
        self.turn()?;
        PlainDisk.place_new(fresh, target)
    }

    fn put_back(&mut self, backup: &Path, target: &Path) -> io::Result<()> {
        PlainDisk.put_back(backup, target)
    }
}

#[test]
fn a_full_disk_leaves_the_original_and_no_temporary_file() {
    let t = TempDir::new("w-full");
    let target = t.join("out.txt");
    fs::write(&target, "old").expect("written");
    let (mut grants, handles) = picked(
        std::slice::from_ref(&target),
        Box::new(Failing::new(true, None)),
    );
    let r = write_all(&mut grants, &handles, b"a longer report");
    let why = r.expect_err("failed").to_string();
    assert!(why.contains("no storage space"), "{why}");
    assert!(!why.contains("pikslar-plugin-w-full"), "{why}");
    assert_eq!(fs::read(&target).expect("read"), b"old");
    assert_eq!(names(t.path()), ["out.txt"]);
}

#[test]
fn files_are_written_all_or_none() {
    let t = TempDir::new("w-all");
    fs::write(t.join("a.txt"), "a").expect("written");
    let targets = [t.join("a.txt"), t.join("b.txt"), t.join("c.txt")];
    fs::write(&targets[2], "c").expect("written");
    // The third fails: the first is put back from its backup, the new second removed.
    let (mut grants, handles) = picked(&targets, Box::new(Failing::new(false, Some(2))));
    let r = write_all(&mut grants, &handles, b"new");
    assert!(matches!(r, Err(FileError::Io(_))), "{r:?}");
    assert_eq!(fs::read(&targets[0]).expect("read"), b"a");
    assert_eq!(fs::read(&targets[2]).expect("read"), b"c");
    assert_eq!(names(t.path()), ["a.txt", "c.txt"]);
}

#[test]
fn a_cancel_while_files_are_written_changes_none() {
    let t = TempDir::new("w-cancel");
    fs::write(t.join("a.txt"), "a").expect("written");
    let targets = [t.join("a.txt"), t.join("b.txt")];
    let (mut grants, handles) = picked(&targets, Box::new(PlainDisk));
    for &h in &handles {
        grants.claim_write(h, 3).expect("claimed");
    }
    let asked = Cell::new(0);
    // Yes for the first file, then cancelled.
    let go_on = || {
        asked.set(asked.get() + 1);
        asked.get() < 2
    };
    let writes = [(handles[0], &b"new"[..]), (handles[1], &b"new"[..])];
    assert_eq!(grants.commit(&writes, &go_on), Err(FileError::Cancelled));
    assert_eq!(fs::read(&targets[0]).expect("read"), b"a");
    assert_eq!(names(t.path()), ["a.txt"]);
}

#[test]
fn a_handle_is_written_once_and_never_past_the_limit() {
    let t = TempDir::new("w-once");
    let (mut grants, handles) = picked(&[t.join("out.txt")], Box::new(PlainDisk));
    let h = handles[0];
    assert_eq!(
        grants.claim_write(h, MOST_FILE_BYTES + 1),
        Err(FileError::TooLarge)
    );
    grants.claim_write(h, 1).expect("the first");
    assert_eq!(grants.claim_write(h, 1), Err(FileError::AlreadyWritten));
    assert_eq!(
        grants.claim_write(FileHandle::from_number(5), 1),
        Err(FileError::UnknownHandle)
    );
    // A handle not claimed cannot be committed.
    let (mut other, unclaimed) = picked(&[t.join("x.txt")], Box::new(PlainDisk));
    assert_eq!(
        other.commit(&[(unclaimed[0], b"x")], &|| true),
        Err(FileError::UnknownHandle)
    );
    assert!(names(t.path()).is_empty());
}

#[test]
fn a_junction_as_the_target_is_refused_and_what_it_names_is_untouched() {
    let t = TempDir::new("w-junction-target");
    let real = t.join("real");
    fs::create_dir_all(&real).expect("a folder");
    fs::write(real.join("keep.txt"), "keep").expect("written");
    let target = t.join("out.txt");
    if !junction(&target, &real) {
        eprintln!("no junction made; skipped");
        return;
    }
    let r = write_one(&target, b"x");
    assert!(is_refused(&r), "{r:?}");
    assert_eq!(names(&real), ["keep.txt"]);
    assert_eq!(names(t.path()), ["out.txt", "real"]);
}
