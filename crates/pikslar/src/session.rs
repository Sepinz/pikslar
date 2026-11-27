//! Opening and saving, and keeping unsaved work safe.

use crate::app::{starter_document, App, Gesture, NewDialog, ToolChoice};
use crate::diag::{note, Event};
use crate::files;
use crate::pending::QUIT_WAIT;
use crate::recovery::{Origin, Orphan, Recovery};
use crate::save_state::PIXELS_NOT_PLACED;
use pikslar_core::ColorMode;
use pikslar_core::StepCmd;
use pikslar_platform::{
    ask_replace_file, ask_restore_work, ask_save_changes, open_file_dialog, save_file_dialog,
    show_error, SaveChoice, Window,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PROJECT_FILTER: &str = "Pikslar project\0*.piks\0";
pub(crate) const OPEN_FILTER: &str = "Sprites and images\0*.piks;*.png;*.ase;*.aseprite\0\
     Pikslar project\0*.piks\0Aseprite files\0*.ase;*.aseprite\0";
pub(crate) const PNG_FILTER: &str = "PNG images\0*.png\0";
/// How long a change may stay without a recovery copy.
const RECOVERY_DELAY: Duration = Duration::from_secs(8);
/// How soon to try again when a copy could not start yet.
const RECOVERY_RETRY: Duration = Duration::from_millis(250);
/// How long to wait before writing again after a write failed, such as on a full disk.
const RECOVERY_AFTER_FAILURE: Duration = Duration::from_secs(30);
/// Never equals what the editor reports.
const RECOVERY_NOT_WRITTEN: (u64, u64, u64) = (u64::MAX, u64::MAX, u64::MAX);

/// Files open by extension, so a file must never be written under another one.
pub(crate) fn with_required_extension(path: PathBuf, extension: &str) -> PathBuf {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
    {
        return path;
    }
    path.with_extension(extension)
}

/// The project a name chosen in the Save dialog means. A name typed as an Aseprite file, which
/// the dialog may have given `.piks` on top, becomes the project of the same name: nothing
/// writes an Aseprite file, and `walk.ase.piks` would only look like one.
pub(crate) fn project_name(chosen: PathBuf) -> PathBuf {
    let piks = chosen
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("piks"));
    let inner = piks
        .then(|| chosen.with_extension(""))
        .filter(|stem| files::is_aseprite(stem));
    match inner {
        Some(stem) => stem.with_extension("piks"),
        None => with_required_extension(chosen, "piks"),
    }
}

/// The system dialog only warned about the name that was typed, not the corrected one.
pub(crate) fn confirm_corrected(window: Window, chosen: &Path, target: PathBuf) -> Option<PathBuf> {
    if target == chosen || !target.exists() {
        return Some(target);
    }
    let name = target
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    ask_replace_file(window, &name).then_some(target)
}

const PROTECTED_MESSAGE: &str =
    "That file holds data this version cannot keep, so it is not replaced. Choose another name.";

/// What became of a file chosen to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opened {
    /// It is the document now.
    Now,
    /// It could be read, but the person kept the document they had.
    Kept,
    /// It could not be read, and the person was told why.
    Failed,
}

impl App {
    /// Says at launch that crash recovery could not be started, and why.
    pub(crate) fn recovery_off(&mut self, problem: String) {
        self.notify_error(format!("Crash recovery is off: {problem}"));
    }

    /// Returns false if the user chose to keep working.
    pub fn confirm_discard(&mut self, window: Window) -> bool {
        self.settle();
        // A save under way ends first, so the question is about what it did not keep.
        if !self.wait_for_saves(window, crate::save_run::SAVE_WAIT) {
            self.notify_error(crate::save_run::STILL_SAVING.to_string());
            return false;
        }
        if !self.has_unsaved_work() {
            return true;
        }
        match ask_save_changes(window) {
            SaveChoice::Save => self.save(window, false),
            SaveChoice::Discard => true,
            SaveChoice::Cancel => false,
        }
    }

    /// Returns false if the user chose to keep the current sprite.
    pub fn create_new(&mut self, window: Window, dialog: NewDialog) -> bool {
        if !self.confirm_discard(window) {
            return false;
        }
        self.stop_opening();
        let mut doc = starter_document(dialog.size);
        if dialog.mode == ColorMode::Rgba {
            doc.layers_mut()[0].mode = ColorMode::Rgba;
        }
        self.replace_document(doc, None);
        self.first_use = true;
        true
    }

    /// Picks and loads first, so a cancelled or damaged file costs the user nothing.
    pub fn open(&mut self, window: Window) {
        self.finish_gesture();
        if let Some(path) = open_file_dialog(window, OPEN_FILTER) {
            self.open_path(window, path);
        }
    }

    /// Opens a file already chosen, from the dialog, on another thread.
    pub(crate) fn open_path(&mut self, window: Window, path: PathBuf) {
        self.begin_open(window, path, crate::loading::Then::Nothing);
    }

    /// Takes a file that has been read: from the dialog, the recent files or a drop. The question
    /// about unsaved work was asked before the read began, so this replaces the document.
    pub(crate) fn finish_open(
        &mut self,
        window: Window,
        path: PathBuf,
        result: Result<files::Opened, files::FileError>,
    ) -> Opened {
        self.finish_gesture();
        let opened = match result {
            Ok(opened) => opened,
            Err(e) => {
                note(Event::new("open", "fail").failed(&e).file(&path));
                show_error(window, &e.to_string());
                return Opened::Failed;
            }
        };
        note(Event::new("open", "ok").file(&path));
        crate::settings::remember(&mut self.recent, &path);
        let notice = opened.notice();
        let protected = files::protected_path(&path, &opened.notes);
        let imported = files::is_aseprite(&path).then(|| path.clone());
        let read = path.clone();
        let target = files::save_path_for(path, &opened.notes);
        let print = opened.print;
        // Only a file saved back to itself is looked at again before a save.
        let seen = opened
            .stamp
            .filter(|_| target.as_deref() == Some(read.as_path()));
        self.replace_document(opened.doc, target);
        self.seen = seen.map(|stamp| crate::outside::Seen { stamp, print });
        self.attach_rewind(print);
        self.protected_path = protected;
        if let Some(source) = imported {
            self.mark_imported(source);
        }
        if let Some(note) = notice {
            self.notify(note);
        }
        Opened::Now
    }

    pub(crate) fn is_protected(&self, target: &Path) -> bool {
        self.protected_path
            .as_deref()
            .is_some_and(|protected| files::same_file(protected, target))
    }

    pub(crate) fn save_target(&self, window: Window, choose_path: bool) -> Option<PathBuf> {
        if let Some(path) = self.path.clone().filter(|_| !choose_path) {
            return Some(path);
        }
        self.choose_target(window, self.save_start().as_deref())
    }

    /// Where the Save dialog starts: the project's own file, or for an import a project of
    /// the same name beside the file it came from.
    pub(crate) fn save_start(&self) -> Option<PathBuf> {
        self.path.clone().or_else(|| {
            self.imported_from
                .as_ref()
                .map(|source| source.with_extension("piks"))
        })
    }

    /// Asks where to save, starting at `start`.
    pub(crate) fn choose_target(&self, window: Window, start: Option<&Path>) -> Option<PathBuf> {
        let chosen = save_file_dialog(window, PROJECT_FILTER, "piks", start)?;
        let target = project_name(chosen.clone());
        let target = confirm_corrected(window, &chosen, target)?;
        if self.is_protected(&target) {
            show_error(window, PROTECTED_MESSAGE);
            return None;
        }
        Some(target)
    }

    /// Saving without pixels that could not be applied would look finished and lose them. An
    /// open dragged value ends here, so it cannot later rewind to before this save.
    pub(crate) fn ready_to_save(&mut self) -> bool {
        let settled = self.settle();
        if settled {
            self.run(StepCmd::EndDrag);
        } else {
            self.save_blocked(PIXELS_NOT_PLACED.to_string());
        }
        settled
    }

    pub fn use_recovery(&mut self, recovery: Recovery, offer_restore: bool) {
        self.pending_offer = offer_restore && recovery.is_enabled();
        self.recovery = recovery;
        self.recovery.note_origin(self.path.as_deref());
    }

    /// Offers to bring back work from a session that ended unexpectedly.
    pub fn offer_recovery(&mut self, window: Window) {
        self.recovery.sweep();
        let Some(orphan) = self.recovery.find_orphan() else {
            return;
        };
        note(Event::new("restore", "offered"));
        match ask_restore_work(window, &orphan.display_name()) {
            Some(true) => note(Event::new("restore", "accepted")),
            Some(false) => {
                note(Event::new("restore", "declined"));
                Recovery::discard(&orphan);
                return;
            }
            None => return note(Event::new("restore", "later")),
        }
        if let Err(e) = self.restore_orphan(&orphan) {
            note(Event::new("restore", "bad").failed(&e));
            let kept = Recovery::set_aside(&orphan);
            let message = format!(
                "The recovered work could not be opened: {e}\n\nThe copy was kept at {}",
                kept.display()
            );
            show_error(window, &message);
        }
    }

    /// Loads a left-behind copy into this session. The file it came from is never written.
    pub(crate) fn restore_orphan(&mut self, orphan: &Orphan) -> Result<(), files::FileError> {
        let recovered = files::open_recovery(&orphan.project)?;
        // Replacing clears this session's copy, so the restored one is adopted afterwards.
        let can_save_over = orphan.may_save_over_origin();
        self.replace_document(
            recovered.doc,
            orphan
                .origin
                .clone()
                .filter(|_| can_save_over)
                .and_then(|path| files::save_path_for(path, &Default::default())),
        );
        if orphan.protected.is_some() {
            self.protected_path = orphan.protected.clone();
        }
        // Its origin was checked against the stamp just now; its bytes were not read.
        let stamp = orphan.stamp.filter(|_| self.path.is_some());
        self.seen = stamp.map(|stamp| crate::outside::Seen { stamp, print: None });
        let floating = recovered
            .floating
            .map(|snapshot| self.editor.restore_floating(snapshot).is_ok());
        let lost = recovered.floating_damaged || floating == Some(false);
        if floating == Some(true) {
            self.tool = ToolChoice::Move;
        }
        if let Err(e) = self.recovery.adopt(orphan) {
            self.notify_error(format!("Restored, but the backup could not be kept: {e}"));
        } else if !can_save_over && orphan.protected.is_some() {
            self.notify(
                "Restored. Save will ask for a new name, so the original keeps what it holds"
                    .to_string(),
            );
        } else if !can_save_over {
            self.notify(
                "Restored. The file has changed since, so Save will ask for a name".to_string(),
            );
        } else if lost {
            self.notify(
                "Restored your art. The pixels being moved could not be restored".to_string(),
            );
        } else if floating == Some(true) {
            self.notify("Restored. Enter applies the moved pixels, Esc puts them back".to_string());
        } else if let Some(note) = recovered.notes.message() {
            self.notify(note);
        }
        // It differs from anything saved, so it must read as unsaved.
        self.saved_revision = u64::MAX;
        self.editor.forget_saved();
        self.recovered = self.recovery_stamp();
        Ok(())
    }

    /// The file no longer holds the work: a save left other bytes in it. The work counts as
    /// not saved, so closing asks, and a recovery copy of it is due at once.
    pub(crate) fn work_no_longer_saved(&mut self) {
        self.saved_revision = u64::MAX;
        self.editor.forget_saved();
        self.recovered = RECOVERY_NOT_WRITTEN;
        self.recovery_due = Some(Instant::now());
        // A session end already answered took the work as safe; it must look again.
        self.session_kept = None;
        // An open asked nothing, the work then being saved: it must not replace it now.
        self.stop_opening();
    }

    /// What a recovery copy made now would show.
    pub(crate) fn recovery_stamp(&self) -> (u64, u64, u64) {
        (
            self.editor.revision(),
            self.editor.floating_stamp(),
            self.editor.revision_rewinds(),
        )
    }

    /// True if ending now would lose something, including pixels not yet applied. A save
    /// under way counts: until it ends, the file is not known to hold the work.
    pub(crate) fn has_unsaved_work(&self) -> bool {
        self.is_modified() || self.save_job.is_some()
    }

    /// Copies changed work to the recovery folder a few seconds after it changes.
    pub fn tick_recovery(&mut self) {
        if let Some(e) = self.recovery.take_error() {
            self.notify_error(format!("Could not back up your work: {e}"));
            // The copy on disk is stale until a write succeeds, so it is due again.
            self.recovered = RECOVERY_NOT_WRITTEN;
            self.recovery_due = Some(Instant::now() + RECOVERY_AFTER_FAILURE);
        }
        let stamp = self.recovery_stamp();
        if !self.recovery.is_enabled() || stamp == self.recovered {
            return;
        }
        if !self.has_unsaved_work() {
            self.recovery.clear();
            self.recovered = stamp;
            self.recovery_due = None;
            return;
        }
        let now = Instant::now();
        let due = *self.recovery_due.get_or_insert(now + RECOVERY_DELAY);
        // A stroke's pixels are not in the document yet. A drag's are safe in the snapshot.
        if now < due || self.gesture == Gesture::Stroke {
            return;
        }
        let copy = self.editor.doc().clone();
        let floating = self.editor.floating_snapshot();
        if self.recovery.write_in_background(
            copy,
            floating,
            Origin::of(&self.path, &self.protected_path),
        ) {
            self.recovered = stamp;
            self.recovery_due = None;
        }
    }

    pub fn recovery_wait(&self) -> Option<Duration> {
        let now = Instant::now();
        self.recovery_due
            .map(|due| due.saturating_duration_since(now).max(RECOVERY_RETRY))
    }

    /// Windows will not wait for a person to answer, so unsaved work goes to recovery. A
    /// dialog drawn in the window is left open, and what is typed in it is not in the copy:
    /// the diagnostics say which one it was.
    pub fn end_session(&mut self) -> bool {
        self.end_session_within(QUIT_WAIT)
    }

    /// As [`App::end_session`], waiting at most `wait` for a backup already being written.
    fn end_session_within(&mut self, wait: Duration) -> bool {
        // A result must not replace the document before its work is safe.
        self.abandon_opening();
        self.finish_gesture();
        self.finish_rewind_closing();
        if let Some(open) = self.save_blocker() {
            note(Event::new("session-end", "dialog-open").phase(open.tag()));
        }
        if !self.has_unsaved_work() {
            self.session_kept = Some(self.recovery_stamp());
            return true;
        }
        let started = Instant::now();
        let floating = self.editor.floating_snapshot();
        match self.recovery.write_now_within(
            self.editor.doc(),
            floating.as_ref(),
            Origin::of(&self.path, &self.protected_path),
            wait,
        ) {
            Ok(()) => {
                note(Event::new("recovery-now", "ok").took(started));
                self.recovery.set_keep(true);
                self.session_kept = Some(self.recovery_stamp());
                true
            }
            Err(e) => {
                note(Event::new("recovery-now", "fail").took(started).io(&e));
                self.notify_error(format!("Could not protect unsaved work: {e}"));
                false
            }
        }
    }
}

impl App {
    /// `WM_ENDSESSION`. Ending, work not made safe at the question, or changed since, is
    /// copied now; the window then waits for writes under way (`session_end.rs`).
    pub(crate) fn session_over(&mut self, ending: bool) {
        self.session_over_within(ending, pikslar_platform::end_budget());
    }

    /// As [`App::session_over`], with the time the end has left for a backup under way: the
    /// same 4 s the window then spends on writes (`session_end.rs`), not more.
    pub(crate) fn session_over_within(&mut self, ending: bool, budget: Duration) {
        let what = if ending { "ending" } else { "called-off" };
        note(Event::new("session-end", what));
        if !ending {
            self.session_kept = None;
        } else if self.session_kept != Some(self.recovery_stamp()) {
            self.end_session_within(budget);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_backup_is_tried_again_rather_than_forgotten() {
        let dir = std::env::temp_dir().join(format!("pikslar-retry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let recovery = Recovery::try_in_dir(&dir).expect("usable");
        recovery.block_writes();
        let mut app = App::launch(None, Some(16), None);
        app.use_recovery(recovery, false);
        app.run(pikslar_core::FrameCmd::Retime {
            from: 0,
            durations: vec![322],
        });
        app.recovery_due = Some(Instant::now());
        app.tick_recovery();
        assert_eq!(app.recovered.0, app.editor.revision());
        while app.message.is_none() {
            std::thread::sleep(Duration::from_millis(1));
            app.tick_recovery();
        }
        assert_ne!(app.recovered.0, app.editor.revision());
        assert!(app.recovery_due.is_some());
        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saved_files_always_get_the_extension_they_open_by() {
        let piks = |p: &str| with_required_extension(p.into(), "piks");
        assert_eq!(piks("art.png"), PathBuf::from("art.piks"));
        assert_eq!(piks("art"), PathBuf::from("art.piks"));
        assert_eq!(piks("art.PIKS"), PathBuf::from("art.PIKS"));
    }
}

/// Where a test asked for the layout, and what was last written there.
#[derive(Debug)]
pub(crate) struct LayoutDump {
    path: std::path::PathBuf,
    last: String,
    failures: u32,
}

/// Writes in a row that may fail before the dump is given up.
const DUMP_TRIES: u32 = 20;

impl LayoutDump {
    pub fn new(path: std::path::PathBuf) -> LayoutDump {
        LayoutDump {
            path,
            last: String::new(),
            failures: 0,
        }
    }

    /// Replaces the file whole with `text` if it changed. Err(None) asks to be tried again;
    /// Err(Some(message)) means it has failed too often and should stop.
    pub fn write(&mut self, text: String) -> Result<(), Option<String>> {
        if self.last == text {
            return Ok(());
        }
        let mut temp = self.path.clone().into_os_string();
        temp.push(".part");
        let written =
            std::fs::write(&temp, &text).and_then(|()| std::fs::rename(&temp, &self.path));
        match written {
            Ok(()) => {
                self.last = text;
                self.failures = 0;
                Ok(())
            }
            Err(e) => {
                self.failures += 1;
                Err((self.failures >= DUMP_TRIES)
                    .then(|| format!("Could not write the layout to {}: {e}", self.path.display())))
            }
        }
    }
}

impl App {
    /// Keeps where each control is in `path` from now on, for test scripts to find them by
    /// name. The file is replaced whole each time the layout changes.
    pub fn dump_layout_to(&mut self, path: std::path::PathBuf) {
        self.ui.record_names();
        self.layout_dump = Some(crate::session::LayoutDump::new(path));
    }

    /// A failed write, say while a script has the file open, is tried again on the next frame,
    /// and the dump stops only after it keeps failing.
    pub(crate) fn write_layout(&mut self) {
        let (Some(dump), Some(text)) = (&mut self.layout_dump, self.ui.layout()) else {
            return;
        };
        match dump.write(text) {
            Ok(()) => {}
            Err(Some(message)) => {
                self.layout_dump = None;
                self.notify(message);
            }
            Err(None) => self.ui_dirty = true,
        }
    }
}

#[cfg(test)]
mod layout_dump_tests {
    use super::*;

    #[test]
    fn a_dump_is_replaced_whole_and_kept_trying_while_it_cannot_be_written() {
        let dir = std::env::temp_dir().join(format!("pikslar-dump-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("layout.part");
        let mut dump = LayoutDump::new(path.clone());
        assert_eq!(dump.write("a\n".into()), Ok(()));
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "a\n");
        assert_eq!(
            dump.write("b\n".into()),
            Ok(()),
            "a name ending in .part is fine"
        );
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "b\n");
        let mut blocked = LayoutDump::new(dir.join("missing").join("layout.txt"));
        for _ in 1..DUMP_TRIES {
            assert_eq!(blocked.write("c".into()), Err(None));
        }
        assert!(matches!(blocked.write("c".into()), Err(Some(_))));
        std::fs::remove_dir_all(&dir).expect("cleaned");
    }
}

#[cfg(test)]
#[path = "session_end_tests.rs"]
mod end_tests;
