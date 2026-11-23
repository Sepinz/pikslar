use super::{is_hidden, spaced, without_hidden};

const HIDDEN: [char; 8] = [
    '\u{0}', '\u{7f}', '\u{85}', '\u{200B}', '\u{200F}', '\u{202A}', '\u{202E}', '\u{2066}',
];

#[test]
fn every_kind_of_hidden_character_is_found() {
    for c in HIDDEN.into_iter().chain(['\u{2069}', '\n', '\t']) {
        assert!(is_hidden(c), "{c:?}");
    }
    for c in [
        'a', ' ', 'é', '\u{200A}', '\u{2010}', '\u{2029}', '\u{206A}', '\u{FFFD}', '😀',
    ] {
        assert!(!is_hidden(c), "{c:?}");
    }
}

#[test]
fn spacing_replaces_each_hidden_character_and_leaves_clean_text_alone() {
    assert_eq!(spaced("a\u{202E}b\nc\u{200B}d"), "a b c d");
    for clean in ["Walls", "Grün  ", "日本語 name", ""] {
        assert_eq!(spaced(clean), clean);
        assert_eq!(without_hidden(clean), clean);
    }
}

#[test]
fn leaving_out_drops_each_hidden_character() {
    assert_eq!(without_hidden("a\u{202E}b\nc\u{2066}d"), "abcd");
}
