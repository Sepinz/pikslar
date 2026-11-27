//! Windows, input and getting pixels on screen.
//!
//! Only Win32 exists so far. The interface is deliberately small so other systems can
//! implement it later.

#[cfg(not(windows))]
compile_error!("pikslar-platform currently supports Windows only");

mod admit;
mod alt_tap;
mod box_keys;
mod clipboard;
mod clipboard_text;
mod clock;
pub mod console;
mod dialogs;
mod drop;
mod ffi;
pub mod frame_log;
mod glyphs;
mod keymap;
mod long_path;
mod look;
mod replace;
mod scale;
mod session_end;
mod wait;
mod window;

pub use clipboard::{clipboard_stamp, copy_image, paste_image, ClipboardImage};
pub use clipboard_text::{copy_text, paste_text, TextPaste};
pub use clock::local_clock;
pub use dialogs::{
    ask_changed_on_disk, ask_forget_all_history, ask_open_backup, ask_replace_backup,
    ask_replace_file, ask_restore_work, ask_save_changes, ask_upgrade, open_file_dialog,
    open_file_dialog_titled, save_file_dialog, save_file_dialog_titled, show_error, ChangedChoice,
    SaveChoice, UpgradeChoice,
};
pub use glyphs::rasterize_glyph;
pub use look::{high_contrast, text_percent, SystemColors};
pub use replace::{move_new, move_over, replace_file};
pub use scale::{
    chosen_scale, chosen_scale_with_text, size_fits, text_percent_from, LARGEST_SIZE,
    SMALLEST_UNITS,
};
pub use session_end::{
    blocking_shutdown, end_budget, recovery_writes_pending, run_session_fallback,
    set_session_fallback, wait_for_recovery_writes, wait_for_writes, write_pending, writes_pending,
    SessionFallback, WriteKind, WritePending, BLOCK_REASON, END_WAIT, QUERY_FALLBACK_WAIT,
};
pub use wait::wait_answering;
pub use window::{interface_scale, run, Cursor, Waker, Window, WindowOptions};

use pikslar_render::{Rect, Surface};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A letter or digit, in upper case.
    Char(u8),
    Space,
    Escape,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    BracketLeft,
    BracketRight,
    Comma,
    Period,
    Plus,
    Minus,
    Delete,
    Backspace,
    Tab,
    /// A function key, 1 to 12.
    F(u8),
    /// The key that opens a context menu, VK_APPS.
    Menu,
    /// Alt pressed and released on its own, which takes the keyboard to the menu bar.
    AltTap,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Move {
        x: i32,
        y: i32,
    },
    Down {
        x: i32,
        y: i32,
        button: Button,
        mods: Mods,
    },
    Up {
        x: i32,
        y: i32,
        button: Button,
    },
    Wheel {
        x: i32,
        y: i32,
        ticks: i32,
    },
    KeyDown {
        key: Key,
        mods: Mods,
        repeat: bool,
    },
    KeyUp {
        key: Key,
    },
    /// A character typed, as the keyboard layout made it. Comes after the key's `KeyDown`.
    Text(char),
    Resized {
        w: i32,
        h: i32,
        scale: i32,
    },
    /// The pointer left the window.
    Leave,
    /// Mouse capture or focus went away mid-gesture.
    CaptureLost,
    CloseRequested,
    /// Files dropped on the window from Explorer, in the order given.
    Dropped(Vec<std::path::PathBuf>),
    /// A system setting changed that can change the look: high contrast, the system colors or
    /// the text size. A `Resized` with the new scale follows.
    LookChanged,
}

/// The application side of the event loop.
pub trait Handler {
    fn event(&mut self, event: Event, window: Window);

    /// Draws whatever changed and lists the changed rectangles in `dirty`.
    fn frame(&mut self, surface: &mut Surface<'_>, window: Window, dirty: &mut Vec<Rect>);

    /// How long the app can sleep with no input. `None` waits for input forever.
    fn wake_after(&self) -> Option<Duration>;

    /// Windows is logging off or shutting down, so work must be made safe now without
    /// waiting for the user. Returning false asks Windows to wait.
    fn session_ending(&mut self, window: Window) -> bool;

    /// `WM_ENDSESSION`: the session ends now when `ending`, or the end was called off.
    /// Writes holding a [`WritePending`] are waited for afterwards.
    fn session_ended(&mut self, _window: Window, _ending: bool) {}
}
