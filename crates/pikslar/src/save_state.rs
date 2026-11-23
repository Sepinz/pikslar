//! What saving is doing, said on the status bar so it can always be seen (B1, section 13,
//! requirement 1): unsaved changes, saving, saved (with the time), not saved because something
//! is in the way, or failed. "Saved" is shown only once the file is in place and has been read
//! back; never before, and never while a save is under way.
//!
//! The parts are kept apart, so one never hides another: a save under way is shown whenever
//! one runs, whatever else is said; a save kept back while one runs is said as a note beside
//! it; a failure stays said until a save works, whether or not the work is unsaved.

use crate::app::App;
use crate::save_keys::SaveBlocker;

/// Why a save cannot start while moved pixels cannot be put down.
pub(crate) const PIXELS_NOT_PLACED: &str =
    "the moved pixels could not be put down; free some memory and try again";

/// Saving's part of a session.
#[derive(Debug, Default)]
pub(crate) struct SaveStatus {
    /// A save has started and not ended: its questions are being asked, or its thread runs.
    pub(crate) saving: bool,
    /// A save was asked for and could not start, for this reason.
    pub(crate) blocked: Option<String>,
    /// The last save failed, for this reason, and the file was left as it was.
    pub(crate) failed: Option<String>,
    /// When the document was last saved this session, as the local hour and minute.
    pub(crate) saved_at: Option<(u16, u16)>,
    /// A queued save to a chosen place could not start; it waits for the next Save.
    pub(crate) next_held: bool,
    /// A failure still to be said in a message box. The box waits until a frame showing
    /// "Save failed" has been drawn, so nothing behind it still says "Saving...".
    pub(crate) to_report: Option<String>,
    /// The frame with the failure on the status bar has been drawn: the box may open now.
    pub(crate) report_drawn: bool,
}

/// What is waiting to be saved, beyond the status itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    Nothing,
    /// A save's thread runs, maybe with another queued after it.
    Running {
        queued: bool,
    },
    /// A queued save waits for this to go before it starts.
    Waiting(SaveBlocker),
}

/// How a state is shown: its words, and whether they are a warning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SaveLabel {
    pub(crate) text: String,
    pub(crate) warning: bool,
}

impl SaveLabel {
    fn plain(text: String) -> Option<SaveLabel> {
        Some(SaveLabel {
            text,
            warning: false,
        })
    }

    fn warning(text: String) -> Option<SaveLabel> {
        Some(SaveLabel {
            text,
            warning: true,
        })
    }
}

/// "Saving...", with what else waits beside it.
fn saving_label(status: &SaveStatus, queued: bool) -> Option<SaveLabel> {
    match (&status.blocked, queued) {
        (Some(why), _) => SaveLabel::warning(format!("Saving... - next save waits: {why}")),
        (None, true) => SaveLabel::plain("Saving... (another save queued)".to_string()),
        (None, false) => SaveLabel::plain("Saving...".to_string()),
    }
}

/// What the status bar says about saving, from the state, what is pending and whether there
/// is unsaved work.
pub(crate) fn label(status: &SaveStatus, pending: Pending, modified: bool) -> Option<SaveLabel> {
    match pending {
        Pending::Running { queued } => return saving_label(status, queued),
        Pending::Nothing if status.saving => return saving_label(status, false),
        Pending::Waiting(SaveBlocker::Pointer) => {
            return SaveLabel::plain("Saving after you let go".to_string())
        }
        Pending::Waiting(blocker) => {
            return SaveLabel::warning(format!("Save waits: {}", blocker.reason()))
        }
        Pending::Nothing => {}
    }
    if let Some(why) = &status.blocked {
        return SaveLabel::warning(format!("Not saved: {why}"));
    }
    if let Some(why) = &status.failed {
        return SaveLabel::warning(format!("Save failed: {why}"));
    }
    if modified {
        return SaveLabel::plain("Unsaved changes".to_string());
    }
    status
        .saved_at
        .and_then(|(h, m)| SaveLabel::plain(format!("Saved {h:02}:{m:02}")))
}

impl App {
    /// Opens the message box for a failed save once a frame saying so has been drawn: called
    /// first in every frame. The box is modal, so the window behind it keeps that frame.
    pub(crate) fn report_save_failure(&mut self, window: pikslar_platform::Window) {
        if !self.save.report_drawn {
            return;
        }
        self.save.report_drawn = false;
        if let Some(said) = self.save.to_report.take() {
            pikslar_platform::show_error(window, &said);
        }
    }

    /// Called last in every frame: a failure waiting to be said is on the frame just drawn.
    pub(crate) fn save_failure_drawn(&mut self) {
        self.save.report_drawn = self.save.to_report.is_some();
    }

    /// How soon the next frame must come for a failure waiting to be said in a box.
    pub(crate) fn report_wait(&self) -> Option<std::time::Duration> {
        self.save
            .to_report
            .as_ref()
            .map(|_| std::time::Duration::ZERO)
    }
}

impl App {
    /// What is waiting to be saved now.
    pub(crate) fn save_pending(&self) -> Pending {
        if self.save_job.is_some() {
            return Pending::Running {
                queued: self.save_next.is_some(),
            };
        }
        match (&self.save_next, self.follow_up_waits()) {
            (Some(_), Some(blocker)) if !self.save.next_held => Pending::Waiting(blocker),
            _ => Pending::Nothing,
        }
    }

    /// What the status bar says about saving now.
    pub(crate) fn save_label(&self) -> Option<SaveLabel> {
        label(&self.save, self.save_pending(), self.is_modified())
    }

    /// A save is starting: what kept the last one back no longer does.
    pub(crate) fn save_started(&mut self) {
        self.forget_held_save_news();
        self.save.saving = true;
        self.save.blocked = None;
        self.ui_dirty = true;
    }

    /// A save was asked for and cannot start now, for `why`, which says what to do.
    pub(crate) fn save_blocked(&mut self, why: String) {
        self.save.blocked = Some(why);
        self.ui_dirty = true;
    }

    /// The file is in place and read back.
    pub(crate) fn save_done(&mut self) {
        self.save.saved_at = Some(pikslar_platform::local_clock());
        self.save.saving = false;
        self.save.failed = None;
        self.ui_dirty = true;
    }

    /// The save failed for `why`; the file is as it was.
    pub(crate) fn save_failed(&mut self, why: String) {
        self.forget_held_save_news();
        self.save.saving = false;
        self.save.failed = Some(why);
        self.ui_dirty = true;
    }

    /// The save was given up before anything was written, by the person's own choice. An
    /// earlier failure is still said.
    pub(crate) fn save_cancelled(&mut self) {
        self.save.saving = false;
        self.ui_dirty = true;
    }

    /// What kept a save from starting has gone.
    pub(crate) fn save_unblocked(&mut self) {
        if self.save.blocked.take().is_some() {
            self.ui_dirty = true;
        }
    }

    /// True if the last save asked for worked.
    pub(crate) fn last_save_worked(&self) -> bool {
        !self.save.saving && self.save.failed.is_none() && self.save.blocked.is_none()
    }

    /// A new document has nothing saved this session.
    pub(crate) fn save_forget(&mut self) {
        self.save = SaveStatus::default();
    }
}

#[cfg(test)]
#[path = "save_state_tests.rs"]
mod tests;
