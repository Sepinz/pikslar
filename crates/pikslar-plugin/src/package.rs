//! A plugin package: one flat folder holding `plugin.json` and the scripts its commands run.
//!
//! The folder is read once into memory, within limits, and everything after that (the review,
//! the install, the hash) works on those bytes, so what the user approved is exactly what is
//! installed even if the folder changes meanwhile. Nothing in a package is ever executed here.

use crate::error::PluginError;
use crate::manifest::Manifest;
use crate::names::{check_script_name, MANIFEST_FILE};
use crate::sha256::Sha256;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

/// The most files in a package, `plugin.json` included.
pub const MOST_FILES: usize = 16;
/// The largest script file, in bytes.
pub const MOST_SCRIPT_BYTES: u64 = 256 * 1024;
/// The most bytes in all of a package's files together.
pub const MOST_PACKAGE_BYTES: u64 = 1024 * 1024;

/// One file of a package, by its checked name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// A checked package in memory.
#[derive(Clone, Debug)]
pub struct Package {
    manifest: Manifest,
    /// Sorted by name; `plugin.json` among them.
    files: Vec<PackageFile>,
    hash: [u8; 32],
}

impl Package {
    /// Reads and checks a plugin folder. The folder must hold nothing else: no folders,
    /// links, junctions or other files.
    #[inline(never)]
    pub fn read_folder(dir: &Path) -> Result<Package, PluginError> {
        let (files, _) = read_flat_folder(dir, None)?;
        Package::from_files(files)
    }

    /// Checks files already read: the manifest, and that the scripts and the commands match
    /// one for one.
    pub(crate) fn from_files(mut files: Vec<PackageFile>) -> Result<Package, PluginError> {
        sort_by_name(&mut files);
        let manifest_bytes = files
            .iter()
            .find(|f| f.name == MANIFEST_FILE)
            .map(|f| f.bytes.as_slice())
            .ok_or_else(|| refuse(format!("there is no {MANIFEST_FILE}")))?;
        let manifest = Manifest::parse(manifest_bytes).map_err(PluginError::Manifest)?;
        for command in &manifest.commands {
            if !files.iter().any(|f| f.name == command.script) {
                return Err(refuse(format!(
                    "the command {:?} runs {}, which is not in the folder",
                    command.title, command.script
                )));
            }
        }
        for file in files.iter().filter(|f| f.name != MANIFEST_FILE) {
            check_script_file(&manifest, file)?;
        }
        let hash = content_hash(&files);
        Ok(Package {
            manifest,
            files,
            hash,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn files(&self) -> &[PackageFile] {
        &self.files
    }

    /// SHA-256 over every file's name and bytes, in name order.
    pub fn hash(&self) -> [u8; 32] {
        self.hash
    }

    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes.len() as u64).sum()
    }

    /// The source a command runs.
    #[inline(never)]
    pub fn script_for(&self, command_id: &str) -> Option<&str> {
        let command = self.manifest.command(command_id)?;
        let file = self.files.iter().find(|f| f.name == command.script)?;
        std::str::from_utf8(&file.bytes).ok()
    }
}

fn refuse(why: String) -> PluginError {
    PluginError::Package(why)
}

fn check_script_file(manifest: &Manifest, file: &PackageFile) -> Result<(), PluginError> {
    if !manifest.commands.iter().any(|c| c.script == file.name) {
        return Err(refuse(format!(
            "{} is not run by any command; a plugin holds only what its commands run",
            file.name
        )));
    }
    match std::str::from_utf8(&file.bytes) {
        Ok(_) => Ok(()),
        Err(_) => Err(refuse(format!("{} is not UTF-8 text", file.name))),
    }
}

/// The hash a package is known by. Names and lengths are framed, so moving bytes from one
/// file to another always changes it.
pub(crate) fn content_hash(files: &[PackageFile]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"pikslar-plugin-content-1\0");
    for file in files {
        h.update(&(file.name.len() as u64).to_le_bytes());
        h.update(file.name.as_bytes());
        h.update(&(file.bytes.len() as u64).to_le_bytes());
        h.update(&file.bytes);
    }
    h.finish()
}

/// Reads every file of a flat folder, refusing anything that is not a plain file with an
/// allowed name. `reserved` names one more file Pikslar itself keeps there (the install
/// record), returned apart.
pub(crate) fn read_flat_folder(
    dir: &Path,
    reserved: Option<&str>,
) -> Result<(Vec<PackageFile>, Option<Vec<u8>>), PluginError> {
    check_real_folder(dir)?;
    let entries = fs::read_dir(dir).map_err(|e| crate::error::io("list the plugin folder", &e))?;
    let most = MOST_FILES + usize::from(reserved.is_some());
    let mut files = Vec::new();
    let mut kept = None;
    let mut total = 0u64;
    for (seen, entry) in entries.enumerate() {
        if seen >= most {
            return Err(refuse(format!(
                "the folder holds more than {MOST_FILES} files"
            )));
        }
        let entry = entry.map_err(|e| crate::error::io("list the plugin folder", &e))?;
        let name = entry_name(&entry)?;
        if reserved == Some(name.as_str()) {
            kept = Some(read_plain_file(&entry.path(), &name, MOST_SCRIPT_BYTES)?);
            continue;
        }
        if name != MANIFEST_FILE {
            check_script_name(&name).map_err(refuse)?;
        }
        let bytes = read_plain_file(&entry.path(), &name, MOST_SCRIPT_BYTES)?;
        total += bytes.len() as u64;
        if total > MOST_PACKAGE_BYTES {
            return Err(refuse(format!(
                "the files together are more than {MOST_PACKAGE_BYTES} bytes"
            )));
        }
        files.push(PackageFile { name, bytes });
    }
    Ok((files, kept))
}

fn entry_name(entry: &fs::DirEntry) -> Result<String, PluginError> {
    entry
        .file_name()
        .into_string()
        .map_err(|raw| refuse(format!("the file name {raw:?} is not valid Unicode")))
}

/// The folder itself must be a folder, not a link or junction to one somewhere else.
fn check_real_folder(dir: &Path) -> Result<(), PluginError> {
    let meta =
        fs::symlink_metadata(dir).map_err(|e| crate::error::io("open the plugin folder", &e))?;
    if is_link(&meta) {
        return Err(refuse("the folder is a link or junction".to_string()));
    }
    match meta.is_dir() {
        true => Ok(()),
        false => Err(refuse("it is not a folder".to_string())),
    }
}

/// Reads one file without following a link at its name, and refuses anything but a plain
/// file of at most `most` bytes. The checks are made on the opened file, not on the name, so
/// nothing can be swapped in between.
fn read_plain_file(path: &Path, name: &str, most: u64) -> Result<Vec<u8>, PluginError> {
    let before =
        fs::symlink_metadata(path).map_err(|e| crate::error::io("read a plugin file", &e))?;
    check_plain(&before, name, most)?;
    let file = open_no_follow(path).map_err(|e| crate::error::io("read a plugin file", &e))?;
    let meta = file
        .metadata()
        .map_err(|e| crate::error::io("read a plugin file", &e))?;
    check_plain(&meta, name, most)?;
    if crate::reparse::redirects(&file).map_err(|e| crate::error::io("read a plugin file", &e))? {
        return Err(refuse(format!("{name} is a link or junction")));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| crate::error::io("read a plugin file", &e))?;
    match bytes.len() as u64 > most {
        true => Err(refuse(format!("{name} is larger than {most} bytes"))),
        false => Ok(bytes),
    }
}

pub(crate) fn check_plain(meta: &fs::Metadata, name: &str, most: u64) -> Result<(), PluginError> {
    if is_link(meta) {
        return Err(refuse(format!("{name} is a link or junction")));
    }
    if !meta.is_file() {
        return Err(refuse(format!(
            "{name} is not a plain file; a plugin folder holds no folders"
        )));
    }
    match meta.len() > most {
        true => Err(refuse(format!("{name} is larger than {most} bytes"))),
        false => Ok(()),
    }
}

#[cfg(windows)]
const OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// A symbolic link, a junction, or any other reparse point that redirects: on Windows the
/// standard library reads the tag and tests its name-surrogate bit, as
/// [`crate::reparse::is_name_surrogate`] does. A reparse point that keeps the file in place
/// (a cloud placeholder) is not a link. Where a handle is open, use
/// [`crate::reparse::redirects`] on it.
pub(crate) fn is_link(meta: &fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

pub(crate) fn open_no_follow(path: &Path) -> std::io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(OPEN_REPARSE_POINT);
    }
    options.open(path)
}

/// Files in name order. Names in a folder are unique, so this is the one order; the sort is
/// the program's one sort of names.
pub(crate) fn sort_by_name(files: &mut Vec<PackageFile>) {
    pikslar_core::sorting::sort_by_text(files, |file| &file.name);
}
