//! One writer at a time for a set of files. Two exports to the same place would otherwise
//! rename each other's temporary files into place, and a description could end up beside
//! frames it does not describe.
//!
//! The lock lives in the user's temporary folder, not beside the set: a file made and removed
//! in the export's own folder slowed the export of an 8192 by 8192 sheet by a tenth on Windows.
//!
//! The lock also says which temporary files its export makes, before it makes them, each with
//! the run's own tag in its name. An export that stopped part way leaves its lock and some of
//! those files. The next export to the same set takes the lock over and removes exactly the
//! files the lock names, as they are still the stopped export's own; nothing else is touched.

use crate::atomic::{run_temp_path, RUN_TAG_LEN, TEMP_SUFFIX};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};

/// What a lock file says, so one left by an export that stopped can be told from anyone's file.
const MARK: &[u8] = b"Pikslar is writing an export. Delete this file if no export is.\n";

/// Before the run's tag on the line after the mark.
const RUN: &str = "run ";

/// The longest lock a run writes: the mark, its tag, and a path for each of at most 4,097 files.
const LONGEST: u64 = 16 << 20;

/// Windows' ERROR_SHARING_VIOLATION: another handle refuses to share the file.
const SHARING_VIOLATION: i32 = 32;

/// The lock for the set whose last file is `target`, named by its folder, resolved, and its stem,
/// with letters in either case taken as the same, as Windows takes them.
pub fn lock_path(target: &Path) -> PathBuf {
    let folder = match target.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    };
    let folder = fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    let stem = target.file_stem().unwrap_or_default();
    let key = folder.join(stem).to_string_lossy().to_lowercase();
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    std::env::temp_dir().join(format!("pikslar-export-{hash:016x}.lock"))
}

/// Held while a set is written. Dropping it removes the file.
#[derive(Debug)]
pub struct SetLock {
    path: PathBuf,
    file: Option<File>,
    tag: String,
    /// Temporary files of an export that stopped, removed when the lock was taken over.
    swept: Vec<PathBuf>,
}

impl SetLock {
    /// Takes the lock for the set whose last file is `target`. Refused while another export
    /// holds it, and when a file at its name is not one Pikslar left, which is then kept.
    ///
    /// On Windows the lock is the open file itself, shared with no one, so it ends when the
    /// process does, however it ends. Elsewhere only a new lock file excludes another export.
    pub fn take(target: &Path) -> io::Result<SetLock> {
        let path = lock_path(target);
        let tag = run_tag(&path);
        let (file, swept) = match open(&path, true) {
            Ok(file) => (marked(file, &path, &tag)?, Vec::new()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => left_behind(&path, &tag)?,
            Err(e) => return Err(busy_or(e, &path)),
        };
        Ok(SetLock {
            path,
            file: Some(file),
            tag,
            swept,
        })
    }

    /// The temporary name this run gives `target`.
    pub fn temp_for(&self, target: &Path) -> PathBuf {
        run_temp_path(target, &self.tag)
    }

    /// Records `temps` as this run's, on disk, before any of them is made.
    pub fn note(&mut self, temps: &[PathBuf]) -> io::Result<()> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        let mut lines = String::new();
        for temp in temps {
            // A name that is not text cannot be read back exactly, so it is not recorded, and
            // is never removed for this run: a leftover is the lesser problem.
            if let Some(text) = temp.to_str().filter(|t| !t.contains('\n')) {
                lines.push_str(text);
                lines.push('\n');
            }
        }
        file.seek(io::SeekFrom::End(0))?;
        file.write_all(lines.as_bytes())?;
        file.sync_data()
    }

    /// Temporary files a stopped export left, removed when this lock was taken over.
    pub fn swept(&self) -> &[PathBuf] {
        &self.swept
    }
}

impl Drop for SetLock {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

fn open(path: &Path, new: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(new);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(path)
}

/// A tag for this run's temporary names: from the time, the process and the lock, so two runs
/// do not share one. It names files only and is never written into an export.
fn run_tag(lock: &Path) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static RUNS: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let seed = format!(
        "{now}/{}/{}/{}",
        std::process::id(),
        RUNS.fetch_add(1, Ordering::Relaxed),
        lock.display()
    );
    let hash = seed.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{:0width$x}", hash >> 32, width = RUN_TAG_LEN)
}

/// Writes the mark and the run's tag into a lock just made or just taken over.
fn marked(mut file: File, path: &Path, tag: &str) -> io::Result<File> {
    let text = [MARK, RUN.as_bytes(), tag.as_bytes(), b"\n"].concat();
    let written = file
        .set_len(0)
        .and_then(|()| file.seek(io::SeekFrom::Start(0)))
        .and_then(|_| file.write_all(&text))
        .and_then(|()| file.sync_data());
    if let Err(e) = written {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(e);
    }
    Ok(file)
}

fn in_the_way(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "{} is in the way: Pikslar keeps that name for an export in progress",
            path.display()
        ),
    )
}

/// The temporary files a lock says its run makes, if it is one a run wrote: the mark alone (an
/// earlier version, or a run stopped before its tag), or the mark, the run's tag, and paths
/// whose names end with that tag. Anything else is someone's file.
fn recorded(text: &[u8]) -> Option<Vec<PathBuf>> {
    let rest = text.strip_prefix(MARK)?;
    if rest.is_empty() {
        return Some(Vec::new());
    }
    let rest = std::str::from_utf8(rest).ok()?;
    let mut lines = rest.strip_suffix('\n')?.split('\n');
    let tag = lines.next()?.strip_prefix(RUN)?;
    let hex = tag.len() == RUN_TAG_LEN && tag.bytes().all(|b| b.is_ascii_hexdigit());
    if !hex {
        return None;
    }
    let ending = format!(".{tag}{TEMP_SUFFIX}");
    lines
        .map(|line| line.ends_with(&ending).then(|| PathBuf::from(line)))
        .collect()
}

/// Removes what a stopped run made: each recorded path that is a plain file now. A folder or a
/// link at such a name was not made by the run, and is kept.
fn sweep(recorded: Vec<PathBuf>) -> Vec<PathBuf> {
    recorded
        .into_iter()
        .filter(|temp| fs::symlink_metadata(temp).is_ok_and(|m| m.file_type().is_file()))
        .filter(|temp| fs::remove_file(temp).is_ok())
        .collect()
}

/// A lock an export left when it stopped without finishing. Nothing holds it now and it is in
/// Pikslar's words, so it is taken over, and the temporary files it names are removed.
/// Anything else at its name is someone's file.
fn left_behind(path: &Path, tag: &str) -> io::Result<(File, Vec<PathBuf>)> {
    let mut file = open(path, false).map_err(|e| busy_or(e, path))?;
    let mut text = Vec::new();
    Read::by_ref(&mut file)
        .take(LONGEST + 1)
        .read_to_end(&mut text)?;
    let temps = (text.len() as u64 <= LONGEST)
        .then(|| recorded(&text))
        .flatten()
        .ok_or_else(|| in_the_way(path))?;
    let swept = sweep(temps);
    Ok((marked(file, path, tag)?, swept))
}

fn busy_or(e: io::Error, path: &Path) -> io::Error {
    if e.raw_os_error() == Some(SHARING_VIOLATION) {
        return io::Error::new(
            io::ErrorKind::ResourceBusy,
            format!("another export is writing here: {} is held", path.display()),
        );
    }
    e
}

/// Whether `path` can be replaced now: on Windows a file another program holds open without
/// sharing it cannot be, and finding that out before the first rename leaves every target as
/// it was. Anything else a rename would meet is left for the rename to report.
pub fn free_to_replace(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const DELETE: u32 = 0x0001_0000;
        const SHARE_ALL: u32 = 0x7;
        let probe = OpenOptions::new()
            .access_mode(DELETE)
            .share_mode(SHARE_ALL)
            .open(path);
        if let Err(e) = probe {
            if e.raw_os_error() == Some(SHARING_VIOLATION) {
                return Err(io::Error::new(
                    io::ErrorKind::ResourceBusy,
                    format!(
                        "{} is open in another program, so it cannot be replaced",
                        path.display()
                    ),
                ));
            }
        }
    }
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pikslar-lock-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).expect("temp dir");
        d
    }

    #[test]
    fn a_lock_is_removed_when_dropped() {
        let d = dir("drop");
        let target = d.join("walk.json");
        let lock = SetLock::take(&target).expect("taken");
        assert!(lock_path(&target).exists());
        drop(lock);
        assert!(!lock_path(&target).exists());
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[cfg(windows)]
    #[test]
    fn a_second_export_is_refused_while_the_first_holds_the_lock() {
        let d = dir("held");
        let target = d.join("walk.json");
        let first = SetLock::take(&target).expect("taken");
        let err = SetLock::take(&target).expect_err("held");
        assert_eq!(err.kind(), io::ErrorKind::ResourceBusy, "{err}");
        assert!(err.to_string().contains("another export"), "{err}");
        drop(first);
        drop(SetLock::take(&target).expect("free again"));
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_lock_an_export_left_behind_is_taken_over() {
        let d = dir("left");
        let target = d.join("walk.json");
        fs::write(lock_path(&target), MARK).expect("left by a stopped export");
        drop(SetLock::take(&target).expect("taken over"));
        assert!(!lock_path(&target).exists());
        fs::remove_dir_all(d).expect("cleanup");
    }

    /// What an export killed now leaves: its lock file, closed, on disk.
    fn killed(mut lock: SetLock) -> Vec<u8> {
        drop(lock.file.take());
        let text = fs::read(&lock.path).expect("left on disk");
        std::mem::forget(lock);
        text
    }

    fn tag_of(lock: &SetLock, target: &Path) -> String {
        let name = lock
            .temp_for(target)
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let tagged = name.strip_suffix(TEMP_SUFFIX).expect("suffix");
        tagged[tagged.len() - RUN_TAG_LEN..].to_string()
    }

    #[test]
    fn a_lock_says_its_run_and_the_files_it_will_make() {
        let d = dir("note");
        let target = d.join("walk.json");
        let mut lock = SetLock::take(&target).expect("taken");
        let tag = tag_of(&lock, &target);
        assert!(tag.bytes().all(|b| b.is_ascii_hexdigit()), "{tag}");
        let temp = lock.temp_for(&target);
        assert_eq!(temp, d.join(format!("walk.json.{tag}{TEMP_SUFFIX}")));
        let other = SetLock::take(&d.join("run.json")).expect("another set");
        assert_ne!(other.temp_for(&target), temp, "runs differ");
        lock.note(std::slice::from_ref(&temp)).expect("noted");
        let text = killed(lock);
        assert!(text.starts_with(MARK));
        assert_eq!(recorded(&text), Some(vec![temp]));
        let again = SetLock::take(&target).expect("taken over");
        assert!(again.swept().is_empty(), "the file was never made");
        assert_ne!(tag_of(&again, &target), tag, "a new run");
        drop(again);
        assert!(!lock_path(&target).exists());
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn taking_over_removes_only_what_the_stopped_run_recorded() {
        let d = dir("sweep");
        let target = d.join("walk.json");
        let tag = "0123abcd";
        let made = [
            d.join(format!("walk_0001.png.{tag}{TEMP_SUFFIX}")),
            d.join(format!("walk.json.{tag}{TEMP_SUFFIX}")),
        ];
        let never_made = d.join(format!("walk_0002.png.{tag}{TEMP_SUFFIX}"));
        let folder = d.join(format!("walk_0003.png.{tag}{TEMP_SUFFIX}"));
        let unrecorded = d.join(format!("walk_0004.png.{tag}{TEMP_SUFFIX}"));
        let old_style = d.join(format!("walk.json{TEMP_SUFFIX}"));
        for f in made.iter().chain([&unrecorded, &old_style]) {
            fs::write(f, "x").expect("file");
        }
        fs::create_dir(&folder).expect("folder");
        let mut text = [MARK, format!("{RUN}{tag}\n").as_bytes()].concat();
        for p in made.iter().chain([&never_made, &folder]) {
            text.extend_from_slice(format!("{}\n", p.display()).as_bytes());
        }
        fs::write(lock_path(&target), &text).expect("left by a stopped export");
        let lock = SetLock::take(&target).expect("taken over");
        assert_eq!(lock.swept(), &made[..]);
        assert!(made.iter().all(|f| !f.exists()));
        assert!(folder.is_dir() && unrecorded.exists() && old_style.exists());
        assert_ne!(tag_of(&lock, &target), tag, "a new run");
        let text = killed(lock);
        assert_eq!(recorded(&text), Some(Vec::new()), "the old records went");
        drop(SetLock::take(&target).expect("taken over again"));
        fs::remove_dir_all(d).expect("cleanup");
    }

    /// Needs the right to make links: on Windows, administrator or Developer Mode. Without it
    /// the test cannot set up its case and says so rather than failing.
    #[test]
    fn a_link_at_a_recorded_name_is_kept() {
        let d = dir("link");
        let target = d.join("walk.json");
        let theirs = d.join("theirs.png");
        fs::write(&theirs, "keep").expect("file");
        let link = d.join(format!("walk.json.0123abcd{TEMP_SUFFIX}"));
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&theirs, &link);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&theirs, &link);
        if made.is_err() {
            eprintln!("skipped: links cannot be made here");
            fs::remove_dir_all(d).expect("cleanup");
            return;
        }
        let text = [
            MARK,
            format!("{RUN}0123abcd\n{}\n", link.display()).as_bytes(),
        ]
        .concat();
        fs::write(lock_path(&target), &text).expect("left by a stopped export");
        let lock = SetLock::take(&target).expect("taken over");
        assert!(lock.swept().is_empty());
        assert!(fs::symlink_metadata(&link).is_ok(), "the link is kept");
        drop(lock);
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn a_lock_naming_files_outside_its_run_is_someones_and_kept() {
        let d = dir("forged");
        let target = d.join("walk.json");
        let precious = d.join("precious.png");
        fs::write(&precious, "keep").expect("file");
        let cases = [
            (format!("{RUN}0123abcd\n"), true),
            (format!("{RUN}0123abcd\n{}\n", precious.display()), false),
            (format!("{RUN}xyz\n"), false),
            (
                format!(
                    "{RUN}0123abcd\n{}.0123abcd{TEMP_SUFFIX}",
                    precious.display()
                ),
                false,
            ),
        ];
        for (text, a_run) in cases {
            let full = [MARK, text.as_bytes()].concat();
            fs::write(lock_path(&target), &full).expect("left there");
            let taken = SetLock::take(&target);
            if a_run {
                drop(taken.expect("a run that recorded nothing yet"));
                continue;
            }
            let err = taken.expect_err("refused");
            assert!(err.to_string().contains("in the way"), "{err}");
            assert_eq!(fs::read(lock_path(&target)).expect("kept"), full);
        }
        assert_eq!(fs::read_to_string(&precious).expect("kept"), "keep");
        let _ = fs::remove_file(lock_path(&target));
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn someone_elses_file_at_the_lock_name_is_kept() {
        let d = dir("theirs");
        let target = d.join("walk.json");
        for theirs in [&b"notes"[..], b"", &[MARK, b"more"].concat()] {
            fs::write(lock_path(&target), theirs).expect("theirs");
            let err = SetLock::take(&target).expect_err("refused");
            assert!(err.to_string().contains("in the way"), "{err}");
            assert_eq!(fs::read(lock_path(&target)).expect("kept"), theirs);
        }
        fs::remove_file(lock_path(&target)).expect("cleanup");
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[test]
    fn one_place_has_one_lock_however_it_is_written() {
        let d = dir("names");
        let lock = lock_path(&d.join("walk.json"));
        assert_eq!(
            lock_path(&d.join("walk.png")),
            lock,
            "the set's files share it"
        );
        assert_eq!(lock_path(&d.join("WALK.json")), lock, "case is ignored");
        fs::create_dir(d.join("sub")).expect("folder");
        let roundabout = d.join("sub").join("..").join("walk.json");
        assert_eq!(lock_path(&roundabout), lock, "the folder is resolved");
        assert_ne!(lock_path(&d.join("run.json")), lock);
        assert_ne!(lock_path(&dir("names-other").join("walk.json")), lock);
        assert_eq!(lock.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(d).expect("cleanup");
    }

    #[cfg(windows)]
    #[test]
    fn a_file_held_without_sharing_is_found_before_any_rename() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = dir("busy");
        let path = d.join("walk_0003.png");
        fs::write(&path, "old").expect("write");
        free_to_replace(&path).expect("nothing holds it");
        free_to_replace(&d.join("missing.png")).expect("nothing there");
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0x1)
            .open(&path)
            .expect("held");
        let err = free_to_replace(&path).expect_err("held");
        assert!(err.to_string().contains("open in another program"), "{err}");
        drop(held);
        free_to_replace(&path).expect("free again");
        fs::remove_dir_all(d).expect("cleanup");
    }
}
