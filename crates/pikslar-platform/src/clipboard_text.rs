//! Exchanging text with other programs through the Windows clipboard, as `CF_UNICODETEXT`: UTF-16
//! with a terminating zero. Images are in [`crate::clipboard`].

use crate::ffi::*;
use crate::window::Window;
use std::ptr::copy_nonoverlapping;

/// Text longer than this many UTF-16 units is cut when read: a name or a line of text on a
/// sprite is far shorter, and a clipboard can hold a whole book.
const MAX_UNITS: usize = 1 << 16;

/// The bytes the clipboard holds for `text`: little-endian UTF-16 and a zero unit.
fn encode_text(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// The text in clipboard bytes: up to the first zero unit or [`MAX_UNITS`] units. Unpaired
/// surrogates become U+FFFD.
fn decode_text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|&unit| unit != 0)
        .take(MAX_UNITS)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Places text on the clipboard. Returns false if the clipboard was unavailable.
pub fn copy_text(window: Window, text: &str) -> bool {
    if window.is_detached() {
        return false;
    }
    let bytes = encode_text(text);
    // SAFETY: the allocation is exactly `bytes.len()` long and is locked while written. The
    // clipboard takes ownership of it only when SetClipboardData succeeds; otherwise it is
    // freed here. The clipboard is opened and closed on this thread within this block.
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if memory.is_null() {
            return false;
        }
        let target = GlobalLock(memory) as *mut u8;
        if target.is_null() {
            GlobalFree(memory);
            return false;
        }
        copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        GlobalUnlock(memory);
        if OpenClipboard(window.raw()) == 0 {
            GlobalFree(memory);
            return false;
        }
        EmptyClipboard();
        let stored = !SetClipboardData(CF_UNICODETEXT, memory).is_null();
        if !stored {
            GlobalFree(memory);
        }
        CloseClipboard();
        stored
    }
}

/// What reading text from the clipboard found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextPaste {
    Text(String),
    /// The clipboard was read and holds no text.
    NoText,
    /// The clipboard could not be opened, as when another program has it.
    Busy,
}

/// Reads text from the clipboard.
pub fn paste_text(window: Window) -> TextPaste {
    if window.is_detached() {
        return TextPaste::Busy;
    }
    // SAFETY: the clipboard owns the handle and it stays valid while the clipboard is open.
    // The locked region is `GlobalSize` bytes long and is only read within that range, and
    // it is copied out before the clipboard is closed.
    unsafe {
        if OpenClipboard(window.raw()) == 0 {
            return TextPaste::Busy;
        }
        let mut text = TextPaste::NoText;
        if IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 {
            let data = GetClipboardData(CF_UNICODETEXT);
            let locked = if data.is_null() {
                std::ptr::null()
            } else {
                GlobalLock(data) as *const u8
            };
            if !locked.is_null() {
                let bytes = std::slice::from_raw_parts(locked, GlobalSize(data));
                text = TextPaste::Text(decode_text(bytes));
                GlobalUnlock(data);
            }
        }
        CloseClipboard();
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_utf16_with_a_zero_unit() {
        assert_eq!(encode_text(""), [0, 0]);
        assert_eq!(encode_text("Aé"), [0x41, 0, 0xe9, 0, 0, 0]);
        // Outside the basic plane: a surrogate pair.
        assert_eq!(encode_text("😀"), [0x3d, 0xd8, 0x00, 0xde, 0, 0]);
    }

    #[test]
    fn text_survives_a_round_trip() {
        for text in ["", "walk cycle", "løp 漢字 Ж", "a😀b", "two\r\nlines"] {
            assert_eq!(decode_text(&encode_text(text)), text);
        }
    }

    #[test]
    fn reading_stops_at_the_zero_and_ignores_what_follows_and_a_stray_byte() {
        let mut bytes = encode_text("abc");
        bytes.extend_from_slice(&[0x58, 0, 0x59, 0, 0x7a]);
        assert_eq!(decode_text(&bytes), "abc");
        assert_eq!(
            decode_text(&[0x61, 0, 0x62, 0, 0x63]),
            "ab",
            "no zero, odd byte"
        );
        assert_eq!(decode_text(&[]), "");
    }

    #[test]
    fn broken_surrogates_become_replacement_characters_and_never_panic() {
        assert_eq!(decode_text(&[0x00, 0xd8, 0x41, 0, 0, 0]), "\u{fffd}A");
        assert_eq!(decode_text(&[0x00, 0xdc, 0, 0]), "\u{fffd}");
    }

    #[test]
    fn a_huge_clipboard_is_cut() {
        let long = "x".repeat(MAX_UNITS + 500);
        assert_eq!(decode_text(&encode_text(&long)).chars().count(), MAX_UNITS);
    }

    #[test]
    fn detached_windows_never_touch_the_clipboard() {
        assert!(!copy_text(Window::detached(), "text"));
        assert_eq!(paste_text(Window::detached()), TextPaste::Busy);
    }

    #[test]
    fn a_busy_clipboard_and_one_without_text_are_told_apart() {
        assert_ne!(TextPaste::Busy, TextPaste::NoText);
        assert_eq!(TextPaste::Text("a".into()), TextPaste::Text("a".into()));
    }
}
