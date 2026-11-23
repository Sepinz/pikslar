//! The plugins folder Pikslar owns: `plugins` inside the settings folder. One folder per
//! plugin, named by its id, holding the package's files and the install record.

use crate::capability::CapabilitySet;
use crate::error::PluginError;
use crate::install;
use crate::manifest::Manifest;
use crate::names::check_id;
use crate::package::{
    check_plain, content_hash, is_link, open_no_follow, read_flat_folder, Package,
};
use crate::record::{Record, RECORD_FILE};
use crate::review::{Change, InstallReview, PendingInstall, Snapshot};
use crate::sha256::{hex, sha256};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The folder inside the settings folder that holds plugins.
pub const PLUGINS_FOLDER: &str = "plugins";
/// The most folders [`PluginStore::list`] looks at.
pub const MOST_LISTED: usize = 256;

/// Why an installed plugin is disabled. A disabled plugin is never loaded in part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Disabled {
    NotInstalled,
    /// The folder name is not a plugin id, so Pikslar did not make it.
    NotAPlugin,
    /// The install record is missing: the install did not finish, or it was removed.
    NoRecord,
    BadRecord(String),
    /// The folder no longer holds a package Pikslar takes (an added, removed or unreadable
    /// file, or a manifest this Pikslar no longer supports).
    Unreadable(String),
    /// The files differ from what the user approved: an accidental or unexpected change,
    /// which the content hash detects. It does not say who changed them.
    Changed,
    /// The record and the files disagree about the id, version or capabilities.
    Mismatch(String),
}

impl fmt::Display for Disabled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Disabled::NotInstalled => f.write_str("it is not installed"),
            Disabled::NotAPlugin => f.write_str("this folder is not a plugin Pikslar installed"),
            Disabled::NoRecord => f.write_str("its install did not finish; install it again"),
            Disabled::BadRecord(why) => write!(f, "its install record is damaged: {why}"),
            Disabled::Unreadable(why) => write!(f, "its files are not usable: {why}"),
            Disabled::Changed => f.write_str(
                "its files changed after it was installed; review and install it again to use it",
            ),
            Disabled::Mismatch(why) => write!(f, "its install record does not match it: {why}"),
        }
    }
}

/// An installed plugin whose files match what the user approved, ready for the script host.
#[derive(Clone, Debug)]
pub struct LoadedPlugin {
    package: Package,
    granted: CapabilitySet,
    /// Lower-case hex SHA-256 of the files, checked against the install record at load.
    hash: String,
}

impl LoadedPlugin {
    pub fn manifest(&self) -> &Manifest {
        self.package.manifest()
    }

    /// What the user approved: every run of this plugin gets exactly these.
    pub fn granted(&self) -> CapabilitySet {
        self.granted
    }

    /// The files' content hash as [`crate::review::fingerprint`] shows it: for comparing
    /// with a published value, not a proof of who made the plugin.
    #[inline(never)]
    pub fn fingerprint(&self) -> String {
        crate::review::fingerprint(&self.hash)
    }

    #[inline(never)]
    pub fn script_for(&self, command_id: &str) -> Option<&str> {
        self.package.script_for(command_id)
    }
}

/// One folder of the store, as listed.
#[derive(Clone, Debug)]
pub struct Listed {
    /// The folder name: the plugin id, or whatever a stray folder is called.
    pub id: String,
    pub state: Result<LoadedPlugin, Disabled>,
}

/// Where plugins live, or nowhere when settings are off.
#[derive(Clone, Debug, Default)]
pub struct PluginStore {
    root: Option<PathBuf>,
}

impl PluginStore {
    /// Plugins are off: nothing is listed and nothing installs.
    pub fn disabled() -> PluginStore {
        PluginStore::default()
    }

    /// A store at `dir` itself.
    #[inline(never)]
    pub fn in_dir(dir: &Path) -> PluginStore {
        PluginStore {
            root: Some(dir.to_path_buf()),
        }
    }

    /// The store for the app's settings folder: `<settings>/plugins`, or off when there is
    /// none (`--no-settings`, or no per-user folder).
    #[inline(never)]
    pub fn for_settings_dir(settings: Option<&Path>) -> PluginStore {
        PluginStore {
            root: settings.map(|d| d.join(PLUGINS_FOLDER)),
        }
    }

    /// The folder plugins are installed in, or none when plugins are off.
    pub fn folder(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    fn root(&self) -> Result<&Path, PluginError> {
        self.root.as_deref().ok_or(PluginError::NoStore)
    }

    /// Reads a plugin folder and says what installing it would mean. Writes nothing and
    /// runs nothing.
    #[inline(never)]
    pub fn review(&self, source: &Path) -> Result<PendingInstall, PluginError> {
        let root = self.root()?;
        let package = Package::read_folder(source)?;
        // Taken before the installed copy is looked at, so any change after this point,
        // even one the review below already saw, makes the approval refuse.
        let snapshot = snapshot(&root.join(&package.manifest().id));
        let (change, before) = self.change_for(&package)?;
        let review = InstallReview::new(&package, change, before);
        Ok(PendingInstall {
            package,
            review,
            snapshot,
            root: root.to_path_buf(),
        })
    }

    fn change_for(&self, package: &Package) -> Result<(Change, CapabilitySet), PluginError> {
        let offered = package.manifest();
        let installed = match self.load(&offered.id) {
            Ok(installed) => installed,
            Err(Disabled::NotInstalled) => return Ok((Change::Fresh, CapabilitySet::NONE)),
            Err(reason) => {
                let reason = reason.to_string();
                return Ok((Change::Repair { reason }, CapabilitySet::NONE));
            }
        };
        let had = installed.manifest().version;
        let before = installed.granted();
        match had.cmp(&offered.version) {
            std::cmp::Ordering::Less => {
                let m = installed.manifest();
                let change = Change::Upgrade {
                    from: had,
                    previous_name: m.name.clone(),
                    previous_author: m.author.clone(),
                };
                Ok((change, before))
            }
            std::cmp::Ordering::Greater => Err(PluginError::Downgrade {
                installed: had,
                offered: offered.version,
            }),
            std::cmp::Ordering::Equal if installed.package.hash() == package.hash() => {
                Ok((Change::Reinstall, before))
            }
            std::cmp::Ordering::Equal => Err(PluginError::SameVersionChanged { version: had }),
        }
    }

    /// Installs a reviewed package: the user said yes. Refused if the installed copy changed
    /// since the review.
    #[inline(never)]
    pub fn approve(&self, pending: PendingInstall) -> Result<(), PluginError> {
        let root = self.root()?;
        let manifest = pending.package.manifest();
        if pending.root != root || snapshot(&root.join(&manifest.id)) != pending.snapshot {
            return Err(PluginError::ChangedSinceReview);
        }
        let record = Record::new(
            &manifest.id,
            manifest.version,
            &pending.package.hash(),
            manifest.capabilities,
            pending
                .package
                .files()
                .iter()
                .map(|f| f.name.clone())
                .collect(),
        );
        install::write(root, &pending.package, &record)
    }

    /// Removes an installed plugin and everything Pikslar kept for it.
    #[inline(never)]
    pub fn uninstall(&self, id: &str) -> Result<(), PluginError> {
        let root = self.root()?;
        let not_installed = || Err(PluginError::NotInstalled(id.to_string()));
        if check_id(id).is_err() {
            return not_installed();
        }
        if root.join(id).is_dir() {
            return install::remove(root, id);
        }
        // No plugin, but an install or uninstall that did not finish may have left its side
        // folders, which are never listed: they go too.
        match install::clear_leftovers(root, id)? {
            true => Ok(()),
            false => not_installed(),
        }
    }

    /// Loads an installed plugin if, and only if, its files are exactly what was approved.
    #[inline(never)]
    pub fn load(&self, id: &str) -> Result<LoadedPlugin, Disabled> {
        let root = self.root.as_deref().ok_or(Disabled::NotInstalled)?;
        if check_id(id).is_err() {
            return Err(Disabled::NotInstalled);
        }
        let dir = root.join(id);
        if std::fs::symlink_metadata(&dir).is_err() {
            return Err(Disabled::NotInstalled);
        }
        if std::fs::symlink_metadata(root).is_ok_and(|m| is_link(&m)) {
            return Err(Disabled::Unreadable(
                "the plugins folder is a link or junction".to_string(),
            ));
        }
        let (files, record) = read_flat_folder(&dir, Some(RECORD_FILE))
            .map_err(|e| Disabled::Unreadable(e.to_string()))?;
        let record =
            Record::parse(&record.ok_or(Disabled::NoRecord)?).map_err(Disabled::BadRecord)?;
        let mut sorted = files;
        crate::package::sort_by_name(&mut sorted);
        if hex(&content_hash(&sorted)) != record.hash {
            return Err(Disabled::Changed);
        }
        let package =
            Package::from_files(sorted).map_err(|e| Disabled::Unreadable(e.to_string()))?;
        check_matches(id, &record, &package)?;
        Ok(LoadedPlugin {
            granted: record.capabilities,
            package,
            hash: record.hash,
        })
    }

    /// Every plugin folder, enabled or not, by id. Folders Pikslar uses while installing
    /// (names starting with a dot) and plain files are not listed.
    #[inline(never)]
    pub fn list(&self) -> Vec<Listed> {
        let Some(root) = self.root.as_deref() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(root) else {
            return Vec::new();
        };
        let mut out: Vec<Listed> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() || t.is_symlink()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| !name.starts_with('.'))
            .take(MOST_LISTED)
            .map(|id| {
                let state = match check_id(&id) {
                    Ok(()) => self.load(&id),
                    Err(_) => Err(Disabled::NotAPlugin),
                };
                Listed { id, state }
            })
            .collect();
        // Folder names are unique, so this is the one order.
        pikslar_core::sorting::sort_by_text(&mut out, |listed| &listed.id);
        out
    }
}

fn check_matches(id: &str, record: &Record, package: &Package) -> Result<(), Disabled> {
    let m = package.manifest();
    let mismatch = |what: &str| Err(Disabled::Mismatch(what.to_string()));
    if record.id != id || m.id != id {
        return mismatch("the id differs from its folder's name");
    }
    if record.version != m.version {
        return mismatch("the version differs");
    }
    if record.capabilities != m.capabilities {
        return mismatch("the capabilities differ from what was approved");
    }
    let names: Vec<&str> = package.files().iter().map(|f| f.name.as_str()).collect();
    match record.files.iter().map(String::as_str).eq(names) {
        true => Ok(()),
        false => mismatch("the list of files differs"),
    }
}

/// What the installed folder for one id looks like now.
fn snapshot(dir: &Path) -> Snapshot {
    if std::fs::symlink_metadata(dir).is_err() {
        return Snapshot::Absent;
    }
    match read_record(&dir.join(RECORD_FILE)) {
        Ok(Some(record)) => Snapshot::Present(sha256(&record)),
        Ok(None) => Snapshot::Present(sha256(&[])),
        // A link, a folder, a device or a pipe where the record should be: never read, and
        // never equal to any record's bytes.
        Err(()) => Snapshot::NotPlain,
    }
}

/// The record's bytes, `None` when there is none, or `Err` when something other than a plain
/// file is in its place. It is opened without following links and checked on the opened
/// handle, so a link to a pipe or a device cannot block the reader.
fn read_record(path: &Path) -> Result<Option<Vec<u8>>, ()> {
    const MOST: u64 = 64 * 1024;
    let file = match open_no_follow(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    let meta = file.metadata().map_err(|_| ())?;
    check_plain(&meta, RECORD_FILE, MOST).map_err(|_| ())?;
    let mut record = Vec::new();
    file.take(MOST).read_to_end(&mut record).map_err(|_| ())?;
    Ok(Some(record))
}
