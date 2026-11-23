//! The window, the event loop and presenting a DIB section.
//!
//! The loop drains every pending message, handles them all, then presents once. It never
//! waits for vsync: Phase 0 measured that as the main source of lag under fast input.
//! Drawing and the blit happen in `process`, after the queue is drained, not on `WM_PAINT`
//! (which only copies the kept bitmap again). A message another thread sends is answered
//! inside whichever `PeekMessageW` of the drain finds it, before that frame is drawn, so a
//! sent message tells another process nothing about when a frame was shown: `frame_log` does.

mod input;
mod placement;
mod sentinel;
mod session;
mod setup;

use crate::ffi::*;
use crate::frame_log::{self, FrameLog, FrameTimes};
use crate::{keymap, Event, Handler, Key, Mods};
use pikslar_render::{Rect, Surface};
use std::mem::{size_of, take, zeroed};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use input::typed;
pub(crate) use input::{button_event, key_of, modifiers, point};

static CURSOR: AtomicUsize = AtomicUsize::new(IDC_ARROW);

/// In interface units, before display scaling.
const DEFAULT_WINDOW: (i32, i32) = (1400, 900);
const MIN_WINDOW: (i32, i32) = (640, 600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Arrow,
    Cross,
    Move,
    /// Resize arrows for a selection handle.
    SizeNwse,
    SizeNesw,
    SizeWe,
    SizeNs,
    /// The arrow with a busy sign: work goes on and the window still answers.
    Busy,
}

/// Wakes the window's loop from another thread, so a frame runs and can collect a result.
/// It holds only the window's handle as a number; waking a window that has closed does nothing.
#[derive(Clone, Copy, Debug)]
pub struct Waker(isize);

impl Waker {
    pub fn wake(self) {
        if self.0 != 0 {
            // SAFETY: takes no pointers. PostMessageW may be called from any thread, and a
            // handle that no longer names a window only makes it fail.
            unsafe { PostMessageW(self.0 as H, WM_WAKE, 0, 0) };
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct WindowOptions {
    /// Outer x, y, width and height.
    pub placement: Option<(i32, i32, i32, i32)>,
    pub topmost: bool,
    /// Pretends the display reports this DPI, for checking layouts on a single machine.
    pub dpi: Option<i32>,
    /// Forces the interface scale, for checking layouts without changing display settings.
    pub scale: Option<i32>,
    /// Appends input-to-present times here, for measurement runs (`frame_log`).
    pub frame_log: Option<PathBuf>,
    /// Pretends Windows' text size is this percent, for checking it without changing settings.
    pub text_scale: Option<i32>,
}

/// A copyable handle for use from inside the event handlers.
#[derive(Clone, Copy, Debug)]
pub struct Window(H);

impl Window {
    /// A handle with no window behind it: dialogs answer as if dismissed. For tests.
    pub fn detached() -> Window {
        Window(null_mut())
    }

    pub(crate) fn raw(self) -> H {
        self.0
    }

    pub(crate) fn is_detached(self) -> bool {
        self.0.is_null()
    }

    /// A waker for this window, for a thread that finishes work the window waits for.
    pub fn waker(self) -> Waker {
        Waker(self.0 as isize)
    }

    pub fn set_title(self, title: &str) {
        let text = wide(title);
        // SAFETY: the handle is valid during callbacks and the string is NUL-terminated.
        unsafe { SetWindowTextW(self.0, text.as_ptr()) };
    }

    /// Windows' own dark title bar, or its light one: see [`crate::look::dark_title`].
    pub fn set_dark_title(self, dark: bool) -> bool {
        crate::look::dark_title(self.0, dark)
    }

    pub fn set_cursor(self, cursor: Cursor) {
        let id = match cursor {
            Cursor::Arrow => IDC_ARROW,
            Cursor::Cross => IDC_CROSS,
            Cursor::Move => IDC_SIZEALL,
            Cursor::SizeNwse => IDC_SIZENWSE,
            Cursor::SizeNesw => IDC_SIZENESW,
            Cursor::SizeWe => IDC_SIZEWE,
            Cursor::SizeNs => IDC_SIZENS,
            Cursor::Busy => IDC_APPSTARTING,
        };
        if CURSOR.swap(id, Ordering::Relaxed) != id {
            apply_cursor(id);
        }
    }

    /// Uses a chosen interface size, or the automatic one for `None`, from the next frame.
    /// A size the window has no room for gives way to the largest that fits.
    pub fn set_interface_size(self, size: Option<i32>) {
        crate::scale::set_chosen(size);
        if !self.is_detached() {
            // SAFETY: takes no pointers; posts to this window, which handles it on its thread.
            unsafe { PostMessageW(self.0, WM_RESCALE, 0, 0) };
        }
    }

    pub fn quit(self) {
        // SAFETY: takes no pointers; posts to this thread's own queue.
        unsafe { PostQuitMessage(0) };
    }
}

fn apply_cursor(id: usize) {
    // SAFETY: `id` is a predefined system cursor, passed as an integer resource as the API
    // specifies, and the returned shared handle needs no cleanup.
    unsafe { SetCursor(LoadCursorW(null_mut(), id as *const u16)) };
}

struct State<H2> {
    handler: H2,
    hwnd: H,
    window_dc: H,
    memory_dc: H,
    bitmap: H,
    bits: *mut u32,
    w: i32,
    h: i32,
    scale: i32,
    forced_scale: Option<i32>,
    forced_dpi: Option<i32>,
    /// The text size a test fixed, which a settings change does not replace.
    forced_text: Option<i32>,
    events: Vec<Event>,
    /// Windows has been asked to report the pointer leaving, and has not yet.
    leave_tracked: bool,
    dirty: Vec<Rect>,
    /// True while handler code runs, including inside modal dialogs that pump messages.
    busy: bool,
    pending_resize: bool,
    /// Another thread asked for a frame, perhaps while a modal dialog held the loop.
    woken: bool,
    /// Wheel movement smaller than one notch, from high-resolution wheels and touchpads.
    wheel_rest: i32,
    /// Half of a character typed as two UTF-16 units.
    utf16: keymap::Utf16,
    /// The last key-down gave a shortcut, so the character it types must not give another.
    keydown_fired: bool,
    /// Whether Alt has been pressed on its own since it went down.
    alt: crate::alt_tap::AltTap,
    /// Only for measurement runs; `None` otherwise, and then `input_at` stays `None` too.
    frame_log: Option<FrameLog>,
    /// When the first input message since the last frame was taken.
    input_at: Option<Instant>,
}

fn rounded_scale(dpi: i32) -> i32 {
    ((dpi + 48) / 96).max(1)
}

/// The interface scale for a display and window size, in physical pixels: what the display
/// asks for, or the largest whole scale below it that leaves the interface the room it needs,
/// [`crate::SMALLEST_UNITS`]. So a 1920 by 1080 display at 150 percent gets 2x, as it asks.
pub fn interface_scale(dpi: i32, w: i32, h: i32) -> i32 {
    (1..=rounded_scale(dpi))
        .rev()
        .find(|&s| crate::scale::size_fits(s, w, h))
        .unwrap_or(1)
}

fn scale_for(hwnd: H, forced_dpi: Option<i32>) -> i32 {
    // SAFETY: `hwnd` is a live window owned by this thread.
    let dpi = forced_dpi.unwrap_or_else(|| unsafe { GetDpiForWindow(hwnd) } as i32);
    let (w, h) = client_size(hwnd);
    let text = crate::scale::text_percent();
    crate::scale::chosen_scale_with_text(crate::scale::chosen(), dpi, text, w, h)
}

fn client_size(hwnd: H) -> (i32, i32) {
    let mut r = RECT::default();
    // SAFETY: `hwnd` is live and `r` is a valid out pointer.
    unsafe { GetClientRect(hwnd, &mut r) };
    ((r.right - r.left).max(1), (r.bottom - r.top).max(1))
}

/// # Safety
/// `s` must be valid and no `Surface` over the current bitmap may exist.
unsafe fn recreate_bitmap<H2>(s: *mut State<H2>) {
    let (w, h) = client_size((*s).hwnd);
    let info = BITMAPINFOHEADER {
        size: size_of::<BITMAPINFOHEADER>() as u32,
        w,
        h: -h,
        planes: 1,
        bits: 32,
        comp: 0,
        img: 0,
        xp: 0,
        yp: 0,
        used: 0,
        imp: 0,
    };
    let mut bits = null_mut();
    let bitmap = CreateDIBSection(
        (*s).memory_dc,
        &info,
        DIB_RGB_COLORS,
        &mut bits,
        null_mut(),
        0,
    );
    if bitmap.is_null() || bits.is_null() {
        return;
    }
    SelectObject((*s).memory_dc, bitmap);
    if !(*s).bitmap.is_null() {
        DeleteObject((*s).bitmap);
    }
    (*s).bitmap = bitmap;
    (*s).bits = bits;
    (*s).w = w;
    (*s).h = h;
    (*s).scale = (*s)
        .forced_scale
        .unwrap_or_else(|| scale_for((*s).hwnd, (*s).forced_dpi));
    push(
        s,
        Event::Resized {
            w,
            h,
            scale: (*s).scale,
        },
    );
}

/// Queues an event, unless the queue rules drop it (see `admit`).
///
/// # Safety
/// `s` must point to the live state.
unsafe fn push<H2>(s: *mut State<H2>, event: Event) {
    crate::admit::admit(&mut (*s).events, event, (*s).busy);
}

/// Delivers queued events, draws, and copies changed areas to the window.
///
/// # Safety
/// `s` must point to the live state, and must only be called from the window's thread.
unsafe fn process<H2: Handler>(s: *mut State<H2>) {
    if (*s).busy {
        return;
    }
    (*s).busy = true;
    if take(&mut (*s).pending_resize) {
        recreate_bitmap(s);
    }
    // Some only when a frame log is kept: the clock is read only then.
    let input = take(&mut (*s).input_at);
    let window = Window((*s).hwnd);
    let mut events = take(&mut (*s).events);
    for event in events.drain(..) {
        (*s).handler.event(event, window);
    }
    if (*s).events.is_empty() {
        (*s).events = events;
    }
    let handled = input.map(|_| Instant::now());
    if !(*s).bits.is_null() {
        let count = (*s).w as usize * (*s).h as usize;
        let px = std::slice::from_raw_parts_mut((*s).bits, count);
        let mut surface = Surface {
            px,
            w: (*s).w,
            h: (*s).h,
        };
        let mut dirty = take(&mut (*s).dirty);
        dirty.clear();
        (*s).handler.frame(&mut surface, window, &mut dirty);
        let framed = input.map(|_| Instant::now());
        let shown = blit(s, &dirty, surface.bounds());
        (*s).dirty = dirty;
        if let (Some(input), Some(handled), Some(framed), true) = (input, handled, framed, shown) {
            note_frame(
                s,
                FrameTimes::between(input, handled, framed, Instant::now()),
            );
        }
    }
    (*s).busy = false;
}

/// Copies the changed areas to the window, returning whether there were any.
///
/// # Safety
/// `s` must point to the live state, on the window's thread.
unsafe fn blit<H2>(s: *mut State<H2>, dirty: &[Rect], bounds: Rect) -> bool {
    let mut any = false;
    for r in dirty
        .iter()
        .map(|r| r.intersect(bounds))
        .filter(|r| !r.is_empty())
    {
        BitBlt(
            (*s).window_dc,
            r.x,
            r.y,
            r.w,
            r.h,
            (*s).memory_dc,
            r.x,
            r.y,
            SRCCOPY,
        );
        any = true;
    }
    GdiFlush();
    any
}

/// # Safety
/// `s` must point to the live state, on the window's thread.
unsafe fn note_frame<H2>(s: *mut State<H2>, times: FrameTimes) {
    if let Some(log) = &mut (*s).frame_log {
        // A log that cannot be written is stopped; the measurement finds it short and says so.
        if log.record(times).is_err() {
            (*s).frame_log = None;
        }
    }
}

/// `s` must point to the live state and the call must be on the window's thread.
unsafe fn repaint<H2>(s: *mut State<H2>, hwnd: H) {
    let mut paint: PAINTSTRUCT = zeroed();
    let dc = BeginPaint(hwnd, &mut paint);
    if !(*s).bitmap.is_null() {
        BitBlt(dc, 0, 0, (*s).w, (*s).h, (*s).memory_dc, 0, 0, SRCCOPY);
    }
    EndPaint(hwnd, &paint);
}

/// Asks Windows to report the pointer leaving, if that is not already arranged.
///
/// # Safety
/// `s` must point to the live state and the call must be on the window's thread.
unsafe fn track_leave<H2>(s: *mut State<H2>, hwnd: H) {
    if (*s).leave_tracked {
        return;
    }
    let mut track = TRACKMOUSEEVENT {
        size: size_of::<TRACKMOUSEEVENT>() as u32,
        flags: TME_LEAVE,
        hwnd,
        hover_time: 0,
    };
    // SAFETY: `track` is a fully initialized struct that outlives the call.
    (*s).leave_tracked = TrackMouseEvent(&mut track) != 0;
}

/// Whether a system command is Alt opening the window menu, which must not happen: Alt is a
/// canvas modifier and there is no menu bar. Alt+Space, with the space as `l`, is the menu that
/// moves and resizes the window, and is left to Windows.
fn swallows_syscommand(w: usize, l: isize) -> bool {
    w & 0xfff0 == SC_KEYMENU && l != 0x20
}

/// # Safety
/// `s` must point to the live state and the call must be on the window's thread.
unsafe fn handle<H2: Handler>(s: *mut State<H2>, hwnd: H, msg: u32, w: usize, l: isize) -> isize {
    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = point(l);
            push(s, Event::Move { x, y });
            track_leave(s, hwnd);
        }
        WM_MOUSELEAVE => {
            (*s).leave_tracked = false;
            push(s, Event::Leave);
        }
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            (*s).alt.spoil();
            SetCapture(hwnd);
            let (x, y) = point(l);
            if let Some(e) = button_event(msg, x, y) {
                push(s, e);
            }
        }
        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            (*s).alt.spoil();
            let (x, y) = point(l);
            if let Some(e) = button_event(msg, x, y) {
                push(s, e);
            }
            if w & 0x13 == 0 {
                ReleaseCapture();
                // Capture can end the tracking without saying so. Asking again is harmless.
                (*s).leave_tracked = false;
            }
        }
        WM_MOUSEWHEEL => {
            let mut p = [
                (l & 0xffff) as i16 as i32,
                ((l >> 16) & 0xffff) as i16 as i32,
            ];
            ScreenToClient(hwnd, &mut p);
            let delta = (*s).wheel_rest + ((w >> 16) & 0xffff) as i16 as i32;
            (*s).wheel_rest = delta % 120;
            if delta / 120 != 0 {
                push(
                    s,
                    Event::Wheel {
                        x: p[0],
                        y: p[1],
                        ticks: delta / 120,
                    },
                );
            }
        }
        WM_KEYDOWN => {
            (*s).alt.key_down(w);
            let mods = modifiers();
            // Ctrl and Alt together are AltGr on most layouts: what it types comes as a character.
            // Shift on the `<` key types `>`: that character decides, not the key's name.
            let key = key_of(w)
                .filter(|_| !(mods.ctrl && mods.alt) && !keymap::defers_to_char(w, mods.shift));
            (*s).keydown_fired = key.is_some();
            if let Some(key) = key {
                let repeat = (l >> 30) & 1 == 1;
                push(s, Event::KeyDown { key, mods, repeat });
            }
        }
        // F10 and Alt with a key arrive as system keys. The menu bar's keys are told to the
        // program; Windows still handles the rest (Alt+Tab, Alt+F4, Alt+Space).
        WM_SYSKEYDOWN => {
            (*s).alt.key_down(w);
            let mods = modifiers();
            if let Some(key) = keymap::menu_bar_key(w).filter(|_| !(mods.ctrl && mods.alt)) {
                let repeat = (l >> 30) & 1 == 1;
                push(s, Event::KeyDown { key, mods, repeat });
            }
            return DefWindowProcW(hwnd, msg, w, l);
        }
        WM_CHAR => typed(s, w as u16, (l >> 30) & 1 == 1),
        WM_KEYUP => {
            alt_released(s, w);
            if let Some(key) = key_of(w) {
                push(s, Event::KeyUp { key });
            }
        }
        // A key released while Alt is down arrives as a system key.
        WM_SYSKEYUP => {
            alt_released(s, w);
            if let Some(key) = key_of(w) {
                push(s, Event::KeyUp { key });
            }
            return DefWindowProcW(hwnd, msg, w, l);
        }
        WM_CAPTURECHANGED if l as H != hwnd => push(s, Event::CaptureLost),
        WM_KILLFOCUS => {
            (*s).alt.lost_focus();
            push(s, Event::CaptureLost);
        }
        WM_CLOSE => push(s, Event::CloseRequested),
        WM_DROPFILES => {
            let paths = crate::drop::dropped_files(w as H);
            if !paths.is_empty() {
                push(s, Event::Dropped(paths));
            }
        }
        WM_QUERYENDSESSION => return session::session_ending(s, hwnd),
        WM_ENDSESSION => session::session_ended(s, hwnd, w != 0),
        WM_GETMINMAXINFO => {
            // The interface falls back to a smaller scale, so the limit does not grow with DPI.
            let scale = (*s).forced_scale.unwrap_or(1);
            let info = l as *mut [i32; 10];
            (*info)[6] = MIN_WINDOW.0 * scale;
            (*info)[7] = MIN_WINDOW.1 * scale;
        }
        WM_RESCALE => {
            (*s).pending_resize = true;
            process(s);
        }
        WM_WAKE => {
            (*s).woken = true;
            process(s);
        }
        WM_DESTROY => PostQuitMessage(0),
        // Alt is a canvas modifier: tapping it, or Alt with a letter, must not open a menu.
        // Alt+Space is left to Windows: it is how a keyboard user moves or resizes the window.
        WM_SYSCOMMAND if swallows_syscommand(w, l) => {}
        WM_ERASEBKGND => return 1,
        WM_PAINT => repaint(s, hwnd),
        // A minimized window keeps its bitmap, so restoring it is instant.
        WM_SIZE if w == SIZE_MINIMIZED => {}
        WM_SIZE => {
            (*s).pending_resize = true;
            process(s);
        }
        // A monitor was removed or the layout changed: a restored window left off every
        // monitor is brought onto the nearest one. A maximized window is resized by Windows.
        WM_DISPLAYCHANGE => {
            if IsZoomed(hwnd) == 0 {
                placement::rescue_offscreen(hwnd);
            }
        }
        WM_SETTINGCHANGE | WM_SYSCOLORCHANGE | WM_THEMECHANGED => look_changed(s),
        WM_DPICHANGED => {
            let r = *(l as *const RECT);
            let flags = SWP_NOZORDER | SWP_NOACTIVATE;
            SetWindowPos(
                hwnd,
                null_mut(),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                flags,
            );
            // A display of the same size sends no WM_SIZE, but the interface scale still changes.
            if !s.is_null() {
                (*s).pending_resize = true;
                process(s);
            }
        }
        WM_SETCURSOR if (l & 0xffff) == HTCLIENT => {
            apply_cursor(CURSOR.load(Ordering::Relaxed));
            return 1;
        }
        _ => return DefWindowProcW(hwnd, msg, w, l),
    }
    0
}

/// Alt came up: if it was pressed alone, the menu bar is asked for.
///
/// # Safety
/// `s` must point to the live state, on the window's thread.
unsafe fn alt_released<H2: Handler>(s: *mut State<H2>, vk: usize) {
    if (*s).alt.key_up(vk, modifiers().ctrl) {
        let (key, mods) = (Key::AltTap, Mods::default());
        push(
            s,
            Event::KeyDown {
                key,
                mods,
                repeat: false,
            },
        );
    }
}

/// Reads the text size again, unless a test fixed it, and lets the handler read the rest.
///
/// # Safety
/// `s` must point to the live state, on the window's thread.
unsafe fn look_changed<H2: Handler>(s: *mut State<H2>) {
    if (*s).forced_text.is_none() {
        crate::scale::set_text_percent(crate::look::text_percent());
    }
    push(s, Event::LookChanged);
    (*s).pending_resize = true;
    process(s);
}

unsafe extern "system" fn wndproc<H2: Handler>(hwnd: H, msg: u32, w: usize, l: isize) -> isize {
    // SAFETY: Windows calls this on the window's thread. USERDATA is either null (before
    // setup) or the `State` that `run` keeps alive until after the window is destroyed.
    unsafe {
        if let Some(answer) = sentinel::answer(msg, w, l) {
            return answer;
        }
        let s = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut State<H2>;
        if s.is_null() {
            return DefWindowProcW(hwnd, msg, w, l);
        }
        if (*s).frame_log.is_some() && (*s).input_at.is_none() && frame_log::is_input(msg) {
            (*s).input_at = Some(Instant::now());
        }
        handle(s, hwnd, msg, w, l)
    }
}

/// The opening window size: comfortable, scaled for the display, and never off screen.
///
/// # Safety
/// The process's DPI awareness must already be set.
unsafe fn default_size(forced_scale: Option<i32>, forced_dpi: Option<i32>) -> (i32, i32) {
    let dpi = forced_dpi.unwrap_or_else(|| GetDpiForSystem() as i32);
    let scale = forced_scale.unwrap_or_else(|| rounded_scale(dpi));
    // A default-placed window opens on the primary monitor; its work area leaves out the taskbar.
    let work = placement::primary_work_area().unwrap_or(RECT {
        left: 0,
        top: 0,
        right: GetSystemMetrics(SM_CXSCREEN),
        bottom: GetSystemMetrics(SM_CYSCREEN),
    });
    placement::first_size(DEFAULT_WINDOW, scale, work)
}

/// # Safety
/// Must be called on the thread that will run the message loop.
unsafe fn create_window<H2: Handler>(title: &str, options: &WindowOptions) -> Result<H, String> {
    setup::set_dpi_awareness();
    let instance = GetModuleHandleW(null());
    let class = wide("PikslarWindow");
    if !setup::register_class(instance, wndproc::<H2>, class.as_ptr()) {
        return Err("Could not register the window class".into());
    }
    let name = wide(title);
    let (x, y, w, h) = options.placement.unwrap_or_else(|| {
        let (w, h) = default_size(options.scale, options.dpi);
        (CW_USEDEFAULT, CW_USEDEFAULT, w, h)
    });
    let ex = if options.topmost { WS_EX_TOPMOST } else { 0 };
    let style = WS_OVERLAPPEDWINDOW | WS_VISIBLE;
    let hwnd = CreateWindowExW(
        ex,
        class.as_ptr(),
        name.as_ptr(),
        style,
        x,
        y,
        w,
        h,
        null_mut(),
        null_mut(),
        instance,
        null_mut(),
    );
    if hwnd.is_null() {
        return Err("Could not create the window".into());
    }
    // Dark from the first paint, so the bar never flashes light; the app corrects it to its look.
    crate::look::dark_title(hwnd, crate::look::high_contrast().is_none());
    crate::drop::accept_files(hwnd);
    Ok(hwnd)
}

/// Waits for input or the next timer, returning false when the app should exit.
///
/// # Safety
/// Must be called on the window's thread.
unsafe fn pump(timeout: u32) -> bool {
    MsgWaitForMultipleObjectsEx(0, null(), timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
    let mut msg: MSG = zeroed();
    while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
        if msg.message == WM_QUIT {
            return false;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    true
}

/// # Safety
/// `hwnd` must be the live window just created on this thread.
unsafe fn new_state<H2>(handler: H2, hwnd: H, options: WindowOptions) -> State<H2> {
    let window_dc = GetDC(hwnd);
    let memory_dc = CreateCompatibleDC(window_dc);
    State {
        handler,
        hwnd,
        window_dc,
        memory_dc,
        bitmap: null_mut(),
        bits: null_mut(),
        w: 0,
        h: 0,
        scale: 1,
        forced_scale: options.scale,
        forced_dpi: options.dpi,
        forced_text: options.text_scale,
        events: Vec::with_capacity(256),
        leave_tracked: false,
        dirty: Vec::with_capacity(64),
        busy: false,
        pending_resize: true,
        woken: false,
        wheel_rest: 0,
        alt: Default::default(),
        utf16: Default::default(),
        keydown_fired: false,
        frame_log: options.frame_log.map(FrameLog::new),
        input_at: None,
    }
}

/// Opens the window and runs until the handler quits.
pub fn run<H2: Handler>(title: &str, options: WindowOptions, handler: H2) -> Result<(), String> {
    // SAFETY: everything below runs on this one thread. `state` is boxed so its address is
    // stable, outlives the window, and is only accessed through the raw pointer while
    // messages are dispatched. `process` guards against re-entry from modal dialogs.
    unsafe {
        crate::scale::set_text_percent(
            options.text_scale.unwrap_or_else(crate::look::text_percent),
        );
        let hwnd = create_window::<H2>(title, &options)?;
        let state = Box::into_raw(Box::new(new_state(handler, hwnd, options)));
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
        process(state);
        loop {
            // Anything that arrived during a modal dialog must not wait for more input.
            let woken = take(&mut (*state).woken);
            let backlog = !(*state).events.is_empty() || (*state).pending_resize || woken;
            let wake = (*state).handler.wake_after();
            let timeout = if backlog {
                0
            } else {
                wake.map_or(INFINITE, |d| d.as_millis() as u32)
            };
            if !pump(timeout) {
                break;
            }
            process(state);
        }
        destroy(state, hwnd);
    }
    Ok(())
}

/// # Safety
/// `state` must be the pointer `run` boxed, and must not be used after this call.
unsafe fn destroy<H2>(state: *mut State<H2>, hwnd: H) {
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    DestroyWindow(hwnd);
    if !(*state).bitmap.is_null() {
        DeleteObject((*state).bitmap);
    }
    drop(Box::from_raw(state));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_alt_tap_or_f10_is_swallowed() {
        assert!(swallows_syscommand(SC_KEYMENU | 0x2, 0));
        assert!(swallows_syscommand(SC_KEYMENU, 0));
    }

    #[test]
    fn an_alt_letter_or_digit_chord_is_swallowed() {
        assert!(swallows_syscommand(SC_KEYMENU, isize::from(b'f')));
        assert!(swallows_syscommand(SC_KEYMENU, isize::from(b'7')));
    }

    #[test]
    fn alt_space_reaches_the_system_menu() {
        assert!(!swallows_syscommand(SC_KEYMENU, 0x20));
    }

    #[test]
    fn other_system_commands_are_not_swallowed() {
        assert!(!swallows_syscommand(0xF060, 0), "close");
        assert!(!swallows_syscommand(0xF010, 0), "move");
    }
}
