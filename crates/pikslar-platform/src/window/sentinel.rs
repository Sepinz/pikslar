//! A marker a test harness posts behind its input, to learn when the window has read it all.
//!
//! The soak (spikes/hands.ps1) posts keys and sets modifiers separately, and the window reads
//! the modifiers when it takes each key off the queue. Before changing them the harness must
//! know every key posted earlier has been taken. It posts this registered message with a
//! number after them (`lParam` 0), and asks, by sending the same message with `lParam` 1,
//! which number the window last took. Posted messages leave the queue in order, so once the
//! answer reaches the number, everything posted before it has been read.
//!
//! Nothing else uses the message. Without a harness, the window compares one number per
//! message and does nothing more.

use crate::ffi::{wide, RegisterWindowMessageW};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// The name both sides register.
pub const NAME: &str = "Pikslar.HarnessSentinel";

static MESSAGE: AtomicU32 = AtomicU32::new(0);
static SEEN: AtomicUsize = AtomicUsize::new(0);

fn message() -> u32 {
    let known = MESSAGE.load(Ordering::Relaxed);
    if known != 0 {
        return known;
    }
    let name = wide(NAME);
    // SAFETY: a NUL-terminated string that outlives the call.
    let id = unsafe { RegisterWindowMessageW(name.as_ptr()) };
    MESSAGE.store(id, Ordering::Relaxed);
    id
}

/// What to answer for `msg`, if it is the sentinel.
pub(crate) fn answer(msg: u32, w: usize, l: isize) -> Option<isize> {
    // Registered messages are 0xC000 and above; nothing below can be the sentinel.
    if msg < 0xC000 || msg != message() {
        return None;
    }
    Some(reply(&SEEN, w, l))
}

/// Posted (`l` 0): note `w` as taken. Asked (`l` 1): the last number taken.
fn reply(seen: &AtomicUsize, w: usize, l: isize) -> isize {
    if l == 0 {
        seen.store(w, Ordering::Relaxed);
        0
    } else {
        seen.load(Ordering::Relaxed) as isize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_posted_number_is_kept_and_then_told() {
        let seen = AtomicUsize::new(0);
        assert_eq!(reply(&seen, 0, 1), 0, "nothing taken yet");
        assert_eq!(reply(&seen, 41, 0), 0);
        assert_eq!(reply(&seen, 42, 0), 0);
        assert_eq!(reply(&seen, 0, 1), 42, "the last one taken");
    }

    #[test]
    fn ordinary_messages_are_not_the_sentinel() {
        for msg in [0u32, 0x0100, 0x0102, 0x0400, 0x8000, 0xBFFF] {
            assert_eq!(answer(msg, 7, 0), None);
        }
    }

    #[test]
    fn the_registered_message_is_answered() {
        let id = message();
        assert!(id >= 0xC000, "registered: {id:#x}");
        assert_eq!(answer(id, 9, 0), Some(0));
        assert_eq!(answer(id, 0, 1), Some(9));
    }
}
