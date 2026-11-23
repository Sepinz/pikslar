//! Which key a keyboard message means, whatever the keyboard layout.
//!
//! Letters, digits and the named keys have the same virtual-key codes on every layout. The
//! punctuation keys do not: `[` is its own key on a US keyboard, AltGr and 8 on a Norwegian or
//! German one. So punctuation shortcuts are named by the character the layout gives, never by
//! where the key sits.

use crate::Key;

/// The shortcut a typed character stands for, if it is one of the punctuation shortcuts.
pub(crate) fn key_for_char(c: char) -> Option<Key> {
    Some(match c {
        '[' => Key::BracketLeft,
        ']' => Key::BracketRight,
        ',' | '<' => Key::Comma,
        '.' | '>' => Key::Period,
        '+' | '=' => Key::Plus,
        '-' => Key::Minus,
        _ => return None,
    })
}

/// Whether a key-down is left to the character that follows it. Shift on the `<` key of a
/// Nordic, German or French keyboard types `>`, the next-frame key, so the key's own name
/// (`<`, previous frame) must not fire.
pub(crate) fn defers_to_char(vk: usize, shift: bool) -> bool {
    shift && vk == 0xe2
}

/// The shortcut a typed character gives when its key-down gave none: on French and Belgian
/// keyboards Shift with the key marked `;` types `.`. With Ctrl the character is a control
/// code, and with Alt it is AltGr's, which is handled apart.
pub(crate) fn char_shortcut(c: char, ctrl: bool, alt: bool, keydown_fired: bool) -> Option<Key> {
    if keydown_fired || ctrl || alt {
        return None;
    }
    key_for_char(c)
}

/// The punctuation keys, whose characters differ from layout to layout.
fn is_punctuation(vk: usize) -> bool {
    matches!(vk, 0xba..=0xc0 | 0xdb..=0xdf | 0xe2)
}

/// The character a US layout puts on a punctuation key that has a shortcut.
fn us_char(vk: usize) -> Option<char> {
    Some(match vk {
        0xdb => '[',
        0xdd => ']',
        0xbc => ',',
        0xbe => '.',
        0xbb => '=',
        0xbd => '-',
        _ => return None,
    })
}

/// The key a virtual-key code means. `layout_char` gives the character the layout in use puts
/// on a key, unshifted.
pub(crate) fn key_from(vk: usize, layout_char: impl Fn(usize) -> Option<char>) -> Option<Key> {
    if is_punctuation(vk) {
        return match layout_char(vk) {
            // A layout for another script, Russian or Greek say, puts its own letters on these
            // keys and types its punctuation elsewhere: the key keeps its US meaning.
            Some(c) if c.is_alphabetic() && u32::from(c) > 0x24f => us_char(vk),
            c => c,
        }
        .and_then(key_for_char);
    }
    Some(match vk {
        0x30..=0x39 | 0x41..=0x5a => Key::Char(vk as u8),
        0x20 => Key::Space,
        0x1b => Key::Escape,
        0x0d => Key::Enter,
        0x25 => Key::Left,
        0x26 => Key::Up,
        0x27 => Key::Right,
        0x28 => Key::Down,
        0x24 => Key::Home,
        0x23 => Key::End,
        0x6b => Key::Plus,
        0x6d => Key::Minus,
        0x2e => Key::Delete,
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x70..=0x7b => Key::F((vk - 0x6f) as u8),
        0x5d => Key::Menu,
        _ => return None,
    })
}

/// The system keys (those that arrive with Alt, or F10) the menu bar answers: F10, and a letter
/// or digit, which with Alt is a menu's access key. Everything else, Alt+Tab, Alt+F4 and
/// Alt+Space, is left to Windows.
pub(crate) fn menu_bar_key(vk: usize) -> Option<Key> {
    match vk {
        0x30..=0x39 | 0x41..=0x5a => Some(Key::Char(vk as u8)),
        0x79 => Some(Key::F(10)),
        _ => None,
    }
}

/// Joins the UTF-16 units `WM_CHAR` brings into characters: a character outside the basic
/// plane comes as two messages.
#[derive(Debug, Default)]
pub(crate) struct Utf16 {
    high: Option<u16>,
}

impl Utf16 {
    pub(crate) fn push(&mut self, unit: u16) -> Option<char> {
        match unit {
            0xd800..=0xdbff => {
                self.high = Some(unit);
                None
            }
            0xdc00..=0xdfff => {
                let high = self.high.take()?;
                char::decode_utf16([high, unit]).next()?.ok()
            }
            _ => {
                self.high = None;
                char::from_u32(unit as u32)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_bar_answers_f10_and_alt_letters_only() {
        assert_eq!(menu_bar_key(0x79), Some(Key::F(10)));
        assert_eq!(menu_bar_key(0x46), Some(Key::Char(b'F')));
        assert_eq!(menu_bar_key(0x31), Some(Key::Char(b'1')));
        for vk in [0x09, 0x73, 0x20, 0x25] {
            assert_eq!(menu_bar_key(vk), None, "vk {vk:#x}");
        }
    }

    #[test]
    fn a_bare_modifier_is_no_key_at_all() {
        // Shift, Ctrl, Alt, either Windows key and Caps Lock, left and right, make no event, so
        // none of them can dismiss an error message or move a menu.
        for vk in [
            0x10, 0x11, 0x12, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0x5b, 0x5c, 0x14,
        ] {
            assert_eq!(key_from(vk, |_| None), None, "vk {vk:#x}");
        }
    }

    /// The unshifted characters of a few punctuation keys on two layouts.
    fn us(vk: usize) -> Option<char> {
        Some(match vk {
            0xdb => '[',
            0xdd => ']',
            0xbc => ',',
            0xbe => '.',
            0xbb => '=',
            0xbd => '-',
            0xe2 => '\\',
            _ => return None,
        })
    }

    fn norwegian(vk: usize) -> Option<char> {
        Some(match vk {
            0xdd => 'å',
            0xba => '¨',
            0xbc => ',',
            0xbe => '.',
            0xbb => '+',
            0xbd => '-',
            0xe2 => '<',
            _ => return None,
        })
    }

    /// Russian ЙЦУКЕН: its letters sit where US has [ ] , . and the punctuation is elsewhere.
    fn russian(vk: usize) -> Option<char> {
        Some(match vk {
            0xdb => 'х',
            0xdd => 'ъ',
            0xbc => 'б',
            0xbe => 'ю',
            0xbf => '.',
            0xbb => '=',
            0xbd => '-',
            _ => return None,
        })
    }

    /// French AZERTY: the key marked `;` (OEM_PERIOD) gives `;` and `.` with Shift; `,` is `?`
    /// with Shift; the `<` key is `<`.
    fn azerty(vk: usize) -> Option<char> {
        Some(match vk {
            0xbc => ',',
            0xbe => ';',
            0xbf => ':',
            0xbb => '=',
            0xdb => ')',
            0xdd => '^',
            0xe2 => '<',
            _ => return None,
        })
    }

    /// Belgian period layout: the same keys as French for these.
    fn belgian(vk: usize) -> Option<char> {
        azerty(vk)
    }

    /// German QWERTZ: `,` and `.` have their own keys, `+` is its own key, `<` is the extra key.
    fn german(vk: usize) -> Option<char> {
        Some(match vk {
            0xbc => ',',
            0xbe => '.',
            0xbb => '+',
            0xbd => '-',
            0xdb => 'ß',
            0xdd => '\u{b4}',
            0xe2 => '<',
            _ => return None,
        })
    }

    #[test]
    fn azerty_and_belgian_reach_next_frame_by_the_typed_character() {
        for layout in [azerty, belgian] {
            assert_eq!(key_from(0xbe, layout), None, "the key is named ;");
            assert_eq!(key_from(0xbc, layout), Some(Key::Comma));
            // Shift with that key types `.`, and its key-down gave nothing.
            assert_eq!(char_shortcut('.', false, false, false), Some(Key::Period));
        }
        assert_eq!(key_from(0xe2, azerty), Some(Key::Comma));
    }

    #[test]
    fn german_and_nordic_shift_on_the_angle_key_is_next_frame() {
        for layout in [german, norwegian] {
            assert_eq!(key_from(0xe2, layout), Some(Key::Comma));
            assert!(defers_to_char(0xe2, true), "Shift+< waits for the >");
            assert!(!defers_to_char(0xe2, false));
            assert_eq!(key_from(0xbe, layout), Some(Key::Period));
        }
        assert_eq!(char_shortcut('>', false, false, false), Some(Key::Period));
        assert!(
            !defers_to_char(0xbe, true),
            "other keys keep their key-down"
        );
        assert_eq!(key_from(0xbb, german), Some(Key::Plus));
    }

    #[test]
    fn a_character_never_fires_after_its_own_key_down() {
        assert_eq!(char_shortcut('.', false, false, true), None);
        assert_eq!(char_shortcut('<', false, false, true), None);
    }

    #[test]
    fn a_character_typed_with_ctrl_or_alt_is_no_second_shortcut() {
        assert_eq!(char_shortcut('.', true, false, false), None);
        assert_eq!(char_shortcut('.', false, true, false), None);
        assert_eq!(char_shortcut('a', false, false, false), None);
    }

    #[test]
    fn a_layout_for_another_script_keeps_the_us_meaning_of_its_letter_keys() {
        assert_eq!(key_from(0xdb, russian), Some(Key::BracketLeft));
        assert_eq!(key_from(0xdd, russian), Some(Key::BracketRight));
        assert_eq!(key_from(0xbc, russian), Some(Key::Comma));
        assert_eq!(key_from(0xbe, russian), Some(Key::Period));
        assert_eq!(
            key_from(0xbf, russian),
            Some(Key::Period),
            "its own full stop"
        );
        assert_eq!(
            key_from(0xdd, norwegian),
            None,
            "a Latin letter stays a letter"
        );
    }

    #[test]
    fn punctuation_is_named_by_the_character_the_layout_gives() {
        assert_eq!(key_from(0xdb, us), Some(Key::BracketLeft));
        assert_eq!(key_from(0xbb, us), Some(Key::Plus));
        assert_eq!(key_from(0xe2, us), None, "backslash is no shortcut");
        assert_eq!(key_from(0xbb, norwegian), Some(Key::Plus));
        assert_eq!(
            key_from(0xe2, norwegian),
            Some(Key::Comma),
            "the < key moves frames earlier"
        );
        assert_eq!(key_from(0xdd, norwegian), None, "å is a letter, not ]");
        assert_eq!(key_from(0xdb, |_| None), None, "a key with no character");
    }

    #[test]
    fn letters_digits_and_named_keys_do_not_depend_on_the_layout() {
        assert_eq!(key_from(0x4e, norwegian), Some(Key::Char(b'N')));
        assert_eq!(key_from(0x37, norwegian), Some(Key::Char(b'7')));
        assert_eq!(key_from(0x6b, norwegian), Some(Key::Plus), "the keypad");
        assert_eq!(key_from(0x2e, us), Some(Key::Delete));
    }

    #[test]
    fn function_keys_and_the_menu_key_are_keys() {
        assert_eq!(key_from(0x79, us), Some(Key::F(10)));
        assert_eq!(key_from(0x70, us), Some(Key::F(1)));
        assert_eq!(key_from(0x7b, us), Some(Key::F(12)));
        assert_eq!(key_from(0x5d, us), Some(Key::Menu), "VK_APPS");
        assert_eq!(key_from(0x7c, us), None, "F13 is not one");
        assert_eq!(key_from(0x5b, us), None, "the Windows key is still no key");
    }

    #[test]
    fn characters_typed_with_altgr_still_reach_their_shortcuts() {
        assert_eq!(key_for_char('['), Some(Key::BracketLeft));
        assert_eq!(key_for_char('>'), Some(Key::Period));
        assert_eq!(key_for_char('ø'), None);
    }

    #[test]
    fn surrogate_pairs_make_one_character_and_a_lone_half_makes_none() {
        let mut u = Utf16::default();
        assert_eq!(u.push('é' as u16), Some('é'));
        assert_eq!(u.push(0xd83d), None);
        assert_eq!(u.push(0xde00), Some('\u{1f600}'));
        assert_eq!(u.push(0xde00), None, "a low half on its own");
        assert_eq!(u.push(0xd83d), None);
        assert_eq!(
            u.push('a' as u16),
            Some('a'),
            "a high half followed by no low one"
        );
        assert_eq!(u.push(0xde00), None, "and is forgotten after it");
    }
}
