//! What the status line says: news, which goes by itself, and errors, which wait to be seen.

use super::App;
use std::time::{Duration, Instant};

const MESSAGE_LIFETIME: Duration = Duration::from_secs(4);

/// News stays four seconds, or longer when it is a long sentence to read.
fn message_lifetime(text: &str) -> Duration {
    let chars = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
    (Duration::from_secs(1) + Duration::from_millis(60) * chars)
        .clamp(MESSAGE_LIFETIME, Duration::from_secs(15))
}

impl App {
    pub fn notify(&mut self, text: String) {
        // News does not erase an error nobody has answered yet: it waits for the next input,
        // which takes the error down, and is said then. Only the latest waits.
        if self.message_is_error && self.message.is_some() {
            self.held_news = Some(text);
            return;
        }
        let lifetime = message_lifetime(&text);
        self.message = Some((text, Instant::now() + lifetime));
        self.message_is_error = false;
        self.ui_dirty = true;
    }

    /// Says something went wrong. It carries "Error:" so the difference is in the words, and it
    /// stays until the next key or click.
    pub fn notify_error(&mut self, text: String) {
        self.message = Some((format!("Error: {text}"), Instant::now() + MESSAGE_LIFETIME));
        self.message_is_error = true;
        self.ui_dirty = true;
    }

    /// Takes whatever the status line says down, and the mark of an error with it.
    pub(crate) fn clear_message(&mut self) {
        self.message = None;
        self.message_is_error = false;
        self.held_news = None;
        self.ui_dirty = true;
    }

    /// News that a save worked, held behind an error, is dropped: a later save has started or
    /// failed, so it would say Saved about a file that may not be.
    pub(crate) fn forget_held_save_news(&mut self) {
        if self
            .held_news
            .as_deref()
            .is_some_and(|n| n.starts_with("Saved"))
        {
            self.held_news = None;
        }
    }

    /// Takes an error down when the person does something else.
    pub(crate) fn dismiss_error(&mut self) {
        if std::mem::take(&mut self.message_is_error) {
            let held = self.held_news.take();
            self.clear_message();
            if let Some(news) = held {
                self.notify(news);
            }
        }
    }
}

#[cfg(test)]
#[path = "messages_tests.rs"]
mod tests;
