//! A backup left by an interrupted save (B1, section 13: no unmitigated data-loss path).
//!
//! A save over a project moves the original to `name.pikslar-bak` (`ReplaceFileW`) and removes
//! it once the new file reads back. A save stopped in between (the process killed, the power
//! cut) can leave the backup with a damaged project beside it, or with none at all: it may be
//! the only good copy. So when a project is opened, or a recent file is found missing, a
//! backup beside it is offered: Open backup opens its work as a new, unnamed document (the
//! file stays as it is), Ignore leaves it. Until the person has answered, no save replaces it.
//!
//! A backup this session's own save left (one that could not be removed after its new file
//! read back) is known to be the older version, and may be replaced.

use crate::app::App;
use crate::diag::{note, Event};
use crate::files;
use pikslar_format::publish::backup_path;
use pikslar_platform::{show_error, Window};
use std::path::{Path, PathBuf};

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Said when a save was not made because the person kept a backup nobody decided about.
fn kept_said(backup: &Path) -> String {
    format!(
        "Not saved: {} is a backup left by an interrupted save, and may be the only good copy of          earlier work, so it was kept. Your work is still open: use Save as to keep it under          another name, or move the backup away, and save again",
        backup.display()
    )
}

/// `path` as Windows compares names: ignoring case, and one kind of slash.
fn folded(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

impl App {
    /// The backup is one the person decided about, or one this session's save left.
    fn backup_known(&self, backup: &Path) -> bool {
        let want = folded(backup);
        self.backups_known.iter().any(|b| folded(b) == want)
    }

    /// Forgets that `backup` may be replaced: it may be the only good copy now.
    pub(crate) fn backup_unknown(&mut self, backup: &Path) {
        let gone = folded(backup);
        self.backups_known.retain(|b| folded(b) != gone);
    }

    /// Remembers that `backup` may be replaced.
    pub(crate) fn backup_known_old(&mut self, backup: PathBuf) {
        if !self.backup_known(&backup) {
            self.backups_known.push(backup);
        }
    }

    /// After opening `path`, or finding it missing: offers a backup an interrupted save left
    /// beside it, once.
    pub(crate) fn offer_orphan_backup(&mut self, window: Window, path: &Path) {
        let backup = backup_path(path);
        if !backup.is_file() || self.backup_known(&backup) {
            return;
        }
        note(Event::new("backup", "found").file(path));
        match (self.ask_backup)(window, &name_of(&backup)) {
            Some(true) => {
                note(Event::new("backup", "opened").file(path));
                self.open_backup(window, &backup);
            }
            Some(false) => {
                note(Event::new("backup", "ignored").file(path));
                self.backup_known_old(backup);
            }
            // Nobody answered: it stays, and saves leave it alone.
            None => {}
        }
    }

    /// Opens the backup's work as a new document with no file, so the file stays as it is.
    fn open_backup(&mut self, window: Window, backup: &Path) {
        match files::open(backup) {
            Ok(opened) => {
                self.replace_document(opened.doc, None);
                self.notify(format!(
                    "Opened the backup {}. Save it under a name of its own with Save as",
                    name_of(backup)
                ));
            }
            Err(e) => show_error(
                window,
                &format!("The backup {} could not be opened: {e}", backup.display()),
            ),
        }
    }

    /// A save to `path` would replace a backup nobody decided about: asks at once whether it
    /// may, without closing the work being saved. True if the save may go on.
    pub(crate) fn backup_allows_save(&mut self, window: Window, path: &Path) -> bool {
        let backup = backup_path(path);
        if !backup.exists() || self.backup_known(&backup) {
            return true;
        }
        note(Event::new("save", "backup-in-the-way").file(path));
        if (self.ask_replace_backup)(window, &name_of(&backup)) == Some(true) {
            note(Event::new("backup", "replace-allowed").file(path));
            self.backup_known_old(backup);
            return true;
        }
        let said = kept_said(&backup);
        self.save_failed(said.clone());
        show_error(window, &said);
        false
    }
}

#[cfg(test)]
#[path = "orphan_backup_tests.rs"]
mod tests;
