//! What the person approved, kept outside the plugins folder (review finding S1, RC3).
//!
//! A plugin's install record sits in its own folder, so a folder copied in by hand can bring a
//! record of its own, and the record's hash is one anyone can compute. The approvals file, beside
//! the plugins folder, holds the hash of each record Pikslar wrote after the install review was
//! answered Yes. A plugin loads only if its record is the one approved. Plugins cannot write it:
//! their file access refuses the settings folder.
//!
//! One line per plugin: its id, a space, and the SHA-256 of its install record, in hex.

use crate::error::{io, PluginError};
use crate::sha256::{hex, sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The approvals file's name, beside the plugins folder.
pub const APPROVALS_FILE: &str = "plugin-approvals.txt";
/// Far more than [`crate::store::MOST_LISTED`] lines need; a larger file is not read.
const MOST_BYTES: u64 = 256 * 1024;

fn approvals_path(root: &Path) -> Option<PathBuf> {
    root.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.join(APPROVALS_FILE))
}

/// The approved entries, as (id, hash). Missing or unreadable reads as none approved.
fn entries(root: &Path) -> Vec<(String, String)> {
    let Some(path) = approvals_path(root) else {
        return Vec::new();
    };
    let small = fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MOST_BYTES);
    let text = match small {
        true => fs::read_to_string(&path).unwrap_or_default(),
        false => String::new(),
    };
    text.lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(id, hash)| (id.to_string(), hash.trim().to_string()))
        .collect()
}

/// True if `record`, the bytes of `id`'s install record, is the one the person approved.
pub(crate) fn is_approved(root: &Path, id: &str, record: &[u8]) -> bool {
    let hash = hex(&sha256(record));
    entries(root).iter().any(|(i, h)| i == id && *h == hash)
}

/// Notes `record` as `id`'s approved install record, or with `None` forgets `id`.
pub(crate) fn set(root: &Path, id: &str, record: Option<&[u8]>) -> Result<(), PluginError> {
    let hashes: Vec<String> = record.map(|r| hex(&sha256(r))).into_iter().collect();
    restore(root, id, &hashes)
}

/// Notes `record` as approved too, keeping `id`'s other approvals. Gives those, for
/// [`restore`].
pub(crate) fn add(root: &Path, id: &str, record: &[u8]) -> Result<Vec<String>, PluginError> {
    let before: Vec<String> = entries(root)
        .into_iter()
        .filter(|(i, _)| i == id)
        .map(|(_, h)| h)
        .collect();
    let mut hashes = before.clone();
    hashes.push(hex(&sha256(record)));
    restore(root, id, &hashes)?;
    Ok(before)
}

/// Makes `hashes` the approvals of `id`, exactly.
pub(crate) fn restore(root: &Path, id: &str, hashes: &[String]) -> Result<(), PluginError> {
    let Some(path) = approvals_path(root) else {
        return Err(PluginError::NoStore);
    };
    let mut kept: Vec<(String, String)> =
        entries(root).into_iter().filter(|(i, _)| i != id).collect();
    kept.extend(hashes.iter().map(|h| (id.to_string(), h.clone())));
    let text: String = kept.iter().map(|(i, h)| format!("{i} {h}\n")).collect();
    let temp = path.with_extension("txt.new");
    let written = fs::File::create(&temp).and_then(|mut f| {
        f.write_all(text.as_bytes())?;
        f.sync_all()
    });
    written
        .and_then(|()| fs::rename(&temp, &path))
        .map_err(|e| {
            let _ = fs::remove_file(&temp);
            io("note the plugin's approval", &e)
        })
}
