//! A tap of the Alt key on its own, which takes the keyboard to the menu bar as it does in
//! Windows programs. Alt is also a canvas modifier, so it only counts when nothing else was
//! pressed between its going down and its coming up: no other key, no mouse button. A key that
//! was already down when Alt went down spoils every Alt press until it is up (AltGr sends Ctrl
//! and Alt together, and types a character instead).

use crate::ffi::VK_MENU;

/// Tracks whether the Alt key has been alone since it went down, and which keys are down.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AltTap {
    alone: bool,
    /// Alt is down, so its repeats start nothing new.
    alt_down: bool,
    /// The other keys down, by virtual-key code.
    held: [u64; 4],
}

impl AltTap {
    fn others_down(&self) -> bool {
        self.held.iter().any(|&w| w != 0)
    }

    /// A key went down: Alt starts a tap if no other key is down, any other key spoils it.
    pub(crate) fn key_down(&mut self, vk: usize) {
        if vk == VK_MENU as usize {
            // A repeat of Alt keeps the tap it has; a fresh press starts one.
            if !self.alt_down {
                self.alt_down = true;
                self.alone = !self.others_down();
            }
        } else {
            self.alone = false;
            self.held[(vk >> 6) & 3] |= 1 << (vk & 63);
        }
    }

    /// A mouse button went down or came up while Alt may be held: Alt was used for something else.
    pub(crate) fn spoil(&mut self) {
        self.alone = false;
    }

    /// Focus left the window: no key is known to be down.
    pub(crate) fn lost_focus(&mut self) {
        *self = AltTap::default();
    }

    /// A key came up. True when it is Alt, ending a tap, with Ctrl not held.
    pub(crate) fn key_up(&mut self, vk: usize, ctrl: bool) -> bool {
        if vk != VK_MENU as usize {
            self.held[(vk >> 6) & 3] &= !(1 << (vk & 63));
            return false;
        }
        let tapped = self.alone && !ctrl;
        self.alone = false;
        self.alt_down = false;
        tapped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALT: usize = VK_MENU as usize;
    const F: usize = 0x46;

    #[test]
    fn alt_down_and_up_alone_is_a_tap() {
        let mut t = AltTap::default();
        t.key_down(ALT);
        assert!(t.key_up(ALT, false));
        assert!(!t.key_up(ALT, false), "a tap is spent");
    }

    #[test]
    fn a_held_alt_repeating_is_still_one_tap() {
        let mut t = AltTap::default();
        t.key_down(ALT);
        t.key_down(ALT);
        assert!(t.key_up(ALT, false));
    }

    #[test]
    fn another_key_between_spoils_the_tap() {
        let mut t = AltTap::default();
        t.key_down(ALT);
        t.key_down(F);
        assert!(!t.key_up(F, false));
        assert!(!t.key_up(ALT, false));
    }

    #[test]
    fn a_mouse_button_or_lost_focus_spoils_the_tap() {
        let mut t = AltTap::default();
        t.key_down(ALT);
        t.spoil();
        assert!(!t.key_up(ALT, false));
    }

    #[test]
    fn a_release_with_no_press_is_no_tap() {
        let mut t = AltTap::default();
        assert!(!t.key_up(ALT, false));
    }

    #[test]
    fn altgr_with_control_is_no_tap() {
        let mut t = AltTap::default();
        t.key_down(ALT);
        assert!(!t.key_up(ALT, true));
    }

    const CTRL: usize = 0x11;

    #[test]
    fn altgr_sends_control_then_alt_and_is_no_tap() {
        let mut t = AltTap::default();
        t.key_down(CTRL);
        t.key_down(ALT);
        assert!(!t.key_up(ALT, false), "even with Ctrl already read as up");
        t.key_up(CTRL, false);
        // The next lone tap works again.
        t.key_down(ALT);
        assert!(t.key_up(ALT, false));
    }

    #[test]
    fn a_key_held_before_alt_keeps_every_alt_press_spoiled_until_it_is_up() {
        let mut t = AltTap::default();
        t.key_down(F);
        t.key_down(ALT);
        assert!(!t.key_up(ALT, false));
        t.key_down(ALT);
        assert!(!t.key_up(ALT, false), "F is still down");
        t.key_up(F, false);
        t.key_down(ALT);
        assert!(t.key_up(ALT, false), "all keys are up");
    }

    #[test]
    fn a_key_pressed_and_released_during_the_hold_spoils_it() {
        let mut t = AltTap::default();
        t.key_down(ALT);
        t.key_down(F);
        t.key_up(F, false);
        assert!(!t.key_up(ALT, false));
    }

    #[test]
    fn a_mouse_button_pressed_or_released_during_the_hold_spoils_it() {
        let mut t = AltTap::default();
        t.key_down(ALT);
        t.spoil();
        t.spoil();
        assert!(!t.key_up(ALT, false));
        t.key_down(ALT);
        assert!(t.key_up(ALT, false), "the next hold is fresh");
    }

    #[test]
    fn losing_focus_forgets_held_keys() {
        let mut t = AltTap::default();
        t.key_down(F);
        t.lost_focus();
        t.key_down(ALT);
        assert!(t.key_up(ALT, false));
    }
}
