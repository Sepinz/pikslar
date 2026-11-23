//! Where a window sits relative to the monitors: plain geometry, with the Win32 calls kept thin.

use crate::ffi::*;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

/// `rect` moved, and shrunk if it must be, to lie inside `work`.
pub(super) fn clamp_into(rect: RECT, work: RECT) -> RECT {
    let w = (rect.right - rect.left).min(work.right - work.left);
    let h = (rect.bottom - rect.top).min(work.bottom - work.top);
    let left = rect.left.clamp(work.left, work.right - w);
    let top = rect.top.clamp(work.top, work.bottom - h);
    RECT {
        left,
        top,
        right: left + w,
        bottom: top + h,
    }
}

/// The opening size: `base` units at `scale`, at most nine tenths of the work area.
pub(super) fn first_size(base: (i32, i32), scale: i32, work: RECT) -> (i32, i32) {
    (
        (base.0 * scale).min((work.right - work.left) * 9 / 10),
        (base.1 * scale).min((work.bottom - work.top) * 9 / 10),
    )
}

/// The work area (screen minus taskbar) of a monitor, if Windows will say.
unsafe fn work_area(monitor: H) -> Option<RECT> {
    if monitor.is_null() {
        return None;
    }
    let mut info: MONITORINFO = zeroed();
    info.size = size_of::<MONITORINFO>() as u32;
    (GetMonitorInfoW(monitor, &mut info) != 0).then_some(info.work)
}

/// The work area of the monitor a default-placed window opens on: the primary one.
///
/// # Safety
/// Must be called on a thread of a process with DPI awareness already set.
pub(super) unsafe fn primary_work_area() -> Option<RECT> {
    work_area(MonitorFromPoint(
        POINT { x: 0, y: 0 },
        MONITOR_DEFAULTTOPRIMARY,
    ))
}

/// If the window no longer meets any monitor, as after one was unplugged, moves it onto the
/// nearest one's work area.
///
/// # Safety
/// `hwnd` must be a live window of this thread.
pub(super) unsafe fn rescue_offscreen(hwnd: H) {
    if !MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL).is_null() || IsIconic(hwnd) != 0 {
        return;
    }
    let Some(work) = work_area(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)) else {
        return;
    };
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect) == 0 {
        return;
    }
    let r = clamp_into(rect, work);
    SetWindowPos(
        hwnd,
        null_mut(),
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn r(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn same(a: RECT, b: RECT) -> bool {
        (a.left, a.top, a.right, a.bottom) == (b.left, b.top, b.right, b.bottom)
    }

    #[test]
    fn a_window_far_right_of_a_removed_monitor_comes_back_inside() {
        let work = r(0, 0, 1920, 1040);
        let out = clamp_into(r(2500, 100, 3300, 700), work);
        assert!(same(out, r(1120, 100, 1920, 700)));
    }

    #[test]
    fn a_window_above_or_left_of_the_work_area_moves_down_and_right() {
        let work = r(0, 40, 1920, 1080);
        assert!(same(
            clamp_into(r(-900, -500, -100, 100), work),
            r(0, 40, 800, 640)
        ));
    }

    #[test]
    fn a_window_already_inside_is_unchanged() {
        let work = r(0, 0, 1920, 1040);
        let win = r(100, 100, 900, 700);
        assert!(same(clamp_into(win, work), win));
    }

    #[test]
    fn a_window_larger_than_the_work_area_is_shrunk_to_it() {
        let work = r(100, 0, 1380, 700);
        assert!(same(clamp_into(r(2000, 900, 4000, 2900), work), work));
    }

    #[test]
    fn the_first_size_is_scaled_and_capped_by_the_work_area() {
        let work = r(0, 0, 1366, 728);
        assert_eq!(first_size((1000, 800), 1, work), (1000, 655));
        assert_eq!(first_size((1000, 800), 2, work), (1229, 655));
        assert_eq!(first_size((400, 300), 1, work), (400, 300));
    }
}
