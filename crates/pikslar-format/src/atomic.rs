//! Replacing a file so that a failure at any moment leaves the old one whole.

use crate::FormatError;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// Added to a file's name while its replacement is being written. Distinctive, because a save
/// removes whatever already has its temporary name (an export never does; see `run_temp_path`).
pub const TEMP_SUFFIX: &str = ".pikslar-tmp";

/// Hex digits in the tag an export run puts in its temporary names.
pub const RUN_TAG_LEN: usize = 8;

/// What a run's temporary name adds to a file's name: a dot, the tag and `TEMP_SUFFIX`.
pub const RUN_TEMP_EXTRA: usize = 1 + RUN_TAG_LEN + TEMP_SUFFIX.len();

/// A moment during a save at which a test can make things go wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveStep {
    Created,
    HalfWritten,
    Written,
    Synced,
    Verified,
}

impl SaveStep {
    pub const ALL: [SaveStep; 5] = [
        SaveStep::Created,
        SaveStep::HalfWritten,
        SaveStep::Written,
        SaveStep::Synced,
        SaveStep::Verified,
    ];
}

/// Called at each step. An error abandons the save.
pub type StepCheck<'a> = &'a mut dyn FnMut(SaveStep, &Path) -> io::Result<()>;

/// The file a replacement is written to before it takes the place of `path`.
pub fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(TEMP_SUFFIX);
    PathBuf::from(name)
}

/// The temporary name one export run gives `path`. The run's `tag` makes the name its own, so
/// a file another run left, or anyone's file, is not at it, and the run only ever creates it new.
pub fn run_temp_path(path: &Path, tag: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(tag);
    name.push(TEMP_SUFFIX);
    PathBuf::from(name)
}

/// Compares in pieces, so that checking a large save needs no second copy in memory.
pub(crate) fn matches_on_disk(path: &Path, expected: &[u8]) -> io::Result<bool> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    matches_file(&mut file, len, expected)
}

/// [`matches_on_disk`] for a file already open, `len` bytes long.
pub(crate) fn matches_file(file: &mut File, len: u64, expected: &[u8]) -> io::Result<bool> {
    if len != expected.len() as u64 {
        return Ok(false);
    }
    let mut buffer = vec![0u8; 1 << 16];
    for piece in expected.chunks(buffer.len()) {
        let read = &mut buffer[..piece.len()];
        file.read_exact(read)?;
        if read != piece {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A new, empty file at `temp`. Whatever had the name before is unlinked rather than emptied,
/// so a file hard-linked there, or one left by an interrupted save, is never written through.
pub(crate) fn fresh_file(temp: &Path) -> io::Result<File> {
    match fs::remove_file(temp) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)
}

/// A file made new at `temp`, never one that was there: whatever is at the name already stays
/// as it is, and the write stops.
pub(crate) fn new_file(temp: &Path) -> io::Result<File> {
    let made = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp);
    made.map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{} is in the way, and is kept. Another export to the same file may be \
                 running. If none is, it is left from an interrupted export: delete it",
                temp.display()
            ),
        ),
        _ => e,
    })
}

/// Runs `rest` on a temporary file just made; if it fails, the file goes, since this write
/// made it.
pub(crate) fn or_remove(temp: &Path, rest: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    let done = rest();
    if done.is_err() {
        let _ = fs::remove_file(temp);
    }
    done
}

/// Reaches the disk and reads back correctly before anything relies on it. Without the sync, a
/// power cut after the rename could replace a good file with an empty one.
pub(crate) fn write_checked(temp: &Path, bytes: &[u8], at: StepCheck<'_>) -> io::Result<()> {
    write_into(fresh_file(temp)?, temp, bytes, at)
}

/// [`write_checked`] into a file made new at `temp`: whatever is at the name stays.
pub(crate) fn write_checked_new(temp: &Path, bytes: &[u8], at: StepCheck<'_>) -> io::Result<()> {
    write_into(new_file(temp)?, temp, bytes, at)
}

fn write_into(mut file: File, temp: &Path, bytes: &[u8], at: StepCheck<'_>) -> io::Result<()> {
    or_remove(temp, || {
        at(SaveStep::Created, temp)?;
        let (first, rest) = bytes.split_at(bytes.len() / 2);
        file.write_all(first)?;
        at(SaveStep::HalfWritten, temp)?;
        file.write_all(rest)?;
        at(SaveStep::Written, temp)?;
        file.sync_all()?;
        drop(file);
        at(SaveStep::Synced, temp)?;
        if !matches_on_disk(temp, bytes)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "what was written did not read back correctly",
            ));
        }
        at(SaveStep::Verified, temp)
    })
}

/// A read-only file is never replaced: the person marked it so for a reason. Windows refuses a
/// rename or `ReplaceFileW` over one too (error 5, measured in `docs/b1-spikes-save.md`), but
/// only as "access denied", which reads like a file in use; this says why.
pub(crate) fn refuse_read_only(path: &Path) -> io::Result<()> {
    match fs::metadata(path) {
        Ok(found) if found.permissions().readonly() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the file is read-only",
        )),
        _ => Ok(()),
    }
}

/// [`write_atomic`], stopping at each step to ask whether to go on.
pub fn write_atomic_checked(
    path: &Path,
    bytes: &[u8],
    at: StepCheck<'_>,
) -> Result<(), FormatError> {
    refuse_read_only(path)?;
    let temp = temp_path(path);
    crate::room::check_room_beside(path, &temp)?;
    write_checked(&temp, bytes, at)?;
    if let Err(e) = fs::rename(&temp, path) {
        // The save already failed, and the temporary file is this save's own.
        let _ = fs::remove_file(&temp);
        return Err(e.into());
    }
    Ok(())
}

/// Writes beside the target, proves the bytes arrived, then renames over it, so a failed save
/// never damages the existing file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), FormatError> {
    write_atomic_checked(path, bytes, &mut |_, _| Ok(()))
}

/// Files that belong together, such as a sheet and its description, could not all be written.
#[derive(Debug)]
pub struct SetError {
    /// Targets already replaced or removed when the failure came. Empty unless a rename or a
    /// removal failed part way.
    pub replaced: Vec<PathBuf>,
    pub cause: FormatError,
}

impl std::fmt::Display for SetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.cause.fmt(f)?;
        if !self.replaced.is_empty() {
            let names: Vec<String> = self
                .replaced
                .iter()
                .map(|p| p.display().to_string())
                .collect();
            write!(f, ". Already replaced: {}", names.join(", "))?;
        }
        Ok(())
    }
}

impl std::error::Error for SetError {}

fn set_error(cause: impl Into<FormatError>) -> SetError {
    SetError {
        replaced: Vec::new(),
        cause: cause.into(),
    }
}

/// Writes every file beside its target and checks it before the first rename, so any failure
/// up to then changes no target. A rename can still fail part way; the error says which
/// targets were replaced by then.
pub fn write_set_atomic(files: &[(&Path, &[u8])]) -> Result<(), SetError> {
    write_set_atomic_checked(files, &mut |_, _| Ok(()))
}

/// [`write_set_atomic`], stopping at each step of each file to ask whether to go on.
pub fn write_set_atomic_checked(
    files: &[(&Path, &[u8])],
    at: StepCheck<'_>,
) -> Result<(), SetError> {
    let paths: Vec<&Path> = files.iter().map(|(path, _)| *path).collect();
    let temps: Vec<PathBuf> = paths.iter().map(|path| temp_path(path)).collect();
    set_with(
        &paths,
        &temps,
        &[],
        |index, temp| write_checked(temp, files[index].1, at),
        &mut || Ok(()),
    )
}

/// The protocol every set follows. Each target is refused if read-only, then `write` puts each
/// file at its temporary name in `temps` and proves it arrived. Only then are the `retire` files
/// removed and the temporary files renamed, in order. Any failure before that changes no target.
/// `ready` is asked last, once everything is written, whether the targets may be replaced now.
///
/// On a failure only the temporary files this set made are removed: a `write` that fails removes
/// what it made itself, and a member never reached is not looked at.
pub(crate) fn set_with(
    paths: &[&Path],
    temps: &[PathBuf],
    retire: &[&Path],
    mut write: impl FnMut(usize, &Path) -> io::Result<()>,
    ready: &mut dyn FnMut() -> io::Result<()>,
) -> Result<(), SetError> {
    check_members(paths, temps)?;
    let mut made = 0;
    let written = (0..paths.len()).try_for_each(|index| {
        write(index, &temps[index])?;
        made = index + 1;
        Ok(())
    });
    if let Err(e) = written.and_then(|()| ready()) {
        remove_all(&temps[..made]);
        return Err(set_error(e));
    }
    replace_all(paths, temps, retire)
}

/// A set names each file once, has a temporary name for each, and replaces no read-only file.
fn check_members(paths: &[&Path], temps: &[PathBuf]) -> Result<(), SetError> {
    if temps.len() != paths.len() {
        let wrong = io::Error::new(io::ErrorKind::InvalidInput, "a file has no temporary name");
        return Err(set_error(wrong));
    }
    let mut seen = std::collections::HashSet::new();
    for (path, temp) in paths.iter().zip(temps) {
        crate::room::check_room_beside(path, temp).map_err(set_error)?;
        if !seen.insert(*path) {
            let twice = io::Error::new(io::ErrorKind::InvalidInput, "one file was named twice");
            return Err(set_error(twice));
        }
        refuse_read_only(path).map_err(set_error)?;
    }
    Ok(())
}

/// Removes `retire`, then renames each temporary file over its target. On a failure the
/// temporary files not yet renamed are removed, and the error says what was replaced.
fn replace_all(paths: &[&Path], temps: &[PathBuf], retire: &[&Path]) -> Result<(), SetError> {
    let mut replaced = Vec::new();
    for path in retire {
        if let Err(e) = remove_if_there(path) {
            remove_all(temps);
            return Err(SetError {
                replaced,
                cause: e.into(),
            });
        }
        replaced.push(path.to_path_buf());
    }
    for (index, (path, temp)) in paths.iter().zip(temps).enumerate() {
        if let Err(e) = fs::rename(temp, path) {
            remove_all(&temps[index..]);
            return Err(SetError {
                replaced,
                cause: e.into(),
            });
        }
        replaced.push(path.to_path_buf());
    }
    Ok(())
}

fn remove_if_there(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The set already failed, and these temporary files are its own. One left behind is the
/// lesser problem.
fn remove_all(temps: &[PathBuf]) {
    for temp in temps {
        let _ = fs::remove_file(temp);
    }
}

#[cfg(test)]
mod set_tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pikslar-set-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).expect("temp dir");
        d
    }

    fn no_temps(d: &Path) -> bool {
        fs::read_dir(d).expect("list").all(|e| {
            !e.expect("entry")
                .file_name()
                .to_string_lossy()
                .ends_with(TEMP_SUFFIX)
        })
    }

    #[test]
    fn a_file_linked_at_the_temporary_name_keeps_its_bytes() {
        let d = dir("linked");
        let kept = d.join("kept.piks");
        fs::write(&kept, b"the project").expect("write");
        let target = d.join("out.png");
        fs::hard_link(&kept, temp_path(&target)).expect("link");
        write_atomic(&target, b"new").expect("save");
        write_set_atomic(&[(&target, b"newer")]).expect("set");
        assert_eq!(fs::read(&kept).expect("read"), b"the project");
        assert_eq!(fs::read(&target).expect("read"), b"newer");
        assert!(no_temps(&d));
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_set_is_written_whole() {
        let d = dir("whole");
        let (a, b) = (d.join("a.png"), d.join("a.json"));
        fs::write(&a, b"old").expect("old");
        write_set_atomic(&[(&a, b"image"), (&b, b"text")]).expect("written");
        assert_eq!(fs::read(&a).expect("a"), b"image");
        assert_eq!(fs::read(&b).expect("b"), b"text");
        assert!(no_temps(&d));
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_failure_before_the_renames_replaces_nothing() {
        let d = dir("early");
        let (a, b) = (d.join("a.png"), d.join("a.json"));
        fs::write(&a, b"old a").expect("old");
        fs::write(&b, b"old b").expect("old");
        for step in SaveStep::ALL {
            let mut fail = |at: SaveStep, temp: &Path| {
                if at == step && temp == temp_path(&b) {
                    return Err(io::Error::other("planted"));
                }
                Ok(())
            };
            let err = write_set_atomic_checked(&[(&a, b"new a"), (&b, b"new b")], &mut fail)
                .expect_err("fails");
            assert!(err.replaced.is_empty(), "{step:?}");
            assert_eq!(fs::read(&a).expect("a"), b"old a", "{step:?}");
            assert_eq!(fs::read(&b).expect("b"), b"old b", "{step:?}");
            assert!(no_temps(&d), "{step:?}");
        }
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_read_only_member_stops_the_set_before_anything_is_written() {
        let d = dir("readonly");
        let (a, b) = (d.join("a.png"), d.join("a.json"));
        fs::write(&b, b"keep").expect("old");
        let mut perms = fs::metadata(&b).expect("meta").permissions();
        perms.set_readonly(true);
        fs::set_permissions(&b, perms.clone()).expect("read-only");
        assert!(write_set_atomic(&[(&a, b"x"), (&b, b"y")]).is_err());
        assert!(!a.exists());
        assert_eq!(fs::read(&b).expect("b"), b"keep");
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        fs::set_permissions(&b, perms).expect("writable");
        assert!(no_temps(&d));
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn the_same_file_twice_is_refused() {
        let d = dir("twice");
        let a = d.join("a.png");
        assert!(write_set_atomic(&[(&a, b"x"), (&a, b"y")]).is_err());
        assert!(!a.exists());
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_rename_that_fails_part_way_names_what_was_replaced() {
        let d = dir("late");
        let (a, b) = (d.join("a.png"), d.join("a.json"));
        fs::create_dir(&b).expect("a folder where a file should go");
        let err = write_set_atomic(&[(&a, b"x"), (&b, b"y")]).expect_err("fails");
        assert_eq!(err.replaced, vec![a.clone()]);
        assert!(err.to_string().contains("a.png"), "{err}");
        assert!(no_temps(&d));
        fs::remove_dir_all(d).expect("cleanup");
    }
}
