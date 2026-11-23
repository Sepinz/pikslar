//! Fitting text into a width: cut with an ellipsis, never silently.

use crate::font::{advance, text_width};
use std::borrow::Cow;

/// The longest start of `text` that fits, broken only between characters.
///
/// One pass over the characters, so even a very long text costs no more than reading it once.
fn longest_start(text: &str, width: i32, scale: i32) -> &str {
    let scale = i64::from(scale);
    let mut used = 0i64;
    for (at, ch) in text.char_indices() {
        used += i64::from(advance(ch)) * scale;
        // As `text_width`: each character a whole advance, less the gap after the last one.
        if used - scale > i64::from(width) {
            return &text[..at];
        }
    }
    text
}

/// `text` as it fits `width`. When it does not fit, the start that fits with an ellipsis after
/// it, so a cut is never silent. If not even the ellipsis fits, as many dots as do.
pub fn fit_to_width(text: &str, width: i32, scale: i32) -> Cow<'_, str> {
    let start = longest_start(text, width, scale);
    if start.len() == text.len() {
        return Cow::Borrowed(text);
    }
    // The start and the dots join into one text: its gap is counted once, not twice.
    let room = width - text_width("...", scale) - scale;
    if room < 0 {
        return Cow::Borrowed(longest_start("...", width, scale));
    }
    Cow::Owned(format!("{}...", longest_start(text, room, scale)))
}

/// A short text built on the stack, so a readout drawn every frame allocates nothing. Text
/// past `N` bytes is cut at a character boundary.
pub struct StackText<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> StackText<N> {
    /// Formats `args` into a new buffer.
    pub fn new(args: std::fmt::Arguments<'_>) -> Self {
        let mut text = StackText {
            buf: [0; N],
            len: 0,
        };
        // The writer never fails: it cuts instead.
        let _ = std::fmt::Write::write_fmt(&mut text, args);
        text
    }

    /// What was written.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> std::fmt::Write for StackText<N> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let room = N - self.len;
        let mut take = s.len().min(room);
        while !s.is_char_boundary(take) {
            take -= 1;
        }
        self.buf[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
        self.len += take;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_text_formats_and_cuts_at_a_character() {
        assert_eq!(
            StackText::<16>::new(format_args!("{}/{}", 7, 9)).as_str(),
            "7/9"
        );
        let cut = StackText::<4>::new(format_args!("ab\u{e9}\u{e9}"));
        assert_eq!(cut.as_str(), "ab\u{e9}");
    }
}
