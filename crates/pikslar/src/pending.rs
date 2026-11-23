//! Waits on the window's thread are bounded (B1, section 13, requirement 6).
//!
//! The window waits a moment for the backup writer, a Rewind write or a stopped read to end.
//! Past that it goes on answering, the status line says "Still writing the backup..." or
//! "Still writing history...", and the frame loop finishes what was left once the thread
//! ends. Closing waits longer, still bounded, and notes in the diagnostics what it left.

use crate::app::App;
use std::time::{Duration, Instant};

/// The longest the window's thread waits for another thread before leaving it to the frame.
pub(crate) const UI_WAIT: Duration = Duration::from_millis(100);
/// The longest a closing session waits for a write under way.
pub(crate) const QUIT_WAIT: Duration = Duration::from_secs(3);
/// How often a frame looks again while it has a wait to finish.
const POLL: Duration = Duration::from_millis(50);

pub(crate) const STILL_BACKUP: &str = "Still writing the backup...";
pub(crate) const STILL_HISTORY: &str = "Still writing history...";

/// Waits up to `within` for `done`. True once it is.
pub(crate) fn wait_until(within: Duration, done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + within;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

impl App {
    /// What a wait left to the frame loop is for, if one is.
    fn pending_text(&self) -> Option<&'static str> {
        if self.recovery.clear_waiting() {
            Some(STILL_BACKUP)
        } else if self.rewind.waiting() {
            Some(STILL_HISTORY)
        } else {
            None
        }
    }

    /// Says a wait was left to the frame loop.
    pub(crate) fn say_pending(&mut self, text: &'static str) {
        self.notify(text.to_string());
    }

    /// Each frame: keeps saying what is still being written while nothing else is said, and
    /// takes the words down once it is done.
    pub(crate) fn tick_pending(&mut self) {
        let showing = self.message.as_ref().map(|(t, _)| t.as_str());
        match self.pending_text() {
            Some(text) if showing.is_none() => self.say_pending(text),
            None if showing.is_some_and(|t| t == STILL_BACKUP || t == STILL_HISTORY) => {
                self.clear_message()
            }
            _ => {}
        }
    }

    /// When the next frame should look again at a wait left to it.
    pub(crate) fn pending_wait(&self) -> Option<Duration> {
        let waiting = self.pending_text().is_some() || self.open_after_stop.is_some();
        waiting.then_some(POLL)
    }
}

#[cfg(test)]
#[path = "pending_tests.rs"]
mod tests;
