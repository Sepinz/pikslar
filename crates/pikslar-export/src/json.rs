//! Writing JSON by hand, which is all the descriptions need.

use std::fmt::Write;

/// Appends `text` as a JSON string, quoted and escaped.
#[inline(never)]
pub fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c < ' ' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_escaped_for_json() {
        let mut out = String::new();
        push_string(&mut out, "a\"b\\c\n");
        assert_eq!(out, "\"a\\\"b\\\\c\\u000a\"");
    }
}
