//! Temporary folders and plugin folders for the tests.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A fresh folder, removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(label: &str) -> TempDir {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("pikslar-plugin-{label}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp folder");
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn manifest(id: &str, version: &str, capabilities: &[&str]) -> String {
    let words: Vec<String> = capabilities.iter().map(|c| format!("\"{c}\"")).collect();
    format!(
        r#"{{"manifest": 1, "id": "{id}", "name": "Outline", "version": "{version}", "api": 1,
  "capabilities": [{}],
  "commands": [{{"id": "add-outline", "title": "Add outline", "script": "outline.pks"}}],
  "author": "Example Studio"}}"#,
        words.join(", ")
    )
}

/// Writes a good plugin folder, `name` inside `parent`, and returns its path.
pub fn plugin_folder(parent: &Path, name: &str, id: &str, version: &str, caps: &[&str]) -> PathBuf {
    let dir = parent.join(name);
    fs::create_dir_all(&dir).expect("plugin folder");
    fs::write(dir.join("plugin.json"), manifest(id, version, caps)).expect("manifest");
    fs::write(
        dir.join("outline.pks"),
        format!("# {id} {version}\nlet x = 1\n"),
    )
    .expect("script");
    dir
}

/// Every name under `dir`, recursively, as text, sorted.
pub fn everything_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        out.push(path.display().to_string());
        if path.is_dir() {
            out.extend(everything_under(&path));
        }
    }
    out.sort();
    out
}

/// Makes a junction, which Windows lets anyone make. False where that is not possible.
pub fn junction(link: &Path, target: &Path) -> bool {
    if !cfg!(windows) {
        return false;
    }
    std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Makes a file symbolic link. False where the system does not allow it (Windows without
/// developer mode).
pub fn file_symlink(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}
