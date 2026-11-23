//! A line of editable text: a caret, a selection, and the edits keys make. Drawing is in
//! [`crate::text_field`]; this part knows nothing of the screen, so it is tested on its own.

use pikslar_platform::{Key, Mods};

/// What a key does to a line of text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Puts a character in place of the selection, or at the caret.
    Insert(char),
    /// Puts a line break in place of the selection, or at the caret. Only a field of several
    /// lines uses it; a typed control character is still refused.
    Newline,
    /// Removes the selection, or the character before the caret.
    Back,
    /// Removes the selection, or the character after the caret.
    Delete,
    /// Moves the caret; with `extend` the selection grows from where it was.
    Left {
        extend: bool,
    },
    Right {
        extend: bool,
    },
    Home {
        extend: bool,
    },
    End {
        extend: bool,
    },
    /// Moves the caret to the start of the previous word.
    WordLeft {
        extend: bool,
    },
    /// Moves the caret to the start of the next word, or the end of the text.
    WordRight {
        extend: bool,
    },
    SelectAll,
    /// Changes nothing here: the caller puts [`TextEdit::selected_text`] on the clipboard.
    Copy,
    /// Removes the selection; the caller puts [`TextEdit::selected_text`] on the clipboard first.
    Cut,
    /// Changes nothing here: the caller types the clipboard's text, one `Insert` a character.
    Paste,
}

/// The edit a key makes in a line of text, if it makes one. Characters are not keys: they come
/// as text, from the keyboard layout.
pub fn edit_for(key: Key, mods: Mods) -> Option<Edit> {
    let extend = mods.shift;
    // Ctrl with Alt is how AltGr arrives, and it types: only the keys that move or erase keep
    // meaning with Alt held.
    let chord = mods.ctrl && !mods.alt;
    Some(match key {
        Key::Delete => Edit::Delete,
        Key::Backspace => Edit::Back,
        Key::Left if chord => Edit::WordLeft { extend },
        Key::Right if chord => Edit::WordRight { extend },
        Key::Left => Edit::Left { extend },
        Key::Right => Edit::Right { extend },
        Key::Home => Edit::Home { extend },
        Key::End => Edit::End { extend },
        Key::Char(b'A') if chord => Edit::SelectAll,
        Key::Char(b'C') if chord => Edit::Copy,
        Key::Char(b'V') if chord => Edit::Paste,
        Key::Char(b'X') if chord => Edit::Cut,
        _ => return None,
    })
}

/// Pasted text as a one-line field takes it: the first line, a tab as a space, no other control
/// characters.
pub fn one_line(text: &str) -> String {
    let first = text.split(['\r', '\n']).next().unwrap_or_default();
    first
        .chars()
        .filter_map(|c| match c {
            '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// Pasted text as a field of several lines takes it: every kind of line break one `\n`, a tab a
/// space, no other control characters.
pub fn multi_line(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter_map(|c| match c {
            '\r' | '\n' => Some('\n'),
            '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// What a word move steps over: letters and digits, spaces, or anything else.
#[derive(PartialEq)]
enum Class {
    Word,
    Space,
    Mark,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Mark
    }
}

/// The text, with the caret and the other end of the selection as character counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextEdit {
    text: String,
    caret: usize,
    anchor: usize,
    max_chars: usize,
}

impl TextEdit {
    /// `text` cut to `max_chars` characters, all of it selected, so typing replaces it.
    pub fn new(text: &str, max_chars: usize) -> Self {
        let text: String = text.chars().take(max_chars).collect();
        let n = text.chars().count();
        TextEdit {
            text,
            caret: n,
            anchor: 0,
            max_chars,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Where the caret is, in characters from the start.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The selected characters, as a start and an end; empty when they are equal.
    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    /// How many characters can be typed or pasted now: what is left of the limit, counting the
    /// selection, which the first of them replaces.
    pub fn room(&self) -> usize {
        let (from, to) = self.selection();
        self.max_chars.saturating_sub(self.len() - (to - from))
    }

    /// The selected text; empty when nothing is selected.
    pub fn selected_text(&self) -> &str {
        let (from, to) = self.selection();
        &self.text[self.byte(from)..self.byte(to)]
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// The byte where character `at` starts.
    fn byte(&self, at: usize) -> usize {
        self.text
            .char_indices()
            .nth(at)
            .map_or(self.text.len(), |(b, _)| b)
    }

    /// Removes the selection. Returns whether there was one.
    fn take_selection(&mut self) -> bool {
        let (from, to) = self.selection();
        if from == to {
            return false;
        }
        let (a, b) = (self.byte(from), self.byte(to));
        self.text.replace_range(a..b, "");
        (self.caret, self.anchor) = (from, from);
        true
    }

    fn place(&mut self, at: usize, extend: bool) {
        self.caret = at;
        if !extend {
            self.anchor = at;
        }
    }

    fn insert(&mut self, c: char) -> bool {
        let took = self.take_selection();
        if self.len() >= self.max_chars {
            return took;
        }
        let at = self.byte(self.caret);
        self.text.insert(at, c);
        self.place(self.caret + 1, false);
        true
    }

    /// The same field with every character changed by `f`, as when line breaks are shown as marks.
    pub fn mapped(&self, f: impl Fn(char) -> char) -> TextEdit {
        TextEdit {
            text: self.text.chars().map(f).collect(),
            ..self.clone()
        }
    }

    /// Makes an edit. Returns whether the text changed; moving the caret is not a change.
    pub fn apply(&mut self, edit: Edit) -> bool {
        let n = self.len();
        match edit {
            Edit::Insert(c) if c.is_control() => false,
            Edit::Insert(c) => self.insert(c),
            Edit::Newline => self.insert('\n'),
            Edit::Back => self.take_selection() || self.remove_at(self.caret.checked_sub(1)),
            Edit::Delete => self.take_selection() || self.remove_at(Some(self.caret)),
            Edit::Left { extend } => self.step(self.caret.saturating_sub(1), extend, true),
            Edit::Right { extend } => self.step((self.caret + 1).min(n), extend, false),
            Edit::Home { extend } => {
                self.place(0, extend);
                false
            }
            Edit::End { extend } => {
                self.place(n, extend);
                false
            }
            Edit::WordLeft { extend } => {
                self.place(self.word_start(), extend);
                false
            }
            Edit::WordRight { extend } => {
                self.place(self.word_end(), extend);
                false
            }
            Edit::SelectAll => {
                (self.anchor, self.caret) = (0, n);
                false
            }
            Edit::Copy | Edit::Paste => false,
            Edit::Cut => self.take_selection(),
        }
    }

    /// The start of the word before the caret: back over spaces, then over one kind of character.
    fn word_start(&self) -> usize {
        let chars: Vec<char> = self.text.chars().take(self.caret).collect();
        let mut at = chars.len();
        while at > 0 && class(chars[at - 1]) == Class::Space {
            at -= 1;
        }
        if let Some(&last) = chars[..at].last() {
            while at > 0 && class(chars[at - 1]) == class(last) {
                at -= 1;
            }
        }
        at
    }

    /// The start of the next word: over one kind of character, then over spaces.
    fn word_end(&self) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut at = self.caret.min(chars.len());
        if let Some(first) = chars.get(at).map(|&c| class(c)) {
            while chars
                .get(at)
                .is_some_and(|&c| class(c) == first && first != Class::Space)
            {
                at += 1;
            }
        }
        while chars.get(at).is_some_and(|&c| class(c) == Class::Space) {
            at += 1;
        }
        at
    }

    /// An arrow: without `extend`, a selection collapses to its end on that side first.
    fn step(&mut self, to: usize, extend: bool, left: bool) -> bool {
        let (from, end) = self.selection();
        if !extend && from != end {
            self.place(if left { from } else { end }, false);
        } else {
            self.place(to, extend);
        }
        false
    }

    fn remove_at(&mut self, at: Option<usize>) -> bool {
        let Some(at) = at.filter(|&a| a < self.len()) else {
            return false;
        };
        let (a, b) = (self.byte(at), self.byte(at + 1));
        self.text.replace_range(a..b, "");
        self.place(at, false);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(edit: &mut TextEdit, text: &str) {
        for c in text.chars() {
            edit.apply(Edit::Insert(c));
        }
    }

    #[test]
    fn typing_replaces_the_text_it_starts_with() {
        let mut e = TextEdit::new("walk", 20);
        assert_eq!(e.selection(), (0, 4));
        typed(&mut e, "løp");
        assert_eq!((e.text(), e.caret()), ("løp", 3));
    }

    #[test]
    fn the_caret_moves_and_edits_by_characters_not_bytes() {
        let mut e = TextEdit::new("aéb", 20);
        e.apply(Edit::End { extend: false });
        e.apply(Edit::Left { extend: false });
        assert!(e.apply(Edit::Back), "the é goes");
        assert_eq!((e.text(), e.caret()), ("ab", 1));
        typed(&mut e, "ø");
        assert_eq!(e.text(), "aøb");
        assert!(e.apply(Edit::Delete));
        assert_eq!(e.text(), "aø");
        assert!(!e.apply(Edit::Delete), "nothing after the caret");
        e.apply(Edit::Home { extend: false });
        assert!(!e.apply(Edit::Back), "nothing before it");
    }

    #[test]
    fn shift_arrows_select_and_a_plain_arrow_collapses_the_selection() {
        let mut e = TextEdit::new("abcd", 20);
        e.apply(Edit::Home { extend: false });
        e.apply(Edit::Right { extend: true });
        e.apply(Edit::Right { extend: true });
        assert_eq!(e.selection(), (0, 2));
        e.apply(Edit::Right { extend: false });
        assert_eq!((e.selection(), e.caret()), ((2, 2), 2));
        e.apply(Edit::End { extend: true });
        e.apply(Edit::Left { extend: false });
        assert_eq!(e.caret(), 2, "collapses to the left end");
        e.apply(Edit::SelectAll);
        assert!(e.apply(Edit::Back));
        assert_eq!(e.text(), "");
    }

    #[test]
    fn the_length_is_kept_to_its_limit_and_control_characters_are_refused() {
        let mut e = TextEdit::new("abcdef", 4);
        assert_eq!(e.text(), "abcd");
        e.apply(Edit::End { extend: false });
        assert!(!e.apply(Edit::Insert('x')), "full");
        assert!(!e.apply(Edit::Insert('\u{8}')));
        assert!(!e.apply(Edit::Insert('\t')));
        e.apply(Edit::SelectAll);
        assert!(
            e.apply(Edit::Insert('z')),
            "a full selection can still be replaced"
        );
        assert_eq!(e.text(), "z");
    }

    const PLAIN: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
    };
    const CTRL: Mods = Mods {
        ctrl: true,
        ..PLAIN
    };
    const SHIFT: Mods = Mods {
        shift: true,
        ..PLAIN
    };
    const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        shift: true,
        alt: false,
    };

    #[test]
    fn keys_map_to_the_edits_the_name_fields_used_to_spell_out() {
        assert_eq!(edit_for(Key::Delete, PLAIN), Some(Edit::Delete));
        assert_eq!(edit_for(Key::Backspace, PLAIN), Some(Edit::Back));
        assert_eq!(
            edit_for(Key::Left, PLAIN),
            Some(Edit::Left { extend: false })
        );
        assert_eq!(
            edit_for(Key::Right, SHIFT),
            Some(Edit::Right { extend: true })
        );
        assert_eq!(
            edit_for(Key::Home, SHIFT),
            Some(Edit::Home { extend: true })
        );
        assert_eq!(edit_for(Key::End, PLAIN), Some(Edit::End { extend: false }));
        assert_eq!(edit_for(Key::Char(b'A'), CTRL), Some(Edit::SelectAll));
    }

    #[test]
    fn ctrl_c_v_x_are_clipboard_edits_and_ctrl_arrows_move_by_words() {
        assert_eq!(edit_for(Key::Char(b'C'), CTRL), Some(Edit::Copy));
        assert_eq!(edit_for(Key::Char(b'V'), CTRL), Some(Edit::Paste));
        assert_eq!(edit_for(Key::Char(b'X'), CTRL), Some(Edit::Cut));
        assert_eq!(edit_for(Key::Char(b'V'), CTRL_SHIFT), Some(Edit::Paste));
        assert_eq!(
            edit_for(Key::Left, CTRL),
            Some(Edit::WordLeft { extend: false })
        );
        assert_eq!(
            edit_for(Key::Right, CTRL_SHIFT),
            Some(Edit::WordRight { extend: true })
        );
    }

    #[test]
    fn other_keys_and_alt_chords_are_not_edits() {
        let alt = Mods { alt: true, ..PLAIN };
        let ctrl_alt = Mods { alt: true, ..CTRL };
        for (key, mods) in [
            (Key::Char(b'C'), PLAIN),
            (Key::Char(b'A'), PLAIN),
            (Key::Char(b'Z'), CTRL),
            (Key::Char(b'V'), ctrl_alt),
            (Key::Char(b'A'), alt),
            (Key::Up, PLAIN),
            (Key::Enter, PLAIN),
            (Key::Escape, PLAIN),
            (Key::Space, PLAIN),
            (Key::Tab, PLAIN),
        ] {
            assert_eq!(edit_for(key, mods), None, "{key:?} {mods:?}");
        }
    }

    #[test]
    fn alt_keeps_the_caret_moves_it_had_and_ctrl_alt_arrows_are_not_word_moves() {
        let alt = Mods { alt: true, ..PLAIN };
        let alt_shift = Mods { alt: true, ..SHIFT };
        assert_eq!(edit_for(Key::Left, alt), Some(Edit::Left { extend: false }));
        assert_eq!(
            edit_for(Key::End, alt_shift),
            Some(Edit::End { extend: true })
        );
        assert_eq!(edit_for(Key::Backspace, alt), Some(Edit::Back));
        let ctrl_alt = Mods { alt: true, ..CTRL };
        assert_eq!(
            edit_for(Key::Left, ctrl_alt),
            Some(Edit::Left { extend: false })
        );
    }

    #[test]
    fn room_counts_what_a_paste_may_add_and_a_selection_gives_back() {
        let mut e = TextEdit::new("abcd", 6);
        assert_eq!(e.room(), 6, "the selection of all four is replaced");
        e.apply(Edit::End { extend: false });
        assert_eq!(e.room(), 2);
        e.apply(Edit::Left { extend: true });
        e.apply(Edit::Left { extend: true });
        assert_eq!(e.room(), 4);
        assert_eq!(TextEdit::new("", 0).room(), 0);
    }

    fn at(text: &str, caret: usize) -> TextEdit {
        let mut e = TextEdit::new(text, 80);
        e.apply(Edit::Home { extend: false });
        for _ in 0..caret {
            e.apply(Edit::Right { extend: false });
        }
        e
    }

    #[test]
    fn word_moves_land_on_word_starts_and_ends() {
        let mut e = at("hello big  world", 0);
        let mut stops = Vec::new();
        for _ in 0..3 {
            e.apply(Edit::WordRight { extend: false });
            stops.push(e.caret());
        }
        assert_eq!(
            stops,
            [6, 11, 16],
            "to the start of the next word, then the end"
        );
        for want in [11, 6, 0, 0] {
            e.apply(Edit::WordLeft { extend: false });
            assert_eq!(e.caret(), want);
        }
    }

    #[test]
    fn punctuation_is_a_word_of_its_own_and_shift_extends_the_selection() {
        let mut e = at("a.b", 0);
        e.apply(Edit::WordRight { extend: false });
        assert_eq!(e.caret(), 1);
        e.apply(Edit::WordRight { extend: true });
        assert_eq!((e.caret(), e.selection()), (2, (1, 2)));
        e.apply(Edit::WordLeft { extend: true });
        assert_eq!(e.selection(), (1, 1));
        let mut e = at("one two", 7);
        e.apply(Edit::WordLeft { extend: true });
        assert_eq!(e.selection(), (4, 7));
        assert_eq!(e.selected_text(), "two");
    }

    #[test]
    fn a_plain_word_move_starts_from_the_caret() {
        let mut e = TextEdit::new("one two", 20);
        assert_eq!(e.selection(), (0, 7));
        e.apply(Edit::WordLeft { extend: false });
        assert_eq!(e.selection(), (4, 4));
    }

    #[test]
    fn copy_and_paste_do_not_change_the_text_and_cut_removes_the_selection() {
        let mut e = TextEdit::new("hello world", 20);
        e.apply(Edit::Home { extend: false });
        for _ in 0..5 {
            e.apply(Edit::Right { extend: true });
        }
        assert_eq!(e.selected_text(), "hello");
        assert!(!e.apply(Edit::Copy));
        assert!(!e.apply(Edit::Paste));
        assert_eq!(e.text(), "hello world");
        assert!(e.apply(Edit::Cut));
        assert_eq!((e.text(), e.caret()), (" world", 0));
        assert!(!e.apply(Edit::Cut), "nothing selected, nothing cut");
        assert_eq!(e.text(), " world");
    }

    #[test]
    fn selected_text_follows_characters_not_bytes() {
        let mut e = TextEdit::new("aéb漢", 20);
        assert_eq!(e.selected_text(), "aéb漢");
        e.apply(Edit::Home { extend: false });
        e.apply(Edit::Right { extend: false });
        e.apply(Edit::Right { extend: true });
        assert_eq!(e.selected_text(), "é");
        e.apply(Edit::End { extend: false });
        assert_eq!(e.selected_text(), "");
    }

    #[test]
    fn pasted_text_is_one_line_without_control_characters() {
        assert_eq!(one_line("walk cycle"), "walk cycle");
        assert_eq!(one_line("first\r\nsecond"), "first");
        assert_eq!(one_line("a\tb\u{7}c\u{0}d"), "a bcd");
        assert_eq!(one_line("\nlater"), "");
        assert_eq!(one_line("løp 漢"), "løp 漢");
    }

    #[test]
    fn a_newline_is_an_edit_of_its_own_and_counts_toward_the_limit() {
        let mut e = TextEdit::new("", 3);
        typed(&mut e, "a");
        assert!(e.apply(Edit::Newline));
        typed(&mut e, "b");
        assert_eq!(e.text(), "a\nb");
        assert!(!e.apply(Edit::Newline), "full");
        assert!(
            !e.apply(Edit::Insert('\n')),
            "a typed control is still refused"
        );
        e.apply(Edit::SelectAll);
        assert!(e.apply(Edit::Newline), "it replaces a selection");
        assert_eq!(e.text(), "\n");
    }

    #[test]
    fn the_shown_copy_keeps_the_caret_and_selection() {
        let mut e = TextEdit::new("a\nb", 9);
        e.apply(Edit::Left { extend: true });
        let shown = e.mapped(|c| if c == '\n' { '\u{b6}' } else { c });
        assert_eq!(shown.text(), "a\u{b6}b");
        assert_eq!(
            (shown.caret(), shown.selection()),
            (e.caret(), e.selection())
        );
    }

    #[test]
    fn pasted_text_keeps_its_lines_when_the_field_has_them() {
        assert_eq!(multi_line("a\r\nb\rc\nd"), "a\nb\nc\nd");
        assert_eq!(multi_line("a\tb\u{7}c"), "a bc");
        assert_eq!(multi_line("\n\n"), "\n\n");
    }
}
