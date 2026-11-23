//! Windows ending the session: the writes still under way, and the reason Windows shows
//! while it waits for them.
//!
//! Each write that keeps work safe (a backup copy, a save) holds a [`WritePending`] while it
//! held.as_mut().is_none_or(|run| run(wait))s, counted by its kind: the fallback that writes the copy when the handler is busy waits
//! only for other copies, which write the same files, never for a save, which may be slow. At `WM_QUERYENDSESSION` a write under way registers the reason "Saving a backup of
//! your work"; at `WM_ENDSESSION` the window waits for every such write, for at most
//! [`END_WAIT`], and takes the reason down. Windows gives a window about 5 s. While the
//! handler is busy higher up the stack the app's [`set_session_fallback`] runs instead.

use crate::ffi::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy, H};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// What Windows shows beside the program while it waits for it.
pub const BLOCK_REASON: &str = "Saving a backup of your work";
/// The longest `WM_ENDSESSION` waits for writes under way, inside the 5 s Windows allows.
pub const END_WAIT: Duration = Duration::from_secs(4);
/// The longest the fallback waits at `WM_QUERYENDSESSION` for a recovery copy already being
/// written: the question must be answered quickly. `WM_ENDSESSION` gives it all it has left.
pub const QUERY_FALLBACK_WAIT: Duration = Duration::from_secs(1);

static RECOVERY_WRITES: AtomicUsize = AtomicUsize::new(0);
static SAVE_WRITES: AtomicUsize = AtomicUsize::new(0);
static BLOCKING: AtomicBool = AtomicBool::new(false);

/// What a write that keeps work safe is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKind {
    /// A recovery copy.
    Recovery,
    /// A save of the project.
    Save,
}

impl WriteKind {
    fn counter(self) -> &'static AtomicUsize {
        match self {
            WriteKind::Recovery => &RECOVERY_WRITES,
            WriteKind::Save => &SAVE_WRITES,
        }
    }
}

/// Held while a write that keeps work safe is under way. Dropping it ends the write.
#[must_use = "the write counts as under way only while this is held"]
#[derive(Debug)]
pub struct WritePending(WriteKind);

/// Counts a write of `kind` as under way until what is returned is dropped.
pub fn write_pending(kind: WriteKind) -> WritePending {
    kind.counter().fetch_add(1, Ordering::SeqCst);
    WritePending(kind)
}

impl Drop for WritePending {
    fn drop(&mut self) {
        self.0.counter().fetch_sub(1, Ordering::SeqCst);
    }
}

/// How many writes of either kind are under way now.
pub fn writes_pending() -> usize {
    RECOVERY_WRITES.load(Ordering::SeqCst) + SAVE_WRITES.load(Ordering::SeqCst)
}

/// How many recovery copies are being written now.
pub fn recovery_writes_pending() -> usize {
    RECOVERY_WRITES.load(Ordering::SeqCst)
}

/// Waits up to `within` for every write under way to end. True once none is.
pub fn wait_for_writes(within: Duration) -> bool {
    wait_while(within, writes_pending)
}

/// Waits up to `within` for the recovery copies under way, and only those. True once none is.
pub fn wait_for_recovery_writes(within: Duration) -> bool {
    wait_while(within, recovery_writes_pending)
}

fn wait_while(within: Duration, pending: fn() -> usize) -> bool {
    let end = Instant::now() + within;
    while pending() > 0 {
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

/// Keeps work safe without the handler, waiting at most the time given for a recovery copy
/// already being written: true once it is safe.
pub type SessionFallback = Box<dyn FnMut(Duration) -> bool>;

thread_local! {
    static FALLBACK: RefCell<Option<SessionFallback>> = const { RefCell::new(None) };
}

/// Gives what keeps the work safe when Windows ends the session while the handler is busy
/// higher up the stack (a message box, a file dialog, a wait that answers the window), which
/// must not be touched then. For the window made on this thread.
pub fn set_session_fallback(fallback: SessionFallback) {
    FALLBACK.with(|f| *f.borrow_mut() = Some(fallback));
}

/// Runs the fallback, if one was given. True if the work is safe, or there is none to run.
pub fn run_session_fallback(wait: Duration) -> bool {
    FALLBACK.with(|f| match f.try_borrow_mut() {
        Ok(mut held) => held.as_mut().is_none_or(|run| run(wait)),
        Err(_) => false,
    })
}

thread_local! {
    static END_BY: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Starts the clock of the [`END_WAIT`] at `WM_ENDSESSION`: everything the end does shares it.
pub(crate) fn begin_end() {
    END_BY.with(|b| b.set(Some(Instant::now() + END_WAIT)));
}

/// Ends the clock of the end.
pub(crate) fn finish_end() {
    END_BY.with(|b| b.set(None));
}

/// What is left of the [`END_WAIT`] since `WM_ENDSESSION` came, all of it before.
pub fn end_budget() -> Duration {
    END_BY
        .with(|b| b.get())
        .map_or(END_WAIT, |by| by.saturating_duration_since(Instant::now()))
}

/// Whether the reason is registered now.
pub fn blocking_shutdown() -> bool {
    BLOCKING.load(Ordering::SeqCst)
}

/// Registers the reason while a write is under way, and only then.
///
/// # Safety
/// Must be called on the thread that made `hwnd`, which must be a live window.
pub(crate) unsafe fn block_while_writing(hwnd: H) {
    if writes_pending() == 0 || BLOCKING.load(Ordering::SeqCst) {
        return;
    }
    let reason: Vec<u16> = BLOCK_REASON.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `reason` is NUL-terminated and outlives the call; Windows copies it.
    if ShutdownBlockReasonCreate(hwnd, reason.as_ptr()) != 0 {
        BLOCKING.store(true, Ordering::SeqCst);
    }
}

/// Takes the reason down, if it is up.
///
/// # Safety
/// As [`block_while_writing`].
pub(crate) unsafe fn stop_blocking(hwnd: H) {
    if BLOCKING.swap(false, Ordering::SeqCst) {
        ShutdownBlockReasonDestroy(hwnd);
    }
}

#[cfg(test)]
#[path = "session_end_tests.rs"]
mod tests;
