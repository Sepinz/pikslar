//! Writing the files a run chose to save as, once the run has succeeded.
//!
//! Every write a run asked for is put in place together, or none is:
//!
//! 1. **Staged.** For each, the chosen place is checked again: a plain absolute path, no
//!    stream, no device name, of a kind on the allow-list ([`crate::kinds`]). The folder it
//!    goes in (that folder alone: the folders above it are not checked one by one) is opened
//!    on its own name, refused if it is a link, junction or other redirecting reparse point
//!    ([`crate::reparse`]; a cloud placeholder is fine), and held open so it cannot be
//!    renamed or swapped meanwhile. Its real place, with any link above it resolved, must not
//!    be in or under a folder Pikslar keeps its own files in. The target must be absent or a
//!    plain, writable file whose real name is of an allowed kind too, and of the kind it was
//!    when chosen. The bytes go to a new temporary file beside it, made with `create_new`,
//!    written a piece at a time (a cancel is seen between pieces) and synced.
//! 2. **Placed.** Each target is checked once more, by kind and by identity, then replaced by
//!    its temporary file ([`Disk::replace`], which keeps the original as a backup beside it)
//!    or, if there was none, the temporary file is moved there without replacing anything.
//!
//! A failure anywhere takes back every write already tried ([`crate::rollback`]): each place
//! is looked at, the original put back from its backup where it is not there, and a backup
//! removed only when its place is seen to hold the original again. No error names a path.
//!
//! The checks are made on opened handles, but the replace and the move go by name: a program
//! running as the same user could still swap a file in the moment between them.

use crate::files::{FileError, MOST_FILE_BYTES};
use crate::identity::{self, Found};
use crate::kinds::is_allowed_name;
use crate::names::is_device;
use crate::rollback::{self, Staged, Tried};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Bytes written to the temporary file at a time; a cancel is seen between pieces.
pub(crate) const PIECE: usize = 1 << 20;

/// What was at the chosen place when the user chose it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Seen {
    Absent,
    File,
    /// A folder, link, device or anything else no write may go to.
    Unusable,
}

/// The few steps that touch the disk, so the platform can do them its own way and tests can
/// make them fail.
pub trait Disk: Send {
    /// Writes one piece of the bytes into the new temporary file.
    fn fill(&mut self, file: &mut File, bytes: &[u8]) -> io::Result<()> {
        file.write_all(bytes)
    }

    /// Makes what was written to the temporary file durable.
    fn sync(&mut self, file: &mut File) -> io::Result<()> {
        file.sync_all()
    }

    /// Puts `fresh` where `target` is. The original ends up at `backup` (a file Pikslar made
    /// for it, which may be replaced). On an error anything may have happened: the original
    /// still at `target`, at `backup`, or `fresh` in place after all. The caller looks.
    fn replace(&mut self, target: &Path, fresh: &Path, backup: &Path) -> io::Result<()>;

    /// Moves `fresh` to `target`, which does not exist; never replaces anything there.
    fn place_new(&mut self, fresh: &Path, target: &Path) -> io::Result<()>;

    /// Puts the original back from `backup` over `target`.
    fn put_back(&mut self, backup: &Path, target: &Path) -> io::Result<()>;
}

/// [`Disk`] with the standard library alone: a rename over the target (which gives the new
/// file its folder's permissions, not the original's) and a hard link for a new file. The
/// app uses the system's `ReplaceFileW` instead (`pikslar-platform`).
pub struct PlainDisk;

impl Disk for PlainDisk {
    fn replace(&mut self, target: &Path, fresh: &Path, backup: &Path) -> io::Result<()> {
        fs::rename(target, backup)?;
        // A failed move back leaves the original at the backup; the caller sees that.
        fs::rename(fresh, target).inspect_err(|_| {
            let _ = fs::rename(backup, target);
        })
    }

    fn place_new(&mut self, fresh: &Path, target: &Path) -> io::Result<()> {
        fs::hard_link(fresh, target)?;
        let _ = fs::remove_file(fresh);
        Ok(())
    }

    fn put_back(&mut self, backup: &Path, target: &Path) -> io::Result<()> {
        fs::rename(backup, target)
    }
}

/// One write a run asked for.
pub(crate) struct Wanted<'a> {
    pub target: &'a Path,
    pub seen: Seen,
    pub bytes: &'a [u8],
}

fn refused(why: &'static str) -> FileError {
    FileError::Refused(why)
}

fn plain_io(e: &io::Error) -> FileError {
    FileError::Io(format!("could not write the file: {}", e.kind()))
}

/// The chosen place as a path Pikslar writes to: absolute (a drive, a UNC path, or either in
/// the `\\?\` form), made only of plain names, its last one no stream (`name:stream`),
/// device (`nul.txt`), kind off the allow-list (`run.bat`), or name ending in a dot or space.
pub(crate) fn check_path(path: &Path) -> Result<(), FileError> {
    let not_plain = refused("the chosen place is not a plain file path");
    // Components drop a `.` in the middle, so the text is looked at as well.
    let text = path.to_str().ok_or_else(|| not_plain.clone())?;
    let dotted = text.split(['\\', '/']).any(|s| s == "." || s == "..");
    if dotted || !path.is_absolute() || !path.components().all(plain_component) {
        return Err(not_plain);
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| not_plain.clone())?;
    if name.contains(':') {
        return Err(refused("the chosen name holds a ':' (a stream)"));
    }
    if name.len() > 255 || name.ends_with(['.', ' ']) {
        return Err(not_plain);
    }
    check_kind(name)?;
    let stem = name.split('.').next().unwrap_or_default();
    match is_device(&stem.trim_end().to_ascii_lowercase()) {
        true => Err(refused("the chosen name is a device's")),
        false => Ok(()),
    }
}

fn check_kind(name: &str) -> Result<(), FileError> {
    match is_allowed_name(name) {
        true => Ok(()),
        false => Err(refused(
            "a plugin may only save plain data and picture kinds of file",
        )),
    }
}

#[cfg(windows)]
fn plain_component(c: Component<'_>) -> bool {
    use std::path::Prefix;
    match c {
        Component::Prefix(p) => matches!(
            p.kind(),
            Prefix::Disk(_) | Prefix::VerbatimDisk(_) | Prefix::UNC(..) | Prefix::VerbatimUNC(..)
        ),
        Component::RootDir | Component::Normal(_) => true,
        Component::CurDir | Component::ParentDir => false,
    }
}

#[cfg(not(windows))]
fn plain_component(c: Component<'_>) -> bool {
    matches!(c, Component::RootDir | Component::Normal(_))
}

/// What is at `path` now, without following a link.
pub(crate) fn look(path: &Path) -> Seen {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Seen::Absent,
        Ok(m) if m.is_file() && !crate::package::is_link(&m) => Seen::File,
        _ => Seen::Unusable,
    }
}

#[cfg(windows)]
mod handles {
    use std::fs::{File, OpenOptions};
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::Path;

    const READ_ATTRIBUTES: u32 = 0x80;
    const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const SHARE_READ_WRITE: u32 = 0x1 | 0x2;
    const SHARE_ALL: u32 = SHARE_READ_WRITE | 0x4;

    /// The thing at `path` itself, link or not, for its attributes. Folders open too.
    fn open_itself(path: &Path, share: u32) -> std::io::Result<File> {
        OpenOptions::new()
            .access_mode(READ_ATTRIBUTES)
            .share_mode(share)
            .custom_flags(OPEN_REPARSE_POINT | BACKUP_SEMANTICS)
            .open(path)
    }

    /// The folder, held so that nobody can rename, remove or replace it while it is open.
    #[inline(never)]
    pub fn hold_folder(path: &Path) -> std::io::Result<File> {
        open_itself(path, SHARE_READ_WRITE)
    }

    #[inline(never)]
    pub fn open_target(path: &Path) -> std::io::Result<File> {
        open_itself(path, SHARE_ALL)
    }
}

#[cfg(not(windows))]
mod handles {
    use std::fs::File;
    use std::path::Path;

    #[inline(never)]
    pub fn hold_folder(path: &Path) -> std::io::Result<File> {
        File::open(path)
    }

    #[inline(never)]
    pub fn open_target(path: &Path) -> std::io::Result<File> {
        match std::fs::symlink_metadata(path)?.file_type().is_symlink() {
            true => Err(std::io::Error::other("a link")),
            false => File::open(path),
        }
    }
}

/// The file or folder at `path` itself, a link not followed, opened only to look at it.
pub(crate) fn open_itself(path: &Path) -> io::Result<File> {
    handles::open_target(path)
}

/// The folder the target goes in, opened and held, checked on its handle: a real folder, and
/// not in or under any of `protected`.
fn hold_folder(target: &Path, protected: &[PathBuf]) -> Result<File, FileError> {
    let parent = target
        .parent()
        .ok_or(refused("the chosen place is not a plain file path"))?;
    let folder = handles::hold_folder(parent).map_err(|e| plain_io(&e))?;
    let meta = folder.metadata().map_err(|e| plain_io(&e))?;
    if crate::reparse::redirects(&folder).map_err(|e| plain_io(&e))? {
        return Err(refused("the chosen folder is a link or junction"));
    }
    if !meta.is_dir() {
        return Err(refused("the chosen place is not in a folder"));
    }
    let real = identity::final_path(&folder, parent).map_err(|e| plain_io(&e))?;
    let startup = crate::startup::folders();
    for own in protected.iter().chain(&startup) {
        match identity::final_folder(own) {
            Ok(None) => {}
            Ok(Some(own)) if !identity::is_within(&real, &own) => {}
            Ok(Some(_)) => {
                return Err(refused(
                    "the chosen folder is one Pikslar keeps its own files in",
                ))
            }
            Err(_) => return Err(refused("Pikslar's own folders could not be checked")),
        }
    }
    Ok(folder)
}

/// What the target is now, checked on an opened handle: absent, or a plain writable file
/// whose real name (not a short name standing for it) is of an allowed kind.
pub(crate) fn check_target(target: &Path) -> Result<Seen, FileError> {
    let file = match handles::open_target(target) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Seen::Absent),
        other => other.map_err(|e| plain_io(&e))?,
    };
    let meta = file.metadata().map_err(|e| plain_io(&e))?;
    if crate::reparse::redirects(&file).map_err(|e| plain_io(&e))? {
        return Err(refused("the chosen file is a link or junction"));
    }
    if !meta.is_file() {
        return Err(refused(
            "the chosen place is a folder or device, not a file",
        ));
    }
    if meta.permissions().readonly() {
        return Err(refused("the chosen file is read-only"));
    }
    let real = identity::final_path(&file, target).map_err(|e| plain_io(&e))?;
    let name = real.file_name().map(|n| n.to_string_lossy().into_owned());
    check_kind(&name.unwrap_or_default())?;
    Ok(Seen::File)
}

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A new, empty file of Pikslar's own beside `target`, never one already there.
fn new_beside(target: &Path, kind: &str) -> Result<(PathBuf, File), FileError> {
    let folder = target.parent().unwrap_or(Path::new(""));
    let mut last = None;
    for _ in 0..8 {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = folder.join(format!("~pikslar-{}-{n}.{kind}", std::process::id()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(e) => last = Some(e),
        }
    }
    let e = last.unwrap_or_else(|| io::Error::other("no name"));
    Err(plain_io(&e))
}

/// The bytes into the new temporary file, a piece at a time with `go_on` asked before each,
/// then synced.
fn fill(
    file: &mut File,
    bytes: &[u8],
    disk: &mut dyn Disk,
    go_on: &dyn Fn() -> bool,
) -> Result<(), FileError> {
    for piece in bytes.chunks(PIECE) {
        if !go_on() {
            return Err(FileError::Cancelled);
        }
        disk.fill(file, piece).map_err(|e| plain_io(&e))?;
    }
    disk.sync(file).map_err(|e| plain_io(&e))
}

/// What the target is now and, for a file, which one: refused when it is not what was
/// chosen.
fn check_seen(w: &Wanted<'_>) -> Result<(Seen, Option<identity::FileId>), FileError> {
    let now = check_target(w.target)?;
    match (w.seen, now) {
        (Seen::Unusable, _) => return Err(refused("the chosen place is not a plain file")),
        (Seen::Absent, Seen::File) => {
            return Err(refused(
                "a file appeared at the chosen place after it was chosen",
            ))
        }
        _ => {}
    }
    match (now, identity::found(w.target)) {
        (Seen::File, Found::Is(id)) => Ok((now, Some(id))),
        (Seen::File, _) => Err(refused("the chosen file could not be told apart")),
        _ => Ok((now, None)),
    }
}

fn stage<'a>(
    w: &Wanted<'a>,
    disk: &mut dyn Disk,
    protected: &[PathBuf],
    go_on: &dyn Fn() -> bool,
) -> Result<Staged<'a>, FileError> {
    if w.bytes.len() as u64 > MOST_FILE_BYTES {
        return Err(FileError::TooLarge);
    }
    check_path(w.target)?;
    let folder = hold_folder(w.target, protected)?;
    let (now, original) = check_seen(w)?;
    let (fresh, mut file) = new_beside(w.target, "tmp")?;
    let filled = identity::file_id(&file)
        .map_err(|e| plain_io(&e))
        .and_then(|id| fill(&mut file, w.bytes, disk, go_on).map(|()| id));
    drop(file);
    let backup = filled.and_then(|id| match now {
        Seen::File => new_beside(w.target, "bak").map(|(path, _)| (id, Some(path))),
        _ => Ok((id, None)),
    });
    let (fresh_id, backup) = backup.inspect_err(|_| {
        let _ = fs::remove_file(&fresh);
    })?;
    Ok(Staged {
        target: w.target,
        fresh,
        fresh_id,
        backup,
        original,
        now,
        tried: Tried::Not,
        _folder: folder,
    })
}

/// Puts every write in place, or none. `go_on` is asked before each file is staged, between
/// the pieces it is written in, and before any is placed, so a cancel during the writing
/// leaves every file as it was. No write goes in or under a folder in `protected`.
pub(crate) fn commit(
    wanted: &[Wanted<'_>],
    disk: &mut dyn Disk,
    protected: &[PathBuf],
    go_on: &dyn Fn() -> bool,
) -> Result<(), FileError> {
    let mut staged = Vec::with_capacity(wanted.len());
    for w in wanted {
        let next = match go_on() {
            true => stage(w, disk, protected, go_on),
            false => Err(FileError::Cancelled),
        };
        match next {
            Ok(s) => staged.push(s),
            Err(e) => return rollback::undo(&staged, disk).and(Err(e)),
        }
    }
    if !go_on() {
        return rollback::undo(&staged, disk).and(Err(FileError::Cancelled));
    }
    for i in 0..staged.len() {
        if let Err(e) = rollback::place(&mut staged[i], disk) {
            return rollback::undo(&staged, disk).and(Err(e));
        }
    }
    rollback::finish(&staged);
    Ok(())
}

#[cfg(test)]
#[path = "save_tests.rs"]
mod tests;
