//! The small part of the Win32 API we use, declared by hand to avoid a large dependency.
#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::ffi::c_void;

pub type H = *mut c_void;
pub type WndProc = unsafe extern "system" fn(H, u32, usize, isize) -> isize;

pub const CS_OWNDC: u32 = 0x20;
pub const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_EX_TOPMOST: u32 = 8;
pub const CW_USEDEFAULT: i32 = i32::MIN;
pub const GWLP_USERDATA: i32 = -21;
pub const SRCCOPY: u32 = 0x00CC_0020;
pub const PM_REMOVE: u32 = 1;
pub const QS_ALLINPUT: u32 = 0x04FF;
pub const MWMO_INPUTAVAILABLE: u32 = 4;
pub const INFINITE: u32 = u32::MAX;
pub const DIB_RGB_COLORS: u32 = 0;
pub const TRANSPARENT: i32 = 1;
pub const TA_BASELINE: u32 = 24;
pub const NONANTIALIASED_QUALITY: u32 = 3;

#[repr(C)]
#[derive(Default)]
pub struct SIZE {
    pub cx: i32,
    pub cy: i32,
}
pub const HTCLIENT: isize = 1;
pub const SWP_NOZORDER: u32 = 4;
pub const SWP_NOACTIVATE: u32 = 0x10;
pub const DPI_AWARE_V2: H = -4isize as H;

pub const IDC_ARROW: usize = 32512;
pub const IDC_CROSS: usize = 32515;
pub const IDC_SIZEALL: usize = 32646;
pub const IDC_SIZENWSE: usize = 32642;
pub const IDC_SIZENESW: usize = 32643;
pub const IDC_SIZEWE: usize = 32644;
pub const IDC_SIZENS: usize = 32645;
pub const IDC_APPSTARTING: usize = 32650;
pub const SM_CXSCREEN: i32 = 0;
pub const SM_CYSCREEN: i32 = 1;
pub const SM_CXICON: i32 = 11;
pub const SM_CYICON: i32 = 12;
pub const SM_CXSMICON: i32 = 49;
pub const SM_CYSMICON: i32 = 50;
pub const IMAGE_ICON: u32 = 1;
pub const SIZE_MINIMIZED: usize = 1;
pub const WM_GETMINMAXINFO: u32 = 0x0024;
pub const WM_QUERYENDSESSION: u32 = 0x0011;
pub const WM_ENDSESSION: u32 = 0x0016;
pub const CF_DIB: u32 = 8;
pub const CF_DIBV5: u32 = 17;
pub const CF_UNICODETEXT: u32 = 13;
pub const GMEM_MOVEABLE: u32 = 2;

pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_KILLFOCUS: u32 = 0x0008;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_QUIT: u32 = 0x0012;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_SETCURSOR: u32 = 0x0020;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_CHAR: u32 = 0x0102;
pub const MAPVK_VK_TO_CHAR: u32 = 2;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_SYSKEYUP: u32 = 0x0105;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONDOWN: u32 = 0x0204;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MBUTTONDOWN: u32 = 0x0207;
pub const WM_MBUTTONUP: u32 = 0x0208;
pub const WM_MOUSEWHEEL: u32 = 0x020A;
pub const WM_CAPTURECHANGED: u32 = 0x0215;
pub const WM_MOUSELEAVE: u32 = 0x02A3;
pub const TME_LEAVE: u32 = 2;
pub const WM_DPICHANGED: u32 = 0x02E0;
pub const WM_DISPLAYCHANGE: u32 = 0x007E;
pub const MONITOR_DEFAULTTONULL: u32 = 0;
pub const MONITOR_DEFAULTTOPRIMARY: u32 = 1;
pub const MONITOR_DEFAULTTONEAREST: u32 = 2;
pub const WM_SYSCOMMAND: u32 = 0x0112;
/// Asks the window to work out its interface scale again.
pub const WM_RESCALE: u32 = 0x8001;
/// Posted from another thread: something finished, so a frame should run.
pub const WM_WAKE: u32 = 0x8002;
pub const WM_DROPFILES: u32 = 0x0233;
pub const SC_KEYMENU: usize = 0xF100;

pub const VK_SHIFT: i32 = 0x10;
pub const VK_CONTROL: i32 = 0x11;
pub const VK_MENU: i32 = 0x12;

pub const OFN_PATHMUSTEXIST: u32 = 0x800;
pub const OFN_FILEMUSTEXIST: u32 = 0x1000;
pub const OFN_OVERWRITEPROMPT: u32 = 2;
pub const OFN_NOCHANGEDIR: u32 = 8;
pub const OFN_NOREADONLYRETURN: u32 = 0x8000;
pub const OFN_NODEREFERENCELINKS: u32 = 0x0010_0000;

pub const MB_OK: u32 = 0;
pub const MB_YESNOCANCEL: u32 = 3;
pub const MB_YESNO: u32 = 4;
/// The third button is the one Enter presses.
pub const MB_DEFBUTTON3: u32 = 0x200;
pub const MB_ICONERROR: u32 = 0x10;
pub const MB_ICONWARNING: u32 = 0x30;
pub const IDYES: i32 = 6;
pub const IDNO: i32 = 7;

#[repr(C)]
pub struct WNDCLASSEXW {
    pub size: u32,
    pub style: u32,
    pub proc_: WndProc,
    pub cls: i32,
    pub wnd: i32,
    pub inst: H,
    pub icon: H,
    pub cursor: H,
    pub bg: H,
    pub menu: *const u16,
    pub name: *const u16,
    pub icon_small: H,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: H,
    pub message: u32,
    pub w: usize,
    pub l: isize,
    pub time: u32,
    pub pt: [i32; 2],
    pub private: u32,
}

#[repr(C)]
pub struct TRACKMOUSEEVENT {
    pub size: u32,
    pub flags: u32,
    pub hwnd: H,
    pub hover_time: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct MONITORINFO {
    pub size: u32,
    pub monitor: RECT,
    pub work: RECT,
    pub flags: u32,
}

#[repr(C)]
pub struct PAINTSTRUCT {
    pub hdc: H,
    pub erase: i32,
    pub paint: RECT,
    pub restore: i32,
    pub inc: i32,
    pub reserved: [u8; 32],
}

#[repr(C)]
pub struct BITMAPINFOHEADER {
    pub size: u32,
    pub w: i32,
    pub h: i32,
    pub planes: u16,
    pub bits: u16,
    pub comp: u32,
    pub img: u32,
    pub xp: i32,
    pub yp: i32,
    pub used: u32,
    pub imp: u32,
}

#[repr(C)]
pub struct OPENFILENAMEW {
    pub size: u32,
    pub owner: H,
    pub instance: H,
    pub filter: *const u16,
    pub custom_filter: *mut u16,
    pub max_custom: u32,
    pub filter_index: u32,
    pub file: *mut u16,
    pub max_file: u32,
    pub file_title: *mut u16,
    pub max_file_title: u32,
    pub initial_dir: *const u16,
    pub title: *const u16,
    pub flags: u32,
    pub file_offset: u16,
    pub extension_offset: u16,
    pub default_ext: *const u16,
    pub cust_data: isize,
    pub hook: *const c_void,
    pub template: *const u16,
    pub reserved_ptr: *mut c_void,
    pub reserved: u32,
    pub flags_ex: u32,
}

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassExW(c: *const WNDCLASSEXW) -> u16;
    pub fn LoadImageW(i: H, name: *const u16, kind: u32, cx: i32, cy: i32, flags: u32) -> H;
    pub fn SetProcessDPIAware() -> i32;
    #[allow(clippy::too_many_arguments)]
    pub fn CreateWindowExW(
        ex: u32,
        cls: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        p: H,
        m: H,
        i: H,
        l: H,
    ) -> H;
    pub fn DefWindowProcW(h: H, m: u32, w: usize, l: isize) -> isize;
    pub fn PeekMessageW(m: *mut MSG, h: H, a: u32, b: u32, r: u32) -> i32;
    pub fn TranslateMessage(m: *const MSG) -> i32;
    pub fn DispatchMessageW(m: *const MSG) -> isize;
    pub fn RegisterWindowMessageW(name: *const u16) -> u32;
    pub fn MsgWaitForMultipleObjectsEx(n: u32, h: *const H, ms: u32, mask: u32, flags: u32) -> u32;
    pub fn GetDC(h: H) -> H;
    pub fn DestroyWindow(h: H) -> i32;
    pub fn PostQuitMessage(code: i32);
    pub fn SetWindowLongPtrW(h: H, i: i32, v: isize) -> isize;
    pub fn GetWindowLongPtrW(h: H, i: i32) -> isize;
    pub fn GetClientRect(h: H, r: *mut RECT) -> i32;
    pub fn BeginPaint(h: H, p: *mut PAINTSTRUCT) -> H;
    pub fn EndPaint(h: H, p: *const PAINTSTRUCT) -> i32;
    pub fn SetCapture(h: H) -> H;
    pub fn TrackMouseEvent(t: *mut TRACKMOUSEEVENT) -> i32;
    pub fn ReleaseCapture() -> i32;
    pub fn GetKeyState(k: i32) -> i16;
    pub fn MapVirtualKeyW(code: u32, map: u32) -> u32;
    pub fn ScreenToClient(h: H, p: *mut [i32; 2]) -> i32;
    pub fn LoadCursorW(i: H, name: *const u16) -> H;
    pub fn SetCursor(c: H) -> H;
    pub fn SetWindowTextW(h: H, s: *const u16) -> i32;
    pub fn PostMessageW(h: H, msg: u32, w: usize, l: isize) -> i32;
    pub fn SetProcessDpiAwarenessContext(c: H) -> i32;
    pub fn GetDpiForWindow(h: H) -> u32;
    pub fn GetDpiForSystem() -> u32;
    pub fn GetSystemMetrics(i: i32) -> i32;
    pub fn GetWindowRect(h: H, r: *mut RECT) -> i32;
    pub fn IsIconic(h: H) -> i32;
    pub fn IsZoomed(h: H) -> i32;
    pub fn MonitorFromWindow(h: H, flags: u32) -> H;
    pub fn MonitorFromPoint(p: POINT, flags: u32) -> H;
    pub fn GetMonitorInfoW(m: H, info: *mut MONITORINFO) -> i32;
    pub fn OpenClipboard(h: H) -> i32;
    pub fn CloseClipboard() -> i32;
    pub fn EmptyClipboard() -> i32;
    pub fn SetClipboardData(format: u32, data: H) -> H;
    pub fn GetClipboardData(format: u32) -> H;
    pub fn IsClipboardFormatAvailable(format: u32) -> i32;
    pub fn GetClipboardSequenceNumber() -> u32;
    #[allow(clippy::too_many_arguments)]
    pub fn SetWindowPos(h: H, after: H, x: i32, y: i32, w: i32, h2: i32, flags: u32) -> i32;
    pub fn MessageBoxW(h: H, text: *const u16, caption: *const u16, t: u32) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateCompatibleDC(dc: H) -> H;
    pub fn CreateDIBSection(
        dc: H,
        bi: *const BITMAPINFOHEADER,
        usage: u32,
        bits: *mut *mut u32,
        section: H,
        offset: u32,
    ) -> H;
    pub fn SelectObject(dc: H, o: H) -> H;
    pub fn DeleteObject(o: H) -> i32;
    #[allow(clippy::too_many_arguments)]
    pub fn BitBlt(
        dst: H,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        src: H,
        sx: i32,
        sy: i32,
        rop: u32,
    ) -> i32;
    pub fn GdiFlush() -> i32;
    pub fn DeleteDC(dc: H) -> i32;
    #[allow(clippy::too_many_arguments)]
    pub fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike: u32,
        charset: u32,
        precision: u32,
        clip: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> H;
    pub fn SetTextColor(dc: H, color: u32) -> u32;
    pub fn SetBkMode(dc: H, mode: i32) -> i32;
    pub fn SetTextAlign(dc: H, align: u32) -> u32;
    pub fn TextOutW(dc: H, x: i32, y: i32, text: *const u16, n: i32) -> i32;
    pub fn GetTextExtentPoint32W(dc: H, text: *const u16, n: i32, size: *mut SIZE) -> i32;
}

#[link(name = "comdlg32")]
extern "system" {
    pub fn GetOpenFileNameW(o: *mut OPENFILENAMEW) -> i32;
    pub fn GetSaveFileNameW(o: *mut OPENFILENAMEW) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    pub fn DragAcceptFiles(h: H, accept: i32);
    pub fn DragQueryFileW(drop: H, index: u32, file: *mut u16, units: u32) -> u32;
    pub fn DragFinish(drop: H);
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(n: *const u16) -> H;
    pub fn GetLastError() -> u32;
    pub fn GlobalAlloc(flags: u32, bytes: usize) -> H;
    pub fn GlobalFree(h: H) -> H;
    pub fn GlobalLock(h: H) -> *mut c_void;
    pub fn GlobalUnlock(h: H) -> i32;
    pub fn GlobalSize(h: H) -> usize;
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub const WM_SETTINGCHANGE: u32 = 0x001A;
pub const WM_SYSCOLORCHANGE: u32 = 0x0015;
pub const WM_THEMECHANGED: u32 = 0x031A;
pub const SPI_GETHIGHCONTRAST: u32 = 0x0042;
pub const HCF_HIGHCONTRASTON: u32 = 1;
pub const COLOR_WINDOW: i32 = 5;
pub const COLOR_WINDOWTEXT: i32 = 8;
pub const COLOR_HIGHLIGHT: i32 = 13;
pub const COLOR_HIGHLIGHTTEXT: i32 = 14;
pub const COLOR_BTNFACE: i32 = 15;
pub const COLOR_GRAYTEXT: i32 = 17;
pub const COLOR_BTNTEXT: i32 = 18;
pub const COLOR_HOTLIGHT: i32 = 26;
pub const HKEY_CURRENT_USER: H = 0x8000_0001_usize as H;
pub const RRF_RT_REG_DWORD: u32 = 0x18;

#[repr(C)]
pub struct HIGHCONTRASTW {
    pub size: u32,
    pub flags: u32,
    pub scheme: *mut u16,
}

#[link(name = "user32")]
extern "system" {
    pub fn SystemParametersInfoW(action: u32, param: u32, value: *mut c_void, ini: u32) -> i32;
    pub fn GetSysColor(index: i32) -> u32;
    pub fn ShutdownBlockReasonCreate(hwnd: H, reason: *const u16) -> i32;
    pub fn ShutdownBlockReasonDestroy(hwnd: H) -> i32;
}

#[link(name = "advapi32")]
extern "system" {
    #[allow(clippy::too_many_arguments)]
    pub fn RegGetValueW(
        key: H,
        sub: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut c_void,
        bytes: *mut u32,
    ) -> i32;
}

/// Asks for Windows' own dark title bar (Windows 10 20H1 and later, and Windows 11).
pub const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
/// The same attribute's number on Windows 10 1809 to 1909.
pub const DWMWA_USE_IMMERSIVE_DARK_MODE_OLD: u32 = 19;

/// `DwmSetWindowAttribute`, found at run time in the system's `dwmapi.dll` (`look.rs`), so the
/// program needs no `libdwmapi.a` import library: rustup's own MinGW, the pinned release
/// linker (`docs/b1-packaging.md`), ships none.
pub type DwmSetWindowAttributeFn =
    unsafe extern "system" fn(hwnd: H, attribute: u32, value: *const c_void, size: u32) -> i32;

/// `LOAD_LIBRARY_SEARCH_SYSTEM32`: a DLL is looked for in System32 only, never beside the exe.
pub const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x800;

#[link(name = "kernel32")]
extern "system" {
    pub fn LoadLibraryExW(name: *const u16, file: H, flags: u32) -> H;
    pub fn GetProcAddress(module: H, name: *const u8) -> *mut c_void;
}
