//! `WM_QUERYENDSESSION` and `WM_ENDSESSION` (`session_end.rs`).

use super::{process, State, Window};
use crate::ffi::H;
use crate::Handler;

/// Answers Windows' question about whether the session may end. A modal dialog or a wait open
/// higher up the stack owns the handler, so it is not touched: the fallback the app gave
/// writes the work it kept aside, and `WM_ENDSESSION` waits for the writes under way.
///
/// # Safety
/// `s` must point to the live state and the call must be on the window's thread.
pub(super) unsafe fn session_ending<H2: Handler>(s: *mut State<H2>, hwnd: H) -> isize {
    let allowed = if (*s).busy {
        crate::session_end::run_session_fallback(crate::session_end::QUERY_FALLBACK_WAIT)
    } else {
        process(s);
        (*s).busy = true;
        let allowed = (*s).handler.session_ending(Window(hwnd));
        (*s).busy = false;
        allowed
    };
    crate::session_end::block_while_writing(hwnd);
    isize::from(allowed)
}

/// The session ends now, or the end was called off: writes under way end or are left within
/// the time Windows gives, and the reason comes down.
///
/// # Safety
/// As [`session_ending`].
pub(super) unsafe fn session_ended<H2: Handler>(s: *mut State<H2>, hwnd: H, ending: bool) {
    if ending {
        crate::session_end::begin_end();
    }
    if !(*s).busy {
        process(s);
        (*s).busy = true;
        (*s).handler.session_ended(Window(hwnd), ending);
        (*s).busy = false;
    } else if ending {
        // Writes again only if the work kept aside changed since the question, waiting for a
        // copy under way for as long as the end has left.
        crate::session_end::run_session_fallback(crate::session_end::end_budget());
    }
    if ending {
        // What the copy did not use of the 4 s, for the writes under way, saves included.
        crate::session_end::wait_for_writes(crate::session_end::end_budget());
        crate::session_end::finish_end();
    }
    crate::session_end::stop_blocking(hwnd);
}
