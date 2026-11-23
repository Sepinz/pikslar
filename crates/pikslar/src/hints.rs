//! Hints that name a shortcut, built from the live keymap so rebinding keeps them true.

use crate::shortcuts::{binding_name, Action, Keymap};

impl Keymap {
    /// `text`, then a tab and the action's key, for a menu row that shows its key at the
    /// right. An action with no key leaves it out.
    pub fn tab_hint(&self, text: &str, action: Action) -> String {
        match self.binding(action) {
            Some(_) => format!("{text}\t{}", self.chord(action)),
            None => text.to_string(),
        }
    }

    /// How `action`'s key is written, or `none` when it has no key.
    pub fn chord(&self, action: Action) -> String {
        binding_name(self.binding(action))
    }

    /// `text`, two spaces and the action's key, then two spaces and `tail` when it is not empty.
    /// An action with no key leaves its key out.
    pub fn hint(&self, text: &str, action: Action, tail: &str) -> String {
        let mut out = text.to_string();
        if self.binding(action).is_some() {
            out.push_str("  ");
            out.push_str(&self.chord(action));
        }
        if !tail.is_empty() {
            out.push_str("  ");
            out.push_str(tail);
        }
        out
    }

    /// The key that undoes `action` when it has Shift: its own key with Shift added.
    pub fn back_chord(&self, action: Action) -> String {
        match self.binding(action) {
            Some(b) => binding_name(Some(crate::shortcuts::Binding { shift: true, ..b })),
            None => "none".to_string(),
        }
    }

    /// The hint of the selection button `i`.
    pub fn selection_hint(&self, i: usize) -> String {
        match i {
            0 => self.hint("Flip across", Action::FlipHorizontal, ""),
            1 => self.hint("Flip upside down", Action::FlipVertical, ""),
            _ => format!(
                "Rotate a quarter turn  {}, back  {}",
                self.chord(Action::RotateClockwise),
                self.chord(Action::RotateCounter)
            ),
        }
    }

    /// The hint of the text tool's button `i`.
    pub fn text_setting_hint(&self, i: usize) -> String {
        let pair = |text: &str, a: Action| {
            format!("{text}  {}, back  {}", self.chord(a), self.back_chord(a))
        };
        match i {
            0 => self.hint("Font: 5x7 or tiny", Action::TextFont, ""),
            1 => pair("Size: whole-pixel scale", Action::TextScale),
            2 => pair("Gap between letters", Action::TextSpacing),
            _ => pair("Align the lines", Action::TextAlign),
        }
    }

    /// The hint of the transport button `i` that names keys, if it is one of those.
    pub fn transport_hint(&self, i: usize) -> Option<String> {
        match i {
            3 => Some(self.hint("New frame", Action::NewFrame, "")),
            4 => Some(self.hint("Duplicate frame", Action::DuplicateFrame, "")),
            6 => Some(format!(
                "Onion skin  {}, depth  {}. Right click for more",
                self.chord(Action::Onion),
                self.chord(Action::OnionDepth)
            )),
            _ => None,
        }
    }
}
