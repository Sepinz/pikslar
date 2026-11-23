//! Saving on a thread of its own (B1, section 13, requirement 3), so a slow disk never holds
//! the window.
//!
//! On the window's thread: the questions (where, whether to upgrade an older project), the
//! moved pixels put down, and a copy of the document taken. On the saving thread: encoding,
//! the check that the bytes open again, writing, flushing, reading back and putting the file in
//! place. The result goes in a slot and wakes the window; the frame takes it.
//!
//! - Edits made while a save runs stay unsaved: the saved revision is the copy's.
//! - Save while one runs queues one more, made from the document as it is when the first
//!   ends; there is never more than one writer. Save as while one runs asks where at once and
//!   queues the save to that place, which wins over a queued plain save.
//! - Closing, quitting, opening or starting anew waits for a running save and what it queued,
//!   with "Saving..." on the status bar, for at most [`SAVE_WAIT`]; past that the window stays
//!   open and says so. It never goes on before the file is in place or the save failed.
//! - The recovery copy is let go and Rewind's steps written only when the document is still
//!   the revision saved; otherwise the next save does both.
//! - Upgrading an older project runs on the saving thread too, with its backup and its check
//!   that the file was not changed meanwhile, as before.

use crate::app::App;
use crate::diag::{note, Event};
use crate::files::{self, FileError, SaveFailure};
use crate::outside::{Outside, Seen};
use crate::rewind_link::RewindBefore;
use crate::save_keys::SaveBlocker;
use pikslar_core::{Document, SavedCopy};
use pikslar_format::publish_back::{not_kept, NotKept};
use pikslar_format::upgrade;
use pikslar_platform::{ask_upgrade, UpgradeChoice, Window};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The longest closing, quitting or opening waits for a save under way.
pub(crate) const SAVE_WAIT: Duration = Duration::from_secs(30);
/// Said when that wait ends with the save still going.
pub(crate) const STILL_SAVING: &str =
    "Still saving after 30 seconds, so the window stays open. Try again when it says Saved";

/// What a save ended with.
type Outcome = Result<Saved, SaveFailure>;

/// A save that worked: the print of the bytes saved, how the file looked once in place, and
/// the backup of the original if it could not be removed.
#[derive(Clone, Debug)]
struct Saved {
    print: u128,
    seen: Option<Seen>,
    backup_left: Option<PathBuf>,
}
type Slot = Arc<Mutex<Option<Outcome>>>;

fn locked(slot: &Slot) -> MutexGuard<'_, Option<Outcome>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A save running on its thread.
pub(crate) struct SaveJob {
    path: PathBuf,
    kind: &'static str,
    /// The editor's revision the copy was taken at.
    revision: u64,
    /// The place in the history the copy was taken at, which is where the file will be.
    copy: SavedCopy,
    /// Which document it was, counted by [`App::documents`].
    document: u64,
    before: Option<RewindBefore>,
    started: Instant,
    slot: Slot,
    worker: Option<JoinHandle<()>>,
}

impl SaveJob {
    pub(crate) fn is_done(&self) -> bool {
        locked(&self.slot).is_some()
    }
}

/// The one save that may wait for a running one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FollowUp {
    /// Save again, to the document's own file.
    Save,
    /// Save to this place, chosen while the first save ran.
    To(PathBuf),
}

/// What a request to save did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Started {
    Now,
    /// After the save under way.
    Queued,
    /// Not at all: refused, cancelled, or nowhere was chosen.
    No,
}

/// What the saving thread is asked to do.
#[derive(Clone, Copy)]
enum Work {
    /// Save, expecting the file to look as it did when last looked at, if it was.
    Save(Option<Seen>),
    /// Save over a project of this older version, with the same expectation.
    Upgrade(u32, Option<Seen>),
}

/// The save itself, and the print of what it wrote, off the window's thread.
fn run(work: Work, path: &Path, doc: &Document) -> Outcome {
    let (bytes, placed) = match work {
        Work::Save(expect) => files::save_checked(path, doc, expect)?,
        Work::Upgrade(version, expect) => files::save_upgrading(path, version, doc, expect)?,
    };
    let print = pikslar_rewind::hash128(&bytes);
    Ok(Saved {
        print,
        seen: Some(Seen::after_save(placed.stamp, print)),
        backup_left: placed.backup_left,
    })
}

/// Said after a save whose backup of the original could not be removed.
fn backup_left_said(backup: &Path) -> String {
    format!(
        "Saved. The copy of the file from before this save is still at {}: it could not be \
         removed, perhaps because another program has it open. Delete it when you no longer \
         need it",
        backup.display()
    )
}

impl App {
    /// Starts a save, or queues one after the save under way. Does not wait.
    pub(crate) fn start_save(&mut self, window: Window, choose_path: bool) -> Started {
        if self.save_job.is_some() {
            return self.queue_save(window, choose_path);
        }
        if let Some(started) = self.start_held(window) {
            return started;
        }
        if !self.ready_to_save() {
            return Started::No;
        }
        let Some(path) = self.save_target(window, choose_path) else {
            return Started::No;
        };
        if choose_path {
            note(Event::new("save-as", "chosen").file(&path));
        }
        self.start_save_to(window, path)
    }

    fn queue_save(&mut self, window: Window, choose_path: bool) -> Started {
        let next = if choose_path {
            match self.choose_target(window, self.save_start().as_deref()) {
                Some(path) => FollowUp::To(path),
                None => return Started::No,
            }
        } else {
            FollowUp::Save
        };
        // A place chosen is kept over a plain save, which it does as well.
        if !(next == FollowUp::Save && matches!(self.save_next, Some(FollowUp::To(_)))) {
            self.save_next = Some(next);
        }
        note(Event::new("save", "queued"));
        Started::Queued
    }

    /// Starts saving to a path already chosen, asking first if an older version made the
    /// project there.
    pub(crate) fn start_save_to(&mut self, window: Window, path: PathBuf) -> Started {
        if !self.backup_allows_save(window, &path) {
            return Started::No;
        }
        let expect = match self.check_outside(window, &path) {
            Outside::Go(expect) => expect,
            Outside::SaveAs => return self.save_copy(window, &path),
            Outside::Cancel => return Started::No,
        };
        let older = upgrade::older_version(&path);
        let kind = if older.is_some() {
            "upgrade-save"
        } else {
            "save"
        };
        note(Event::new(kind, "start").file(&path));
        self.save_started();
        let before = self.prepare_rewind_save(&path);
        let work = match older {
            None => Work::Save(expect),
            Some(version) => match self.ask_to_upgrade(window, &path, version) {
                UpgradeChoice::Upgrade => Work::Upgrade(version, expect),
                choice => {
                    let copy = choice == UpgradeChoice::SaveCopy;
                    let outcome = if copy { "copy-instead" } else { "cancel" };
                    note(Event::new(kind, outcome).file(&path));
                    self.rewind_not_saved(before);
                    self.save_cancelled();
                    return match copy {
                        true => self.save_copy(window, &path),
                        false => Started::No,
                    };
                }
            },
        };
        self.spawn_save(window, (path, kind), work, before);
        Started::Now
    }

    /// Takes the copy and starts the thread; saves here if no thread can start.
    fn spawn_save(
        &mut self,
        window: Window,
        (path, kind): (PathBuf, &'static str),
        work: Work,
        before: Option<RewindBefore>,
    ) {
        let copied = Instant::now();
        let doc = self.editor.doc().clone();
        let copy = self.editor.begin_saved_copy();
        note(Event::new(kind, "snapshot").took(copied));
        let slot: Slot = Arc::new(Mutex::new(None));
        let (put, target, waker) = (Arc::clone(&slot), path.clone(), window.waker());
        let delay = self.save_delay;
        // Windows ending the session waits for this save (`session_end.rs`).
        let pending = pikslar_platform::write_pending(pikslar_platform::WriteKind::Save);
        let spawned = std::thread::Builder::new()
            .name("save".into())
            .spawn(move || {
                let _pending = pending;
                if let Some(delay) = delay {
                    std::thread::sleep(delay);
                }
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(work, &target, &doc)
                }))
                .unwrap_or_else(|_| Err(files::FileError::Unverified.into()));
                // In the slot before the wake, so the frame it causes finds it.
                *locked(&put) = Some(outcome);
                waker.wake();
            });
        let worker = match spawned {
            Ok(worker) => Some(worker),
            Err(e) => {
                note(Event::new(kind, "no-thread").io(&e));
                *locked(&slot) = Some(run(work, &path, self.editor.doc()));
                None
            }
        };
        self.save_job = Some(SaveJob {
            path,
            kind,
            revision: self.editor.revision(),
            copy,
            document: self.documents,
            before,
            started: copied,
            slot,
            worker,
        });
        self.ui_dirty = true;
    }

    /// Takes a finished save, then starts the one it queued once nothing is in its way. Does
    /// not wait.
    pub(crate) fn collect_save(&mut self, window: Window) {
        if self.save_job.as_ref().is_some_and(SaveJob::is_done) {
            self.take_finished_save();
        }
        if self.save_job.is_none() {
            self.start_follow_up(window);
        }
    }

    fn take_finished_save(&mut self) {
        let Some(mut job) = self.save_job.take() else {
            return;
        };
        if let Some(worker) = job.worker.take() {
            // Its result is already in the slot, so this does not wait.
            let _ = worker.join();
        }
        let outcome = locked(&job.slot)
            .take()
            .unwrap_or_else(|| Err(files::FileError::Unverified.into()));
        self.save_ended(job, outcome);
    }

    /// What keeps a queued save from starting now: a name being typed, a dialog, a menu or a
    /// held mouse button. A queued save never asks a question in the middle of a drag.
    pub(crate) fn follow_up_waits(&self) -> Option<SaveBlocker> {
        self.save_blocker()
    }

    /// Starts the queued save, unless something is in its way or it is held.
    fn start_follow_up(&mut self, window: Window) {
        if self.save.next_held || self.follow_up_waits().is_some() {
            return;
        }
        match self.save_next.take() {
            Some(FollowUp::Save) => {
                self.start_save(window, false);
            }
            Some(FollowUp::To(path)) => {
                self.save_next = Some(FollowUp::To(path));
                self.save.next_held = true;
                self.start_held(window);
            }
            None => {}
        }
    }

    /// A queued save to a chosen place that could not start, started now if the moved pixels
    /// can be put down; kept for the next Save if not. None if there is none.
    fn start_held(&mut self, window: Window) -> Option<Started> {
        if !std::mem::take(&mut self.save.next_held) {
            return None;
        }
        let Some(FollowUp::To(path)) = self.save_next.take() else {
            return None;
        };
        if !self.ready_to_save() {
            self.save_next = Some(FollowUp::To(path));
            self.save.next_held = true;
            return Some(Started::No);
        }
        Some(self.start_save_to(window, path))
    }

    fn save_ended(&mut self, job: SaveJob, outcome: Outcome) {
        let same = job.document == self.documents;
        match outcome {
            Ok(saved) => {
                note(Event::new(job.kind, "ok").took(job.started).file(&job.path));
                if !same {
                    return self.notify(format!("Saved {}", job.path.display()));
                }
                let current = self.editor.revision() == job.revision;
                self.saved_to(job.path, (job.revision, job.copy));
                if let Some(backup) = saved.backup_left.clone() {
                    self.notify(backup_left_said(&backup));
                    // The older version, left by this save: the next save may replace it.
                    self.backup_known_old(backup);
                }
                self.seen = saved.seen;
                self.save_done();
                // After the file is safe, never before: nothing here can fail the save. Edits
                // since the copy are not in this file; the next save keeps their steps.
                if current {
                    self.rewind_saved(saved.print);
                }
                crate::fault::pause_after_save();
            }
            Err(e) => {
                let failed = Event::new(job.kind, "fail").took(job.started);
                note(failed.failed_at(&e).file(&job.path));
                if !same {
                    // The document it saved is gone: its failure is not the new one's.
                    let shown = job.path.display();
                    return self.notify_error(format!("Could not save {shown}: {}", e.error));
                }
                self.rewind_not_saved(job.before);
                self.save_ended_failed(&job.path, e.error);
            }
        }
    }

    /// A save to `path` failed with `error`. When it left other bytes there (a file that did not
    /// read back and could not be put back), the file is taken as this session saw it last, so
    /// the next save does not blame another program.
    pub(crate) fn save_ended_failed(&mut self, path: &Path, error: FileError) {
        let changed = match &error {
            FileError::Format(f) => not_kept(f).is_some_and(NotKept::target_changed),
            _ => false,
        };
        let own = self
            .path
            .as_deref()
            .is_some_and(|p| files::same_file(p, path));
        if changed && own {
            self.seen = Seen::of(path, None);
        }
        // A backup kept for the only good copy is no longer one a save may replace.
        if let FileError::Format(f) = &error {
            for kept in not_kept(f).and_then(NotKept::backup).iter() {
                self.backup_unknown(kept);
            }
        }
        let said = error.to_string();
        self.save_failed(said.clone());
        // Said in a box over the next frame, once the status bar no longer says Saving.
        self.save.to_report = Some(said);
        self.save.report_drawn = false;
    }

    /// Waits for the save under way and the one it queued, for at most `within`, with the
    /// window answering meanwhile. True once none is left.
    pub(crate) fn wait_for_saves(&mut self, window: Window, within: Duration) -> bool {
        let end = Instant::now() + within;
        while let Some(job) = &self.save_job {
            let slot = Arc::clone(&job.slot);
            pikslar_platform::wait_answering(
                window,
                || locked(&slot).is_none() && Instant::now() < end,
                |_| {},
            );
            if locked(&slot).is_none() {
                return false;
            }
            self.collect_save(window);
        }
        true
    }

    /// Saves and waits for the file to be in place. True if it was written.
    pub fn save(&mut self, window: Window, choose_path: bool) -> bool {
        self.save_and_wait(window, |a| a.start_save(window, choose_path))
    }

    /// Saves to a path already chosen and waits for it. True if it was written.
    #[cfg(test)]
    pub(crate) fn save_to(&mut self, window: Window, path: PathBuf) -> bool {
        self.save_and_wait(window, |a| a.start_save_to(window, path))
    }

    fn save_and_wait(&mut self, window: Window, start: impl FnOnce(&mut App) -> Started) -> bool {
        if !self.wait_for_saves(window, SAVE_WAIT) {
            self.notify_error(STILL_SAVING.to_string());
            return false;
        }
        if start(self) == Started::No {
            return false;
        }
        if !self.wait_for_saves(window, SAVE_WAIT) {
            self.notify_error(STILL_SAVING.to_string());
            return false;
        }
        self.last_save_worked()
    }

    fn ask_to_upgrade(&self, window: Window, path: &Path, version: u32) -> UpgradeChoice {
        let in_the_way = upgrade::backup_in_the_way(path, version);
        let opened_here = self
            .path
            .as_deref()
            .is_some_and(|p| files::same_file(p, path));
        let question = files::upgrade_question(path, version, opened_here, in_the_way);
        ask_upgrade(window, &question, !in_the_way)
    }

    /// Saves under a new name, leaving the older project where it is untouched.
    fn save_copy(&mut self, window: Window, older: &Path) -> Started {
        match self.choose_target(window, Some(older)) {
            Some(path) => self.start_save_to(window, path),
            None => Started::No,
        }
    }

    /// The file at `path` now holds the document as it was at `revision`.
    fn saved_to(&mut self, path: PathBuf, (revision, copy): (u64, SavedCopy)) {
        self.notify(format!("Saved {}", path.display()));
        self.recovery.note_origin(Some(&path));
        crate::settings::remember(&mut self.recent, &path);
        self.path = Some(path);
        self.imported_from = None;
        self.saved_revision = revision;
        self.editor.finish_saved_copy(copy);
        if self.editor.revision() == revision {
            self.recovery.clear();
            self.recovered = self.recovery_stamp();
            self.recovery_due = None;
        }
    }
}

#[cfg(test)]
#[path = "save_run_tests.rs"]
mod tests;
