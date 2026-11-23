//! Turning key and mouse messages into events.

use super::{push, State};
use crate::ffi::*;
use crate::{keymap, Button, Event, Handler, Key, Mods};

pub(crate) fn point(l: isize) -> (i32, i32) {
    (
        (l & 0xffff) as i16 as i32,
        ((l >> 16) & 0xffff) as i16 as i32,
    )
}

/// The character the keyboard layout in use puts on a key, unshifted.
fn layout_char(vk: usize) -> Option<char> {
    // SAFETY: MapVirtualKeyW takes plain numbers and reads the thread's keyboard layout.
    let code = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_CHAR) };
    // The character is the low word; the top bit marks a dead key, such as an accent waiting
    // for its letter, which still names its key.
    char::from_u32(code & 0xffff).filter(|&c| c != '\0')
}

pub(crate) fn key_of(vk: usize) -> Option<Key> {
    keymap::key_from(vk, layout_char)
}

/// A typed character: text for whatever is being typed into, and, if AltGr typed one of the
/// punctuation shortcuts, that key too.
///
/// # Safety
/// `s` is the window's state, as in the window procedure.
pub(super) unsafe fn typed<H2: Handler>(s: *mut State<H2>, unit: u16, repeat: bool) {
    let Some(c) = (*s).utf16.push(unit) else {
        return;
    };
    let mods = modifiers();
    let fired = std::mem::take(&mut (*s).keydown_fired);
    let key = if mods.ctrl && mods.alt {
        keymap::key_for_char(c)
    } else {
        // Keys whose name differs from what they type with Shift, as on AZERTY.
        keymap::char_shortcut(c, mods.ctrl, mods.alt, fired)
    };
    if let Some(key) = key {
        let mods = Mods::default();
        push(s, Event::KeyDown { key, mods, repeat });
    }
    if !c.is_control() {
        push(s, Event::Text(c));
    }
}

pub(crate) fn modifiers() -> Mods {
    // SAFETY: GetKeyState takes a plain key code and reads this thread's input state.
    let held = |vk| unsafe { GetKeyState(vk) } < 0;
    Mods {
        ctrl: held(VK_CONTROL),
        shift: held(VK_SHIFT),
        alt: held(VK_MENU),
    }
}

pub(crate) fn button_event(msg: u32, x: i32, y: i32) -> Option<Event> {
    Some(match msg {
        WM_LBUTTONDOWN => Event::Down {
            x,
            y,
            button: Button::Left,
            mods: modifiers(),
        },
        WM_RBUTTONDOWN => Event::Down {
            x,
            y,
            button: Button::Right,
            mods: modifiers(),
        },
        WM_MBUTTONDOWN => Event::Down {
            x,
            y,
            button: Button::Middle,
            mods: modifiers(),
        },
        WM_LBUTTONUP => Event::Up {
            x,
            y,
            button: Button::Left,
        },
        WM_RBUTTONUP => Event::Up {
            x,
            y,
            button: Button::Right,
        },
        WM_MBUTTONUP => Event::Up {
            x,
            y,
            button: Button::Middle,
        },
        _ => return None,
    })
}
