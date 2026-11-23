//! The one rule for characters a name may not keep, whether it is typed or read from a file:
//! control characters, the bidirectional overrides and isolates (which can make a name show as
//! something else), and the zero-width and direction marks (which leave nothing to see).

/// True for a character no name keeps: a control character (C0, DEL, C1), a zero-width or
/// direction mark (U+200B to U+200F), a bidirectional override (U+202A to U+202E) or an
/// isolate (U+2066 to U+2069).
pub fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// The text with each hidden character turned into a space. Text without any is unchanged.
pub fn spaced(text: &str) -> String {
    text.chars()
        .map(|c| if is_hidden(c) { ' ' } else { c })
        .collect()
}

/// The text with every hidden character left out. Text without any is unchanged.
pub fn without_hidden(text: &str) -> String {
    text.chars().filter(|&c| !is_hidden(c)).collect()
}

#[cfg(test)]
#[path = "names_tests.rs"]
mod tests;
