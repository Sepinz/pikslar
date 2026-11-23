//! A last backup copy for Windows ending the session while the window is busy higher up the
//! stack: a message box, a file dialog or a wait that answers the window (B1, requirement 7).
//!
//! The app cannot be touched then, so each frame with unsaved work keeps a snapshot aside:
//! the document shares its pixels, so it costs about nothing. The platform calls the fallback
//! registered here, which writes that snapshot as the recovery copy. Edits made by the same
//! input that opened the box, before it opened, are not in the snapshot.

use crate::app::{App, Gesture};
use crate::diag::{note, Event};
use crate::recovery::{CopyFiles, Origin};
use pikslar_core::{Document, FloatingSnapshot};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The work as of the last frame, and where its copy goes.
pub(crate) struct Standing {
    stamp: (u64, u64, u64),
    doc: Document,
    floating: Option<FloatingSnapshot>,
    path: Option<PathBuf>,
    protected: Option<PathBuf>,
    files: CopyFiles,
}

/// Shared by the app, which keeps it up to date, and the platform's fallback, which writes it.
pub(crate) type Kept = Rc<RefCell<Option<Standing>>>;

/// A new slot, with the fallback that writes it registered for this thread's window.
pub(crate) fn register() -> Kept {
    let kept: Kept = Rc::default();
    let shared = Rc::clone(&kept);
    let mut written = None;
    pikslar_platform::set_session_fallback(Box::new(move |wait| {
        let Ok(kept) = shared.try_borrow() else {
            return false;
        };
        match kept.as_ref() {
            None => true,
            Some(s) if written == Some(s.stamp) => true,
            Some(s) => {
                let ok = write(s, wait);
                if ok {
                    written = Some(s.stamp);
                }
                ok
            }
        }
    }));
    kept
}

/// Writes what was kept, once a recovery copy already being written has ended, waiting at most
/// `wait` for it: it writes the same files, and what is kept is as new as what that copy holds
/// (a frame keeps its snapshot right after it starts a write), so this never puts older work
/// over a finished copy. A save under way is not waited for: it may be slow. Past `wait`
/// nothing is written, and the copy there stays whole (it is renamed into place), if older.
fn write(s: &Standing, wait: Duration) -> bool {
    let started = Instant::now();
    // A backup being written now writes the same files; none can start while the window is
    // busy, so once it ends this one has them. A save running meanwhile writes other files.
    if !pikslar_platform::wait_for_recovery_writes(wait) {
        note(Event::new("recovery-now", "fail").phase("busy-writing"));
        return false;
    }
    let origin = Origin {
        path: s.path.as_deref(),
        protected: s.protected.as_deref(),
    };
    match s.files.write(&s.doc, s.floating.as_ref(), origin) {
        Ok(()) => {
            note(
                Event::new("recovery-now", "ok")
                    .phase("in-dialog")
                    .took(started),
            );
            true
        }
        Err(e) => {
            note(Event::new("recovery-now", "fail").phase("in-dialog").io(&e));
            false
        }
    }
}

impl App {
    /// Each frame: keeps the snapshot the fallback writes as the work is now. A stroke's
    /// pixels are not in the document yet, so the one from before it stays.
    pub(crate) fn keep_standing(&mut self) {
        if self.gesture == Gesture::Stroke {
            return;
        }
        let stamp = self.recovery_stamp();
        let fresh = match self.recovery.copy_files() {
            Some(files) if self.has_unsaved_work() => Some((files, stamp)),
            _ => None,
        };
        let Ok(mut kept) = self.standing.try_borrow_mut() else {
            return;
        };
        let Some((files, stamp)) = fresh else {
            *kept = None;
            return;
        };
        if kept
            .as_ref()
            .is_some_and(|s| s.stamp == stamp && s.files == files)
        {
            return;
        }
        *kept = Some(Standing {
            stamp,
            doc: self.editor.doc().clone(),
            floating: self.editor.floating_snapshot(),
            path: self.path.clone(),
            protected: self.protected_path.clone(),
            files,
        });
    }
}

#[cfg(test)]
#[path = "standing_tests.rs"]
mod tests;
