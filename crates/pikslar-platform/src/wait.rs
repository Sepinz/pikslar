//! Waiting for work on another thread without the window stopping answering.
//!
//! The handler is busy while it waits, so nothing reaches it: the window is repainted from
//! what was last drawn, and presses are handed to the caller, who can use one to stop the work.
//! Other input (moves, releases, typed characters, the wheel) is dropped, so nothing done
//! while waiting is replayed afterwards.

use crate::ffi::*;
use crate::window::{button_event, key_of, modifiers, point, Window};
use crate::{Event, Key};
use std::mem::zeroed;
use std::ptr::null;

/// How often `working` is asked again when no message arrives.
const POLL_MS: u32 = 15;

/// Waits while `working()` is true. Each press of a mouse button or a key on the window is
/// given to `press`; everything else that is not input is handled as usual. A request to quit
/// is put back for the main loop and ends the wait at once, with the work maybe still going:
/// the caller must then stop it (the Scripts panel cancels the run) rather than wait for it
/// unanswered. A detached window only waits.
///
/// Nothing dispatched here reaches the handler: the window procedure hands messages to it only
/// through the frame, which is the caller and is marked busy, so sizes, rescales, wakes and
/// session questions are queued or refused, and a repaint copies the last drawn bitmap. The
/// caller's state, lent to the work meanwhile, is never touched from here.
pub fn wait_answering(
    window: Window,
    mut working: impl FnMut() -> bool,
    mut press: impl FnMut(Event),
) {
    while working() {
        if window.is_detached() {
            std::thread::sleep(std::time::Duration::from_millis(u64::from(POLL_MS)));
            continue;
        }
        // SAFETY: plain calls on this thread's own queue; `msg` is a valid out pointer for
        // each peek, and dispatching it is what the main loop does with the same message.
        unsafe {
            MsgWaitForMultipleObjectsEx(0, null(), POLL_MS, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            let mut msg: MSG = zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    PostQuitMessage(msg.w as i32);
                    return;
                }
                if let Some(event) = pressed(window, &msg) {
                    press(event);
                } else if !is_input(msg.message) {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }
}

/// A press on `window`, as an event.
fn pressed(window: Window, msg: &MSG) -> Option<Event> {
    if msg.hwnd != window.raw() {
        return None;
    }
    match msg.message {
        WM_KEYDOWN => key_of(msg.w).map(|key: Key| Event::KeyDown {
            key,
            mods: modifiers(),
            repeat: msg.l & (1 << 30) != 0,
        }),
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let (x, y) = point(msg.l);
            button_event(msg.message, x, y)
        }
        _ => None,
    }
}

/// Pointer and keyboard messages, which are dropped while waiting.
fn is_input(message: u32) -> bool {
    (WM_KEYDOWN..=WM_SYSKEYUP + 4).contains(&message)
        || (WM_MOUSEMOVE..=WM_MBUTTONUP + 6).contains(&message)
}
