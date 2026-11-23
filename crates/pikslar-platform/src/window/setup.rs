//! Process and window-class setup: DPI awareness and the program icon.

use crate::ffi::*;

/// Win32 `ERROR_ACCESS_DENIED`.
const ERROR_ACCESS_DENIED: u32 = 5;
/// The program icon's group id in the exe's resources, written by `crates/pikslar/build.rs`.
const APP_ICON: usize = 1;

/// What the start-up call to set per-monitor (v2) DPI awareness achieved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DpiOutcome {
    /// The call set it.
    Set,
    /// It was already set before the call, by the manifest: Windows answers access denied.
    AlreadySet,
    /// The call failed with this error; the fallback should be tried.
    Failed(u32),
}

/// Maps the call's return value and `GetLastError` to its outcome.
pub(super) fn dpi_outcome(returned: i32, last_error: u32) -> DpiOutcome {
    match (returned, last_error) {
        (r, _) if r != 0 => DpiOutcome::Set,
        (_, ERROR_ACCESS_DENIED) => DpiOutcome::AlreadySet,
        (_, e) => DpiOutcome::Failed(e),
    }
}

/// Makes the process per-monitor DPI aware (v2). The manifest normally does this before any
/// code runs, which makes the call answer access denied: that is success. On a Windows too old
/// for v2 the call fails otherwise, and the process falls back to system awareness.
///
/// # Safety
/// Must be called before any window is created.
pub(super) unsafe fn set_dpi_awareness() {
    let returned = SetProcessDpiAwarenessContext(DPI_AWARE_V2);
    let error = if returned == 0 { GetLastError() } else { 0 };
    if let DpiOutcome::Failed(_) = dpi_outcome(returned, error) {
        SetProcessDPIAware();
    }
}

/// Registers the window class, with the program icon from the exe's resources at the large
/// and small system sizes. A build without the resources (tests) gets the default icon.
///
/// # Safety
/// `instance` must be this module's handle and `name` a null-terminated class name that
/// outlives the class.
pub(super) unsafe fn register_class(instance: H, proc_: WndProc, name: *const u16) -> bool {
    let icon = |w, h| {
        LoadImageW(
            instance,
            APP_ICON as *const u16,
            IMAGE_ICON,
            GetSystemMetrics(w),
            GetSystemMetrics(h),
            0,
        )
    };
    let wc = WNDCLASSEXW {
        size: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_OWNDC,
        proc_,
        cls: 0,
        wnd: 0,
        inst: instance,
        icon: icon(SM_CXICON, SM_CYICON),
        cursor: std::ptr::null_mut(),
        bg: std::ptr::null_mut(),
        menu: std::ptr::null(),
        name,
        icon_small: icon(SM_CXSMICON, SM_CYSMICON),
    };
    RegisterClassExW(&wc) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_call_has_set_the_awareness() {
        assert_eq!(dpi_outcome(1, 0), DpiOutcome::Set);
        assert_eq!(
            dpi_outcome(1, 87),
            DpiOutcome::Set,
            "a stale error is ignored"
        );
    }

    #[test]
    fn access_denied_means_the_manifest_already_set_it() {
        assert_eq!(dpi_outcome(0, ERROR_ACCESS_DENIED), DpiOutcome::AlreadySet);
    }

    #[test]
    fn any_other_failure_is_reported_for_the_fallback() {
        assert_eq!(dpi_outcome(0, 87), DpiOutcome::Failed(87));
        assert_eq!(dpi_outcome(0, 0), DpiOutcome::Failed(0));
    }

    #[test]
    fn a_module_without_the_icon_still_registers_a_class() {
        let name = wide("PikslarSetupTestClass");
        unsafe extern "system" fn proc_(h: H, m: u32, w: usize, l: isize) -> isize {
            DefWindowProcW(h, m, w, l)
        }
        // SAFETY: the test exe's own handle and a class name that lives through the call.
        let ok =
            unsafe { register_class(GetModuleHandleW(std::ptr::null()), proc_, name.as_ptr()) };
        assert!(ok);
    }
}
