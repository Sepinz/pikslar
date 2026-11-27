//! Opening a file on another thread, so a large one never freezes the window.
//!
//! The question about unsaved work is asked first, when the person chooses to open something;
//! after that one read runs at a time. The window keeps answering while it runs, the status line
//! says what is being opened, and Esc stops it. A read that was stopped, or replaced by a newer
//! one, is asked to stop and its result is thrown away; the next read waits for it on its own
//! thread, so two never hold memory at once and the window never waits for either. Nothing
//! polls: the thread puts its result in a slot and only then wakes the window. If the drawing
//! changed since the question, the result is dropped rather than replace work nobody was asked
//! about.

use crate::app::{App, Gesture};
use crate::files::{self, FileError, Opened};
use crate::pending::{wait_until, UI_WAIT};
use crate::session;
use pikslar_platform::Window;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

type Worker = JoinHandle<()>;
type Slot = Arc<Mutex<Option<Result<Opened, FileError>>>>;
/// What the thread calls once its result is in the slot.
type Wake = Box<dyn FnOnce() + Send>;

/// What to do once the file has been opened, or could not be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Then {
    Nothing,
    /// From the recent files: one that is gone leaves the list.
    ForgetIfGone,
    /// Several files were dropped and only this one is opened, which is said once it is.
    SayOneAtATime,
}

/// A file being read on another thread.
pub(crate) struct Loading {
    pub(crate) path: PathBuf,
    pub(crate) then: Then,
    /// What the status line says meanwhile.
    pub(crate) label: String,
    /// The drawing as it was when the person was asked about unsaved work.
    asked: (u64, u64, u64),
    stop: Arc<AtomicBool>,
    slot: Slot,
    worker: Worker,
}

/// A thread could not be started: what to open on this one, and the read to wait for first.
struct Unspawned {
    path: PathBuf,
    then: Then,
    before: Option<Worker>,
}

fn locked(slot: &Slot) -> MutexGuard<'_, Option<Result<Opened, FileError>>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

fn unreadable() -> FileError {
    FileError::Format(pikslar_format::FormatError::Io(std::io::Error::other(
        "the file could not be read",
    )))
}

impl Loading {
    /// Starts the thread. If none can be started, hands back what to open on this one.
    fn start(
        path: PathBuf,
        then: Then,
        wake: Wake,
        before: Option<Worker>,
        asked: (u64, u64, u64),
        print: bool,
    ) -> Result<Loading, Unspawned> {
        let stop = Arc::new(AtomicBool::new(false));
        let slot: Slot = Arc::new(Mutex::new(None));
        let (reading, put, told) = (path.clone(), Arc::clone(&slot), Arc::clone(&stop));
        // Kept outside the thread until it runs, so a thread that never starts gives it back.
        let earlier = Arc::new(Mutex::new(before));
        let waiting = Arc::clone(&earlier);
        let spawned = if refuse_thread() {
            Err(std::io::Error::other("no thread, for a test"))
        } else {
            std::thread::Builder::new()
                .name("open".into())
                .spawn(move || {
                    if let Some(before) = waiting
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take()
                    {
                        // Its result was thrown away; waiting keeps one read's memory at a time.
                        let _ = before.join();
                    }
                    // A panic only unwinds in builds that do not abort on one (tests, development);
                    // release builds end the program, so this is not a protection there. Where it
                    // does unwind, the window is still woken and says the file could not be read.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        files::open_printed(&reading, &|| told.load(Ordering::Relaxed), print)
                    }))
                    .unwrap_or_else(|_| Err(unreadable()));
                    // The result is in place before the wake, so the frame it causes finds it.
                    *locked(&put) = Some(result);
                    wake();
                })
        };
        let Ok(worker) = spawned else {
            let before = earlier
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            return Err(Unspawned { path, then, before });
        };
        let label = format!(
            "Opening {}... Esc stops it",
            crate::settings_ui::recent_label(&path)
        );
        Ok(Loading {
            path,
            then,
            label,
            asked,
            stop,
            slot,
            worker,
        })
    }

    /// True once the result is in the slot, which is before the window is woken.
    fn is_done(&self) -> bool {
        locked(&self.slot).is_some()
    }

    /// Asks the read to stop, and hands back the thread so the next read can wait for it.
    fn stop(self) -> Worker {
        self.stop.store(true, Ordering::Relaxed);
        self.worker
    }

    /// Takes the result. Only called once the read is done, so it does not wait.
    fn finish(self) -> (PathBuf, Then, Result<Opened, FileError>) {
        let result = locked(&self.slot)
            .take()
            .unwrap_or_else(|| Err(unreadable()));
        (self.path, self.then, result)
    }
}

impl App {
    /// Asks about unsaved work, then starts reading `path`. A read already running is stopped:
    /// the newest request wins. If the person keeps their work, nothing changes at all.
    pub(crate) fn begin_open(&mut self, window: Window, path: PathBuf, then: Then) {
        if !self.confirm_discard(window) {
            return;
        }
        // Anything said from here on, an error say, is news and is shown over the label.
        self.clear_message();
        // The newest request wins over one waiting for a stopped read too.
        self.open_after_stop = None;
        let asked = self.recovery_stamp();
        let before = match self.loading.take() {
            Some(older) => Some(older.stop()),
            None => self.stopping.take(),
        };
        self.ui_dirty = true;
        let waker = window.waker();
        // Always: the print also tells a file another program only touched from one it changed.
        let print = true;
        let wake = Box::new(move || waker.wake());
        match Loading::start(path, then, wake, before, asked, print) {
            Ok(loading) => self.loading = Some(loading),
            Err(Unspawned { path, then, before }) => {
                // Two reads never overlap, even when this one is made here. One that does not
                // stop soon is left to end on its own, and the frame loop opens this one then.
                if let Some(before) = before {
                    if !wait_until(UI_WAIT, || before.is_finished()) {
                        let name = crate::settings_ui::recent_label(&path);
                        self.stopping = Some(before);
                        self.open_after_stop = Some((path, then));
                        return self.notify(format!("Opening {name} once the last read stops..."));
                    }
                    let _ = before.join();
                }
                let result = files::open(&path);
                self.after_open(window, path, then, result);
            }
        }
    }

    /// Esc while a file is being read: the document stays as it was.
    pub(crate) fn stop_opening(&mut self) {
        if let Some((path, _)) = self.open_after_stop.take() {
            let name = crate::settings_ui::recent_label(&path);
            self.notify(format!("Stopped opening {name}"));
        }
        if let Some(loading) = self.loading.take() {
            let name = crate::settings_ui::recent_label(&loading.path);
            self.stopping = Some(loading.stop());
            self.notify(format!("Stopped opening {name}"));
        }
    }

    /// Stops reading without a word, for a session that is ending: nothing may replace the
    /// document before its work is safe.
    pub(crate) fn abandon_opening(&mut self) {
        self.open_after_stop = None;
        if let Some(loading) = self.loading.take() {
            self.stopping = Some(loading.stop());
            self.ui_dirty = true;
        }
    }

    /// Takes a finished read, once nothing is under way that it would interrupt: a gesture,
    /// a held button, or a dialog or menu over the canvas. Those end with a frame of their
    /// own, so the result is collected then.
    pub(crate) fn collect_opened(&mut self, window: Window) {
        if self.stopping.as_ref().is_some_and(JoinHandle::is_finished) {
            // The result of a stopped read is let go here, with whatever memory it held.
            self.stopping = None;
        }
        let busy = self.gesture != Gesture::None || self.ui.down || self.panel_open();
        if self.stopping.is_none() && !busy && self.save_job.is_none() {
            if let Some((path, then)) = self.open_after_stop.take() {
                let result = files::open(&path);
                self.after_open(window, path, then, result);
                return;
            }
        }
        let ready = self.loading.as_ref().is_some_and(Loading::is_done);
        // A save under way may yet fail and leave the work unsaved: the open waits for it.
        if !ready || busy || self.save_job.is_some() {
            return;
        }
        if let Some(loading) = self.loading.take() {
            self.deliver(window, loading);
        }
    }

    /// Replaces the document with what was read, without a question: that was asked before the
    /// read began. If the drawing changed since, the result is dropped, never the work.
    fn deliver(&mut self, window: Window, loading: Loading) -> session::Opened {
        self.ui_dirty = true;
        if loading.asked != self.recovery_stamp() {
            let name = crate::settings_ui::recent_label(&loading.path);
            self.notify(format!(
                "Opening {name} was stopped because the drawing changed. \
                 Open it again to replace it."
            ));
            return session::Opened::Kept;
        }
        let (path, then, result) = loading.finish();
        self.after_open(window, path, then, result)
    }

    fn after_open(
        &mut self,
        window: Window,
        path: PathBuf,
        then: Then,
        result: Result<Opened, FileError>,
    ) -> session::Opened {
        let opened = self.finish_open(window, path.clone(), result);
        // Opened or missing: a backup an interrupted save left beside it is offered.
        self.offer_orphan_backup(window, &path);
        match (then, opened) {
            (Then::ForgetIfGone, session::Opened::Failed) if is_gone(&path) => {
                self.recent.retain(|p| *p != path);
            }
            (Then::SayOneAtATime, session::Opened::Now) => self.notify(format!(
                "Opened {}: Pikslar opens one file at a time",
                crate::settings_ui::recent_label(&path)
            )),
            _ => {}
        }
        opened
    }

    /// Waits for the read under way and takes its result, for tests.
    #[cfg(test)]
    pub(crate) fn wait_for_open(&mut self, window: Window) -> Option<session::Opened> {
        let loading = self.loading.take()?;
        let started = std::time::Instant::now();
        while !loading.is_done() {
            assert!(started.elapsed().as_secs() < 20, "the read ends");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Some(self.deliver(window, loading))
    }

    /// Opens a file and waits for it, for tests.
    #[cfg(test)]
    pub(crate) fn open_now(&mut self, window: Window, path: PathBuf) -> session::Opened {
        self.begin_open(window, path, Then::Nothing);
        self.wait_for_open(window)
            .unwrap_or(session::Opened::Failed)
    }

    /// Waits for a stopped read to end, for tests.
    #[cfg(test)]
    pub(crate) fn wait_for_stopped(&mut self) {
        if let Some(stopping) = self.stopping.take() {
            let _ = stopping.join();
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Set by a test so that no thread can be started for a read.
    pub(crate) static NO_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn refuse_thread() -> bool {
    NO_THREAD.with(std::cell::Cell::get)
}

#[cfg(not(test))]
fn refuse_thread() -> bool {
    false
}

/// A file that is known not to be there, rather than one that could not be checked.
fn is_gone(path: &Path) -> bool {
    matches!(path.try_exists(), Ok(false))
}

#[cfg(test)]
#[path = "loading_tests.rs"]
mod tests;
