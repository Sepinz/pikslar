//! The store's folders: finding, reading, measuring and removing projects. Writing is in
//! [`crate::write`].

use crate::frame::{self, Head, HEAD_LEN};
use crate::{payload, RewindError};
use pikslar_core::OlderStep;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const HEAD_FILE: &str = "head";
pub const LOCK_FILE: &str = "lock";
const GONE: &str = "gone-";

/// Where Rewind keeps its projects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store {
    root: PathBuf,
}

/// One project folder as found on disk.
#[derive(Clone, Debug)]
pub struct Found {
    pub id: u128,
    pub bytes: u64,
    /// The head's last use, or when its newest file was written if the head does not read.
    pub used: u64,
}

impl Store {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    /// `%LOCALAPPDATA%\Pikslar\rewind`, beside settings and crash recovery.
    pub fn default_dir() -> Option<PathBuf> {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(PathBuf::from(base).join("Pikslar").join("rewind"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn project_dir(&self, id: u128) -> PathBuf {
        self.root.join(format!("{id:032x}"))
    }

    pub fn log_path(&self, id: u128, gen: u64) -> PathBuf {
        self.project_dir(id).join(log_name(gen))
    }

    /// The project's head, none if it has no history, or why it does not read.
    pub fn read_head(&self, id: u128) -> Result<Option<Head>, RewindError> {
        let path = self.project_dir(id).join(HEAD_FILE);
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if file.metadata()?.len() != HEAD_LEN as u64 {
            return Err(RewindError::Damaged("the Rewind head is damaged"));
        }
        let mut bytes = [0u8; HEAD_LEN];
        file.read_exact(&mut bytes)?;
        let head = Head::decode(&bytes)?;
        match head.id == id {
            true => Ok(Some(head)),
            false => Err(RewindError::Damaged("the Rewind head names another file")),
        }
    }

    /// True if anything is kept under this id, readable or not.
    pub fn has_history(&self, id: u128) -> bool {
        self.project_dir(id).join(HEAD_FILE).exists()
    }

    /// Reads the step at `pos`, which ends at `end` in the log `head` names. Returns it and
    /// where the step before it ends. A pixel step may name at most `room` tiles.
    pub fn read_step(
        &self,
        head: &Head,
        end: u64,
        pos: u64,
        room: usize,
    ) -> Result<(OlderStep, u64), RewindError> {
        if pos <= head.base || pos > head.tip {
            return Err(RewindError::Damaged("no stored step is kept there"));
        }
        let mut file = File::open(self.log_path(head.id, head.gen))?;
        let len = file.metadata()?.len();
        let mut start = [0u8; frame::LOG_START as usize];
        file.read_exact(&mut start)?;
        if start != frame::log_start(head.gen) {
            return Err(RewindError::Damaged("the Rewind log is damaged"));
        }
        let found = frame::read_record(&mut file, len, end, pos)?;
        if u64::from(found.head.decoded) > payload::most_decoded(found.head.kind, room) {
            return Err(RewindError::Damaged("a stored step does not read"));
        }
        let decoded = match found.head.flags & frame::DEFLATED {
            0 => found.stored,
            _ => inflate_exact(&found.stored, found.head.decoded)?,
        };
        let step = payload::decode(found.head.kind, &decoded, room)?;
        Ok((step, found.head.prev_end))
    }

    /// Holds the project for this program while it writes. Refused while another holds it.
    pub fn lock(&self, id: u128) -> Result<File, RewindError> {
        let dir = self.project_dir(id);
        real_dir(&self.root)?;
        real_dir(&dir)?;
        fs::create_dir_all(&dir)?;
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(false);
        #[cfg(windows)]
        std::os::windows::fs::OpenOptionsExt::share_mode(&mut options, 0);
        options
            .open(dir.join(LOCK_FILE))
            .map_err(|e| match e.raw_os_error() {
                // Windows: another process has the file open.
                Some(32) | Some(33) => RewindError::InUse,
                _ => e.into(),
            })
    }

    /// Every project folder, measured. Folders that are not projects are left out.
    pub fn projects(&self) -> Vec<Found> {
        if real_dir(&self.root).is_err() {
            return Vec::new();
        }
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let ids = entries.flatten().filter_map(|entry| {
            let real_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            real_dir.then(|| project_id(entry.file_name().to_str()?))?
        });
        ids.map(|id| self.measure(id)).collect()
    }

    /// A project's files: their lengths added up, and when it was last used.
    pub fn measure(&self, id: u128) -> Found {
        let (bytes, newest) = dir_size(&self.project_dir(id));
        let used = match self.read_head(id) {
            // A last use in the future, from a forged or copied head, counts as now.
            Ok(Some(head)) => head.used.min(crate::now_secs()),
            _ => newest,
        };
        Found { id, bytes, used }
    }

    /// The length of every project's files, added up. Nothing else in the store counts, as
    /// nothing else in it is ever removed to keep within the budget.
    pub fn total_bytes(&self) -> u64 {
        self.projects().iter().map(|p| p.bytes).sum()
    }

    /// Removes a project's history: renamed out of the way first, so a half-removed project
    /// never reads as sound. Refused while another Pikslar holds it.
    pub fn forget(&self, id: u128) -> Result<(), RewindError> {
        let dir = self.project_dir(id);
        if !dir.exists() {
            return Ok(());
        }
        let lock = self.lock(id)?;
        let gone = self
            .root
            .join(format!("{GONE}{id:032x}-{}", crate::now_secs()));
        // Windows cannot rename a folder while a file in it is open, the lock included, so it
        // is let go first. Another Pikslar that takes it in between keeps the folder from being
        // renamed (refused as in use); one that takes it after finds no head and, expecting one,
        // writes nothing.
        drop(lock);
        fs::rename(&dir, &gone).map_err(|e| match e.raw_os_error() {
            Some(5) | Some(32) => RewindError::InUse,
            _ => e.into(),
        })?;
        fs::remove_dir_all(&gone)?;
        Ok(())
    }

    /// Removes every project. Returns how many were in use elsewhere and kept.
    pub fn forget_all(&self) -> Result<usize, RewindError> {
        let mut kept = 0;
        for project in self.projects() {
            match self.forget(project.id) {
                Ok(()) => {}
                Err(RewindError::InUse) => kept += 1,
                Err(e) => return Err(e),
            }
        }
        self.sweep();
        Ok(kept)
    }

    /// Removes what an interrupted removal left.
    pub fn sweep(&self) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(GONE) {
                // Best effort: it is retried at the next sweep.
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

pub fn log_name(gen: u64) -> String {
    format!("log-{gen:016x}")
}

/// The lengths of the files directly in `dir` added up, and the newest one's time in Unix
/// seconds. Folders inside are not counted: a project never has any.
fn dir_size(dir: &Path) -> (u64, u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return (0, 0);
    };
    let metas = entries.flatten().filter_map(|e| e.metadata().ok());
    let files = metas.filter(|m| m.is_file());
    files.fold((0, 0), |(bytes, newest), m| {
        let at = m.modified().ok().and_then(|t| {
            let since = t.duration_since(std::time::UNIX_EPOCH).ok()?;
            Some(since.as_secs())
        });
        (bytes.saturating_add(m.len()), newest.max(at.unwrap_or(0)))
    })
}

/// Inflates to exactly `decoded` bytes. Refused before anything is inflated if that is more than
/// a stored payload can grow by.
fn inflate_exact(stored: &[u8], decoded: u32) -> Result<Vec<u8>, RewindError> {
    if u64::from(decoded) > stored.len() as u64 * frame::MOST_RATIO {
        return Err(RewindError::Damaged("a stored step does not read"));
    }
    let bytes = pikslar_format::inflate::inflate(stored, decoded as usize)?;
    match bytes.len() == decoded as usize {
        true => Ok(bytes),
        false => Err(RewindError::Damaged("a stored step does not read")),
    }
}

/// The id a project folder's name gives, only if it is the name the store gives that id.
fn project_id(name: &str) -> Option<u128> {
    let id = u128::from_str_radix(name, 16).ok()?;
    (name == format!("{id:032x}")).then_some(id)
}

/// True if `path` is a folder of its own, false if nothing is there. Refused if it is a link or
/// junction, or anything but a folder: the store never reads or writes through one.
fn real_dir(path: &Path) -> Result<bool, RewindError> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            Err(RewindError::Damaged("a Rewind folder is a link"))
        }
        Ok(m) if m.is_dir() => Ok(true),
        Ok(_) => Err(RewindError::Damaged("a Rewind folder is not a folder")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
