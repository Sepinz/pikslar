//! The names a plugin may use: its id, its file names, its command ids, and text shown to the
//! user. Each check is a whitelist, so separators, drive letters, `..`, `:` (alternate data
//! streams), trailing dots and spaces, upper case (case collisions) and control characters are
//! refused because they are never allowed, not because each was thought of.

/// The manifest's file name inside a plugin folder.
pub const MANIFEST_FILE: &str = "plugin.json";
/// Script files end with this.
pub const SCRIPT_EXTENSION: &str = ".pks";

const MOST_ID_CHARS: usize = 64;
const MOST_ID_PARTS: usize = 6;
const MOST_PART_CHARS: usize = 32;
const MOST_STEM_CHARS: usize = 32;

/// Names Windows keeps for devices, whatever follows a dot. Refused as a file stem or as the
/// first part of an id, since an id names a folder. COM and LPT are kept with 0 to 9 and with
/// the superscripts one to three.
const DEVICES: [&str; 6] = ["con", "prn", "aux", "nul", "conin$", "conout$"];

pub(crate) fn is_device(stem: &str) -> bool {
    let numbered = |p: &str| {
        stem.strip_prefix(p).is_some_and(|n| {
            let mut c = n.chars();
            matches!(
                (c.next(), c.next()),
                (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
            )
        })
    };
    DEVICES.contains(&stem) || numbered("com") || numbered("lpt")
}

/// One lower-case word: letters, digits and inner hyphens, starting with a letter.
fn is_word(part: &str, most: usize) -> bool {
    let bytes = part.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= most
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1] != b'-'
        && bytes
            .iter()
            .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A plugin id: reverse-DNS-like, `com.example.outline`. Two to six dot-separated words of
/// lower-case letters, digits and hyphens, each starting with a letter.
#[inline(never)]
pub fn check_id(id: &str) -> Result<(), String> {
    let parts: Vec<&str> = id.split('.').collect();
    if id.len() > MOST_ID_CHARS {
        return Err(format!("the id is longer than {MOST_ID_CHARS} characters"));
    }
    if parts.len() < 2 || parts.len() > MOST_ID_PARTS {
        return Err(format!(
            "the id {id:?} needs 2 to {MOST_ID_PARTS} parts separated by dots, like com.example.tool"
        ));
    }
    if let Some(bad) = parts.iter().find(|p| !is_word(p, MOST_PART_CHARS)) {
        return Err(format!(
            "the id part {bad:?} must be lower-case letters, digits and hyphens, starting with a letter"
        ));
    }
    match is_device(parts[0]) {
        true => Err(format!(
            "the id {id:?} begins with a name Windows keeps for a device"
        )),
        false => Ok(()),
    }
}

/// A command id inside one plugin: one word, as in [`check_id`].
#[inline(never)]
pub fn check_command_id(id: &str) -> Result<(), String> {
    match is_word(id, MOST_PART_CHARS) {
        true => Ok(()),
        false => Err(format!(
            "the command id {id:?} must be one lower-case word of letters, digits and hyphens"
        )),
    }
}

/// A script's file name: a lower-case word (underscores allowed) and `.pks`, nothing else.
#[inline(never)]
pub fn check_script_name(name: &str) -> Result<(), String> {
    let refuse = || {
        Err(format!(
            "the file name {name:?} is not allowed: scripts are named like outline.pks, in \
             lower-case letters, digits, hyphens and underscores, with no folders"
        ))
    };
    let Some(stem) = name.strip_suffix(SCRIPT_EXTENSION) else {
        return refuse();
    };
    let ok = !stem.is_empty()
        && stem.len() <= MOST_STEM_CHARS
        && stem.as_bytes()[0].is_ascii_lowercase()
        && stem
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    match (ok, is_device(stem)) {
        (false, _) => refuse(),
        (true, true) => Err(format!(
            "the file name {name:?} is a name Windows keeps for a device"
        )),
        (true, false) => Ok(()),
    }
}

/// Text shown to the user (a name, a title, an author): nothing [`crate::hidden`] catches,
/// which could hide or reorder what the user reads, and no space at either end.
#[inline(never)]
pub fn check_display(what: &str, text: &str, most_chars: usize) -> Result<(), String> {
    let count = text.chars().count();
    if count == 0 {
        return Err(format!("the {what} is empty"));
    }
    if count > most_chars {
        return Err(format!("the {what} is longer than {most_chars} characters"));
    }
    if text.trim() != text {
        return Err(format!("the {what} begins or ends with a space"));
    }
    match crate::hidden::first_unshown(text) {
        Some(c) => Err(format!(
            "the {what} holds the character U+{:04X}, which is not shown as itself",
            u32::from(c)
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_take_reverse_dns_words_only() {
        for good in ["com.example.outline", "org.pikslar.a1", "io.x-y.z", "a.b"] {
            assert_eq!(check_id(good), Ok(()), "{good}");
        }
        for bad in [
            "",
            "outline",
            "com..x",
            ".com.x",
            "com.x.",
            "Com.example",
            "com.example.Tool",
            "com.1x",
            "com.x-",
            "com/x.y",
            "..\\x.y",
            "com.ex ample",
            "com.exämple",
            "con.example",
            "nul.x",
            "com1.x",
            "com0.x",
            "lpt0.example",
            "a.b.c.d.e.f.g",
            "c:.x",
            "com.x:y",
        ] {
            assert!(check_id(bad).is_err(), "{bad:?} taken");
        }
        assert!(check_id(&format!("com.{}", "a".repeat(70))).is_err());
    }

    #[test]
    fn script_names_refuse_paths_devices_streams_and_case() {
        for good in ["main.pks", "outline_2.pks", "a-b.pks"] {
            assert_eq!(check_script_name(good), Ok(()), "{good}");
        }
        for bad in [
            "",
            ".pks",
            "main",
            "main.PKS",
            "Main.pks",
            "MAIN.pks",
            "../main.pks",
            "..\\main.pks",
            "dir/main.pks",
            "dir\\main.pks",
            "C:\\main.pks",
            "C:main.pks",
            "/main.pks",
            "\\\\server\\share\\main.pks",
            "main.pks:hidden",
            "main.pks::$DATA",
            "main.pks.",
            "main.pks ",
            " main.pks",
            "con.pks",
            "nul.pks",
            "com1.pks",
            "lpt9.pks",
            "com0.pks",
            "lpt0.pks",
            "conin$.pks",
            "main.txt",
            "main.pks.exe",
            "plugin.json",
            "1main.pks",
            "ma\u{0}in.pks",
        ] {
            assert!(check_script_name(bad).is_err(), "{bad:?} taken");
        }
        assert!(check_script_name(&format!("{}.pks", "a".repeat(33))).is_err());
    }

    #[test]
    fn display_text_refuses_hidden_and_reordering_characters() {
        assert_eq!(check_display("name", "Outline tool", 48), Ok(()));
        assert_eq!(check_display("name", "Kontur für Ränder", 48), Ok(()));
        for bad in [
            "",
            " Outline",
            "Outline ",
            "Out\nline",
            "Out\tline",
            "Safe\u{202E}txt.exe",
            "Zero\u{200B}width",
            "Iso\u{2066}late",
            "\u{FEFF}Bom",
            "Tag\u{E0041}",
            "Blank\u{2800}",
            "Heart\u{FE0F}",
            "Mark\u{034F}",
            "Private\u{E000}",
            "Mongol\u{180B}ian",
            "Short\u{1BCA1}hand",
            "Egypt\u{13431}",
            "Object\u{FFFC}",
            "No\u{00A0}break",
            "Em\u{2003}space",
            "Ideo\u{3000}graphic",
            "Flood a\u{0300}\u{0301}\u{0302}",
        ] {
            assert!(check_display("name", bad, 48).is_err(), "{bad:?} taken");
        }
        assert!(check_display("name", &"x".repeat(49), 48).is_err());
        assert_eq!(check_display("name", &"é".repeat(48), 48), Ok(()));
    }
}
