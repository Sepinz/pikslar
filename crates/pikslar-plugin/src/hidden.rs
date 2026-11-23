//! Characters that change what text looks like without being seen as themselves. One strict
//! list serves every place text from outside is shown: a plugin's name and titles are refused
//! if they hold any ([`crate::names`]), and a script's output, a stray folder's name and the
//! like are shown with them taken out ([`shown`]).
//!
//! The list is Unicode's Default_Ignorable_Code_Point set, the other format controls (Cf),
//! every space but U+0020, the object replacement character, private use, noncharacters and
//! control characters. A run of more than [`MOST_MARKS`] combining marks on one character is
//! a flood that can paint over the lines around it, and is cut back.

/// The most combining marks kept on one character.
pub const MOST_MARKS: usize = 2;

/// Control characters, Default_Ignorable_Code_Point, the other format controls, private use,
/// noncharacters and U+FFFC.
pub fn is_hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            // Default_Ignorable_Code_Point (Unicode 15.1, DerivedCoreProperties).
            '\u{00AD}'
                | '\u{034F}'
                | '\u{061C}'
                | '\u{115F}'..='\u{1160}'
                | '\u{17B4}'..='\u{17B5}'
                | '\u{180B}'..='\u{180F}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{3164}'
                | '\u{FE00}'..='\u{FE0F}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{FFF0}'..='\u{FFF8}'
                | '\u{1BCA0}'..='\u{1BCA3}'
                | '\u{1D173}'..='\u{1D17A}'
                | '\u{E0000}'..='\u{E0FFF}'
                // Other format controls, and fillers drawn as nothing.
                | '\u{0600}'..='\u{0605}'
                | '\u{06DD}'
                | '\u{070F}'
                | '\u{0890}'..='\u{0891}'
                | '\u{08E2}'
                | '\u{110BD}'
                | '\u{110CD}'
                | '\u{13430}'..='\u{1345F}'
                | '\u{2028}'..='\u{2029}'
                | '\u{2800}'
                | '\u{FFF9}'..='\u{FFFC}'
                // Private use and noncharacters.
                | '\u{E000}'..='\u{F8FF}'
                | '\u{F0000}'..='\u{10FFFF}'
                | '\u{FDD0}'..='\u{FDEF}'
                | '\u{FFFE}'..='\u{FFFF}'
        )
}

/// A space other than U+0020: no-break, the typographic spaces, the ideographic one.
pub fn is_other_space(c: char) -> bool {
    c != ' '
        && matches!(
            c,
            '\u{00A0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
        )
}

/// A combining mark from the blocks that hold only them, which any letter can carry.
pub fn is_combining(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036F}'
            | '\u{0483}'..='\u{0489}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
    )
}

/// The first character of `text` that may not be shown as it is: hidden, a space other than
/// U+0020, or a combining mark past [`MOST_MARKS`] in a row.
#[inline(never)]
pub fn first_unshown(text: &str) -> Option<char> {
    let mut marks = 0;
    for c in text.chars() {
        marks = if is_combining(c) { marks + 1 } else { 0 };
        if is_hidden(c) || is_other_space(c) || marks > MOST_MARKS {
            return Some(c);
        }
    }
    None
}

/// `text` as it may be shown: hidden characters taken out, other spaces and tabs made plain
/// spaces, and combining marks past [`MOST_MARKS`] in a row dropped.
#[inline(never)]
pub fn shown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut marks = 0;
    for c in text.chars() {
        marks = if is_combining(c) { marks + 1 } else { 0 };
        if is_other_space(c) || c == '\t' {
            out.push(' ');
        } else if !is_hidden(c) && marks <= MOST_MARKS {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNSHOWN: [&str; 20] = [
        "Safe\u{202E}txt",
        "Zero\u{200B}width",
        "Iso\u{2066}late",
        "\u{FEFF}Bom",
        "Tag\u{E0041}",
        "Blank\u{2800}",
        "Heart\u{FE0F}",
        "Mongol\u{180B}",
        "Mongol\u{180F}",
        "Short\u{1BCA0}hand",
        "Egypt\u{13430}",
        "Object\u{FFFC}",
        "No\u{00A0}break",
        "En\u{2002}space",
        "Ideo\u{3000}graphic",
        "Hair\u{200A}space",
        "Private\u{E000}",
        "Line\u{2028}sep",
        "Non\u{FFFF}char",
        "Flood e\u{0301}\u{0302}\u{0303}",
    ];

    #[test]
    fn every_kind_of_hidden_character_is_caught() {
        for text in UNSHOWN {
            assert!(first_unshown(text).is_some(), "{text:?} passed");
        }
        for fine in [
            "Outline tool",
            "Kontur für Ränder",
            "e\u{0301}\u{0302}",
            "中文",
        ] {
            assert_eq!(first_unshown(fine), None, "{fine:?}");
        }
    }

    #[test]
    fn shown_text_takes_them_out() {
        assert_eq!(shown("a\u{202E}b\u{200B}c"), "abc");
        assert_eq!(shown("No\u{00A0}break\ttab"), "No break tab");
        assert_eq!(
            shown("e\u{0301}\u{0302}\u{0303}\u{0304}x"),
            "e\u{0301}\u{0302}x"
        );
        assert_eq!(shown("Kontur für Ränder 中文"), "Kontur für Ränder 中文");
        for text in UNSHOWN {
            assert_eq!(first_unshown(&shown(text)), None, "{text:?}");
        }
    }
}
