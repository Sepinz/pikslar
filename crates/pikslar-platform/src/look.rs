//! What Windows says about how the interface should look: whether high contrast is on and its
//! colors, and the text size of Settings, Accessibility, Text size.
//!
//! The text size is a WinRT setting (`UISettings.TextScaleFactor`). Without WinRT it is read
//! where Settings writes it, the per-user registry value
//! `HKCU\Software\Microsoft\Accessibility\TextScaleFactor`, a DWORD from 100 to 225. When it
//! changes Windows sends `WM_SETTINGCHANGE`, and the window reads it again.

use crate::ffi::*;
use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::OnceLock;

/// The system colors high contrast draws with, as 0xffRRGGBB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemColors {
    pub window: u32,
    pub window_text: u32,
    pub button_face: u32,
    pub button_text: u32,
    pub highlight: u32,
    pub highlight_text: u32,
    pub gray_text: u32,
    pub hot_light: u32,
}

/// A COLORREF, 0x00BBGGRR, as an opaque 0xffRRGGBB.
pub(crate) fn from_colorref(c: u32) -> u32 {
    0xff00_0000 | (c & 0xff) << 16 | (c & 0xff00) | (c >> 16 & 0xff)
}

fn sys(index: i32) -> u32 {
    // SAFETY: GetSysColor takes any index and returns a plain value.
    from_colorref(unsafe { GetSysColor(index) })
}

/// The system colors, when high contrast is on.
pub fn high_contrast() -> Option<SystemColors> {
    let mut hc = HIGHCONTRASTW {
        size: size_of::<HIGHCONTRASTW>() as u32,
        flags: 0,
        scheme: null_mut(),
    };
    // SAFETY: `hc` is a valid HIGHCONTRASTW with its size set, as the call requires.
    let ok =
        unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.size, &mut hc as *mut _ as _, 0) };
    (ok != 0 && hc.flags & HCF_HIGHCONTRASTON != 0).then(|| SystemColors {
        window: sys(COLOR_WINDOW),
        window_text: sys(COLOR_WINDOWTEXT),
        button_face: sys(COLOR_BTNFACE),
        button_text: sys(COLOR_BTNTEXT),
        highlight: sys(COLOR_HIGHLIGHT),
        highlight_text: sys(COLOR_HIGHLIGHTTEXT),
        gray_text: sys(COLOR_GRAYTEXT),
        hot_light: sys(COLOR_HOTLIGHT),
    })
}

/// The text size in percent, 100 when it is not set or cannot be read.
pub fn text_percent() -> i32 {
    let key = wide(r"Software\Microsoft\Accessibility");
    let name = wide("TextScaleFactor");
    let mut value = 0u32;
    let mut bytes = size_of::<u32>() as u32;
    // SAFETY: the strings are NUL-terminated and live for the call; `value` and `bytes` are
    // valid out pointers, and `bytes` holds the size of `value`.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut value as *mut u32 as _,
            &mut bytes,
        )
    };
    crate::scale::text_percent_from((status == 0).then_some(value))
}

/// Asks Windows to draw `hwnd`'s title bar in its own dark style, or its light one. Windows
/// draws the bar; nothing of it is drawn here. Older Windows without the setting keeps its
/// light bar, and in high contrast Windows draws the bar in the system's colors whatever is
/// asked. Returns whether Windows took the request.
pub(crate) fn dark_title(hwnd: H, dark: bool) -> bool {
    if hwnd.is_null() {
        return false;
    }
    let value: i32 = dark.into();
    let size = size_of::<i32>() as u32;
    [
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        DWMWA_USE_IMMERSIVE_DARK_MODE_OLD,
    ]
    .into_iter()
    .any(|attribute| {
        let Some(set) = set_window_attribute() else {
            return false;
        };
        // SAFETY: `hwnd` is a window of this process, `value` lives for the call, and
        // `size` is its size, as DwmSetWindowAttribute requires; it changes nothing else.
        let result = unsafe { set(hwnd, attribute, &value as *const i32 as _, size) };
        result >= 0
    })
}

/// `DwmSetWindowAttribute` from System32's `dwmapi.dll`, looked up once; `None` if Windows
/// has none (the title bar then keeps its light style).
fn set_window_attribute() -> Option<DwmSetWindowAttributeFn> {
    static FOUND: OnceLock<usize> = OnceLock::new();
    let address = *FOUND.get_or_init(|| {
        let name = wide("dwmapi.dll");
        // SAFETY: a NUL-terminated name; the module stays loaded for the process's life, so
        // the address below stays valid.
        let module =
            unsafe { LoadLibraryExW(name.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if module.is_null() {
            return 0;
        }
        // SAFETY: a loaded module and a NUL-terminated export name.
        unsafe { GetProcAddress(module, c"DwmSetWindowAttribute".as_ptr().cast()) as usize }
    });
    // SAFETY: a non-zero address is DwmSetWindowAttribute's, whose signature the type states.
    (address != 0)
        .then(|| unsafe { std::mem::transmute::<usize, DwmSetWindowAttributeFn>(address) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colorref_turns_into_rgb() {
        assert_eq!(from_colorref(0x00ff_0000), 0xff00_00ff, "blue");
        assert_eq!(from_colorref(0x0000_00ff), 0xffff_0000, "red");
        assert_eq!(from_colorref(0x0012_3456), 0xff56_3412);
    }

    #[test]
    fn the_title_bar_call_is_found_in_the_system_dwmapi() {
        assert!(set_window_attribute().is_some());
        assert_eq!(
            set_window_attribute().map(|f| f as usize),
            set_window_attribute().map(|f| f as usize),
            "looked up once"
        );
    }

    #[test]
    fn a_detached_window_takes_no_title_request() {
        assert!(!dark_title(null_mut(), true));
    }

    #[test]
    fn reading_the_system_never_fails() {
        let percent = text_percent();
        assert!((100..=225).contains(&percent));
        let _ = high_contrast();
    }
}
