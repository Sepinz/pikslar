//! Ctrl+S under a native message box is not dropped without a word (B1, section 13, requirement
//! 2). The box runs its own message loop, which the app cannot see, so while one is open this
//! thread has a `WH_MSGFILTER` hook (never a global one: it is made for this thread's id, and
//! removed when the box closes). A Ctrl+S key press it sees sounds the "exclamation" beep. The
//! message goes on to the box as before, so nothing is bypassed, answered or closed for it.

use crate::ffi::{H, MSG};
use std::ptr::null_mut;

const WH_MSGFILTER: i32 = -1;
const WM_KEYDOWN: u32 = 0x0100;
const VK_CONTROL: i32 = 0x11;
const MB_ICONEXCLAMATION: u32 = 0x30;
/// The key was already down: a held key repeating.
const WAS_DOWN: isize = 1 << 30;

#[link(name = "user32")]
extern "system" {
    fn SetWindowsHookExW(id: i32, f: HookProc, module: H, thread: u32) -> H;
    fn UnhookWindowsHookEx(hook: H) -> i32;
    fn CallNextHookEx(hook: H, code: i32, w: usize, l: isize) -> isize;
    fn MessageBeep(kind: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
}

type HookProc = unsafe extern "system" fn(i32, usize, isize) -> isize;

/// True for the first press of S with Ctrl held.
fn is_save_press(message: u32, key: usize, flags: isize, ctrl: bool) -> bool {
    message == WM_KEYDOWN && key == usize::from(b'S') && ctrl && flags & WAS_DOWN == 0
}

unsafe extern "system" fn filter(code: i32, w: usize, l: isize) -> isize {
    if code >= 0 && l != 0 {
        // SAFETY: for a non-negative code the system passes a valid MSG in `l`.
        let msg = &*(l as *const MSG);
        // SAFETY: reads the state of a key on this thread's input queue.
        let ctrl = crate::ffi::GetKeyState(VK_CONTROL) < 0;
        if is_save_press(msg.message, msg.w, msg.l, ctrl) {
            // SAFETY: plain call.
            MessageBeep(MB_ICONEXCLAMATION);
        }
    }
    // SAFETY: the hook handle is ignored by Windows; the message goes on unchanged.
    CallNextHookEx(null_mut(), code, w, l)
}

/// The hook, removed when dropped.
pub(crate) struct BoxKeys(H);

impl BoxKeys {
    /// Hooks this thread's message boxes. Without a hook, a box simply works as before.
    pub(crate) fn watch() -> Option<BoxKeys> {
        // SAFETY: `filter` has the signature a hook needs and lives for the program; a thread
        // id makes the hook this thread's only.
        let hook =
            unsafe { SetWindowsHookExW(WH_MSGFILTER, filter, null_mut(), GetCurrentThreadId()) };
        (!hook.is_null()).then_some(BoxKeys(hook))
    }
}

impl Drop for BoxKeys {
    fn drop(&mut self) {
        // SAFETY: the handle came from `SetWindowsHookExW` and is removed once.
        unsafe { UnhookWindowsHookEx(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_press_of_ctrl_s_is_a_save_press() {
        let s = usize::from(b'S');
        assert!(is_save_press(WM_KEYDOWN, s, 0, true));
        assert!(!is_save_press(WM_KEYDOWN, s, WAS_DOWN, true), "repeat");
        assert!(!is_save_press(WM_KEYDOWN, s, 0, false), "no Ctrl");
        assert!(!is_save_press(WM_KEYDOWN, usize::from(b'A'), 0, true));
        assert!(!is_save_press(0x0101, s, 0, true), "key up");
    }

    #[test]
    fn a_hook_for_this_thread_can_be_made_and_removed() {
        let hook = BoxKeys::watch();
        assert!(hook.is_some());
        drop(hook);
    }
}
