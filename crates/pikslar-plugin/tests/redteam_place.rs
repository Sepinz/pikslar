//! Writing a file the user chose, against a disk that fails half-way and against names and
//! folders a plugin must never write to: a replace that moved the original away and could
//! not move it back, a rollback that fails as well, program and script kinds, and Pikslar's
//! own folders reached directly or through a junction. Whatever fails, the user's original
//! is never lost: it is at its place, or kept beside it.

mod common;

use common::{junction, TempDir};
use pikslar_plugin::files::{
    Access, Disk, FileError, FileGrants, FileHandle, FilePicker, FileRequest, PlainDisk,
};
use pikslar_plugin::{Capability, CapabilitySet};
use std::cell::Cell;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Choices(Vec<PathBuf>);

impl FilePicker for Choices {
    fn pick(&mut self, _: &str, _: &FileRequest) -> Option<PathBuf> {
        match self.0.is_empty() {
            true => None,
            false => Some(self.0.remove(0)),
        }
    }
}

fn grants(
    targets: &[PathBuf],
    disk: Box<dyn Disk>,
    protect: &[PathBuf],
) -> (FileGrants, Vec<FileHandle>) {
    let caps = CapabilitySet::NONE.with(Capability::PickFileToWrite);
    let mut grants = FileGrants::for_run("Exporter", caps)
        .with_disk(disk)
        .with_protected(protect.to_vec());
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

fn write(
    targets: &[PathBuf],
    disk: Box<dyn Disk>,
    protect: &[PathBuf],
    bytes: &[u8],
) -> Result<(), FileError> {
    let (mut grants, handles) = grants(targets, disk, protect);
    for &h in &handles {
        grants.claim_write(h, bytes.len() as u64)?;
    }
    let writes: Vec<(FileHandle, &[u8])> = handles.iter().map(|&h| (h, bytes)).collect();
    grants.commit(&writes, &|| true)
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("listed")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The one backup left beside the targets, if any.
fn kept(dir: &Path) -> Option<Vec<u8>> {
    let bak = names(dir)
        .into_iter()
        .find(|n| n.starts_with("~pikslar-") && n.ends_with(".bak"))?;
    fs::read(dir.join(bak)).ok()
}

#[derive(Clone, Copy, Debug)]
enum Replace {
    Works,
    /// Fails having changed nothing.
    FailsCleanly,
    /// Moves the original to the backup name, then fails (Windows' error 1177 with a failed
    /// move back).
    FailsHalfWay,
    /// Replaces the file, and reports an error all the same.
    FailsAfterReplacing,
}

/// A disk that fails the replace of one file, and the put-back of one, as it is told.
struct Broken {
    replace: Replace,
    replace_of: &'static str,
    put_back_fails_for: Option<&'static str>,
}

fn named(path: &Path, name: &str) -> bool {
    path.file_name().is_some_and(|n| n == name)
}

fn failure() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "in use")
}

impl Disk for Broken {
    fn replace(&mut self, target: &Path, fresh: &Path, backup: &Path) -> io::Result<()> {
        let how = match named(target, self.replace_of) {
            true => self.replace,
            false => Replace::Works,
        };
        match how {
            Replace::Works => PlainDisk.replace(target, fresh, backup),
            Replace::FailsCleanly => Err(failure()),
            Replace::FailsHalfWay => fs::rename(target, backup).and(Err(failure())),
            Replace::FailsAfterReplacing => {
                PlainDisk.replace(target, fresh, backup).and(Err(failure()))
            }
        }
    }

    fn place_new(&mut self, fresh: &Path, target: &Path) -> io::Result<()> {
        PlainDisk.place_new(fresh, target)
    }

    fn put_back(&mut self, backup: &Path, target: &Path) -> io::Result<()> {
        match self.put_back_fails_for.is_some_and(|n| named(target, n)) {
            true => Err(failure()),
            false => PlainDisk.put_back(backup, target),
        }
    }
}

fn broken(replace: Replace, put_back_fails: bool) -> Box<dyn Disk> {
    Box::new(Broken {
        replace,
        replace_of: "out.txt",
        put_back_fails_for: put_back_fails.then_some("out.txt"),
    })
}

#[test]
fn a_failed_replace_that_can_be_undone_leaves_only_the_original() {
    for how in [
        Replace::FailsCleanly,
        Replace::FailsHalfWay,
        Replace::FailsAfterReplacing,
    ] {
        let t = TempDir::new("p-undone");
        let target = t.join("out.txt");
        fs::write(&target, "old").expect("written");
        let r = write(
            std::slice::from_ref(&target),
            broken(how, false),
            &[],
            b"new",
        );
        assert!(matches!(r, Err(FileError::Io(_))), "{how:?}: {r:?}");
        assert_eq!(fs::read(&target).expect("read"), b"old", "{how:?}");
        assert_eq!(names(t.path()), ["out.txt"], "{how:?}");
    }
}

#[test]
fn an_original_that_cannot_be_put_back_is_kept_beside_it_never_deleted() {
    for (how, left) in [
        (Replace::FailsHalfWay, None),
        (Replace::FailsAfterReplacing, Some(&b"new"[..])),
    ] {
        let t = TempDir::new("p-kept");
        let target = t.join("out.txt");
        fs::write(&target, "old").expect("written");
        let r = write(
            std::slice::from_ref(&target),
            broken(how, true),
            &[],
            b"new",
        );
        assert_eq!(r, Err(FileError::NotPutBack), "{how:?}");
        assert_eq!(kept(t.path()).as_deref(), Some(&b"old"[..]), "{how:?}");
        assert_eq!(fs::read(&target).ok().as_deref(), left, "{how:?}");
    }
}

#[test]
fn a_replace_that_failed_cleanly_keeps_no_backup_even_when_put_back_would_fail() {
    let t = TempDir::new("p-clean");
    let target = t.join("out.txt");
    fs::write(&target, "old").expect("written");
    let r = write(
        std::slice::from_ref(&target),
        broken(Replace::FailsCleanly, true),
        &[],
        b"new",
    );
    assert!(matches!(r, Err(FileError::Io(_))), "{r:?}");
    assert_eq!(fs::read(&target).expect("read"), b"old");
    assert_eq!(names(t.path()), ["out.txt"]);
}

#[test]
fn earlier_files_are_put_back_when_a_later_one_is_lost_half_way() {
    let t = TempDir::new("p-many");
    fs::write(t.join("a.txt"), "a").expect("written");
    fs::write(t.join("out.txt"), "old").expect("written");
    let targets = [t.join("a.txt"), t.join("new.txt"), t.join("out.txt")];
    let r = write(&targets, broken(Replace::FailsHalfWay, true), &[], b"new");
    assert_eq!(r, Err(FileError::NotPutBack));
    assert_eq!(fs::read(t.join("a.txt")).expect("read"), b"a");
    assert!(!t.join("new.txt").exists());
    assert_eq!(kept(t.path()).as_deref(), Some(&b"old"[..]));
}

/// Moves the new file in place, then reports an error all the same.
struct PlacesThenFails;

impl Disk for PlacesThenFails {
    fn replace(&mut self, target: &Path, fresh: &Path, backup: &Path) -> io::Result<()> {
        PlainDisk.replace(target, fresh, backup)
    }

    fn place_new(&mut self, fresh: &Path, target: &Path) -> io::Result<()> {
        PlainDisk.place_new(fresh, target).and(Err(failure()))
    }

    fn put_back(&mut self, backup: &Path, target: &Path) -> io::Result<()> {
        PlainDisk.put_back(backup, target)
    }
}

#[test]
fn a_new_file_placed_by_a_move_that_reported_an_error_is_taken_away() {
    let t = TempDir::new("p-new");
    let r = write(&[t.join("out.txt")], Box::new(PlacesThenFails), &[], b"new");
    assert!(matches!(r, Err(FileError::Io(_))), "{r:?}");
    assert!(names(t.path()).is_empty(), "{:?}", names(t.path()));
}

#[test]
fn program_and_script_names_are_refused_whatever_the_dialog_offered() {
    let t = TempDir::new("p-kinds");
    for name in [
        "report.bat",
        "report.CMD",
        "a.ps1",
        "a.vbs",
        "a.hta",
        "a.url",
        "a.appref-ms",
    ] {
        let r = write(&[t.join(name)], Box::new(PlainDisk), &[], b"echo hi");
        assert!(matches!(r, Err(FileError::Refused(_))), "{name}: {r:?}");
    }
    assert!(names(t.path()).is_empty(), "{:?}", names(t.path()));
}

/// The 8.3 short name of `path`, when the volume makes them.
fn short_name(path: &Path) -> Option<PathBuf> {
    let out = std::process::Command::new("cmd")
        .arg("/C")
        .arg(format!("for %I in (\"{}\") do @echo %~sI", path.display()))
        .output()
        .ok()?;
    let short = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    (short != path && short.exists()).then_some(short)
}

#[test]
fn an_existing_program_is_refused_by_its_short_name_too() {
    let t = TempDir::new("p-short");
    let long = t.join("a.settingcontent-ms");
    fs::write(&long, "keep").expect("written");
    let Some(short) = short_name(&long) else {
        eprintln!("no short names on this volume; skipped");
        return;
    };
    let r = write(&[short], Box::new(PlainDisk), &[], b"x");
    assert!(matches!(r, Err(FileError::Refused(_))), "{r:?}");
    assert_eq!(fs::read(&long).expect("read"), b"keep");
}

#[test]
fn pikslars_own_folders_are_refused_by_any_name() {
    let t = TempDir::new("p-own");
    let settings = t.join("home").join("Pikslar");
    fs::create_dir_all(settings.join("plugins").join("x")).expect("folders");
    fs::write(settings.join("settings.txt"), "keep").expect("written");
    let protect = [settings.clone(), t.join("missing")];
    for target in [
        settings.join("settings.txt"),
        settings.join("new.txt"),
        settings.join("plugins").join("x").join("plugin.json"),
    ] {
        let r = write(
            std::slice::from_ref(&target),
            Box::new(PlainDisk),
            &protect,
            b"x",
        );
        assert!(matches!(r, Err(FileError::Refused(_))), "{target:?}: {r:?}");
    }
    assert_eq!(
        fs::read(settings.join("settings.txt")).expect("read"),
        b"keep"
    );
    assert_eq!(names(&settings), ["plugins", "settings.txt"]);
    // A junction to a folder above them leads in by another name, and is refused too.
    let link = t.join("link");
    if junction(&link, &t.join("home")) {
        let r = write(
            &[link.join("Pikslar").join("x.txt")],
            Box::new(PlainDisk),
            &protect,
            b"x",
        );
        assert!(matches!(r, Err(FileError::Refused(_))), "{r:?}");
        assert_eq!(names(&settings), ["plugins", "settings.txt"]);
    }
    // Anywhere else is fine.
    write(
        &[t.join("home").join("out.txt")],
        Box::new(PlainDisk),
        &protect,
        b"x",
    )
    .expect("written");
}

/// Counts the pieces written.
struct Counting(Arc<AtomicUsize>);

impl Disk for Counting {
    fn fill(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        self.0.fetch_add(1, Ordering::Relaxed);
        PlainDisk.fill(file, bytes)
    }

    fn replace(&mut self, target: &Path, fresh: &Path, backup: &Path) -> io::Result<()> {
        PlainDisk.replace(target, fresh, backup)
    }

    fn place_new(&mut self, fresh: &Path, target: &Path) -> io::Result<()> {
        PlainDisk.place_new(fresh, target)
    }

    fn put_back(&mut self, backup: &Path, target: &Path) -> io::Result<()> {
        PlainDisk.put_back(backup, target)
    }
}

#[test]
fn a_cancel_is_seen_between_pieces_of_a_large_file() {
    let t = TempDir::new("p-pieces");
    let pieces = Arc::new(AtomicUsize::new(0));
    let target = t.join("out.txt");
    let (mut grants, handles) = grants(
        std::slice::from_ref(&target),
        Box::new(Counting(pieces.clone())),
        &[],
    );
    let bytes = vec![b'a'; 5 << 20];
    grants
        .claim_write(handles[0], bytes.len() as u64)
        .expect("claimed");
    let asked = Cell::new(0);
    let go_on = || {
        asked.set(asked.get() + 1);
        asked.get() < 3
    };
    assert_eq!(
        grants.commit(&[(handles[0], &bytes)], &go_on),
        Err(FileError::Cancelled)
    );
    let written = pieces.load(Ordering::Relaxed);
    assert!(written < 5, "{written} pieces written");
    assert!(names(t.path()).is_empty(), "{:?}", names(t.path()));
}
