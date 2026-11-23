//! Writing and removing installed plugins so that a crash at any step leaves either the old
//! plugin, the new one, or a folder [`crate::PluginStore::load`] disables, never a mix.
//!
//! A new copy is written whole into `.new-<id>`, its record last, then renamed into place;
//! the old copy is first renamed to `.old-<id>` and removed afterwards. Folders whose names
//! start with a dot are never listed or loaded, and are cleared by the next install or
//! uninstall of that id.

use crate::error::{io, PluginError};
use crate::package::{is_link, Package};
use crate::record::{Record, RECORD_FILE};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn side(root: &Path, kind: &str, id: &str) -> PathBuf {
    root.join(format!(".{kind}-{id}"))
}

/// Removes a folder of Pikslar's own if it is there. It never follows a link inside.
fn clear(dir: &Path) -> Result<(), PluginError> {
    match fs::symlink_metadata(dir) {
        Err(_) => Ok(()),
        Ok(_) => fs::remove_dir_all(dir).map_err(|e| io("remove an old plugin copy", &e)),
    }
}

fn check_root(root: &Path) -> Result<(), PluginError> {
    fs::create_dir_all(root).map_err(|e| io("make the plugins folder", &e))?;
    let meta = fs::symlink_metadata(root).map_err(|e| io("open the plugins folder", &e))?;
    match is_link(&meta) {
        true => Err(PluginError::Io(
            "the plugins folder is a link or junction; Pikslar will not install through it"
                .to_string(),
        )),
        false => Ok(()),
    }
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), PluginError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| io("write a plugin file", &e))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| io("write a plugin file", &e))
}

/// Installs `package` under `root`, replacing any copy with the same id.
pub(crate) fn write(root: &Path, package: &Package, record: &Record) -> Result<(), PluginError> {
    check_root(root)?;
    let id = &record.id;
    let staging = side(root, "new", id);
    let old = side(root, "old", id);
    clear(&staging)?;
    clear(&old)?;
    clear(&side(root, "del", id))?;
    fs::create_dir(&staging).map_err(|e| io("make a plugin folder", &e))?;
    let written = package
        .files()
        .iter()
        .try_for_each(|f| write_new(&staging.join(&f.name), &f.bytes))
        .and_then(|()| write_new(&staging.join(RECORD_FILE), &record.to_bytes()));
    if let Err(e) = written {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }
    swap_in(root, id, &staging, &old)
}

fn swap_in(root: &Path, id: &str, staging: &Path, old: &Path) -> Result<(), PluginError> {
    let target = root.join(id);
    let had_old = fs::symlink_metadata(&target).is_ok();
    if had_old {
        fs::rename(&target, old).map_err(|e| io("move the installed copy aside", &e))?;
    }
    if let Err(e) = fs::rename(staging, &target) {
        if had_old {
            let _ = fs::rename(old, &target);
        }
        let _ = fs::remove_dir_all(staging);
        return Err(io("put the new copy in place", &e));
    }
    // The new copy is in place and loads. An old copy that cannot be removed now (a virus
    // scanner holding a file, say) is never listed or loaded, and the next install or
    // uninstall of this id tries again, so the install still succeeded.
    if had_old {
        let _ = clear(old);
    }
    Ok(())
}

/// Uninstalls `id`: moved aside first so it stops loading at once, then removed.
pub(crate) fn remove(root: &Path, id: &str) -> Result<(), PluginError> {
    let gone = side(root, "del", id);
    clear(&gone)?;
    fs::rename(root.join(id), &gone).map_err(|e| io("uninstall the plugin", &e))?;
    clear(&gone)?;
    clear(&side(root, "new", id))?;
    clear(&side(root, "old", id))
}

/// Removes the side folders an unfinished install or uninstall of `id` left, giving back
/// whether there were any.
pub(crate) fn clear_leftovers(root: &Path, id: &str) -> Result<bool, PluginError> {
    let mut any = false;
    for kind in ["del", "new", "old"] {
        let dir = side(root, kind, id);
        if fs::symlink_metadata(&dir).is_ok() {
            any = true;
            clear(&dir)?;
        }
    }
    Ok(any)
}
