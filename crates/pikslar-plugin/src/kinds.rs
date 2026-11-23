//! The only kinds of file a plugin may write: plain data and pictures.
//!
//! A plugin's text is harmless as a picture's report, but the same text saved as `run.pyw`,
//! `x.rdp`, `x.svg` or `x.html` is something this PC opens, runs or renders when it is
//! double-clicked. A block-list of dangerous kinds can never be complete (Windows and the
//! programs installed on it add new ones), so the kinds are an allow-list: a kind not on it
//! is refused in the kinds a save dialog is asked to offer, and again on the name the user
//! finally chose (they can type any name). A name without an extension is allowed: Windows
//! runs nothing by double-clicking one.

/// Extensions a plugin may write, lower case, without the dot. Kinds Pikslar itself reads or
/// writes (pictures and palettes), and plain text and data.
#[rustfmt::skip]
pub const ALLOWED_EXTENSIONS: &[&str] = &[
    // Plain text and data.
    "txt", "md", "csv", "tsv", "json",
    // Palettes.
    "gpl", "hex", "pal", "act", "ase",
    // Pictures and Pikslar's own documents.
    "png", "gif", "bmp", "piks", "aseprite",
];

/// True when `extension` (without its dot, any case) is one a plugin may write. Nothing
/// around it is ignored: a space or a dot makes it a different, unlisted extension.
#[inline(never)]
pub fn is_allowed_extension(extension: &str) -> bool {
    ALLOWED_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
}

/// True when a plugin may write a file of this name: it has no extension, or its last one is
/// on [`ALLOWED_EXTENSIONS`]. Trailing dots and spaces, which Windows drops, are dropped
/// first, so `a.pyw.` and `a.pyw ` are judged as `a.pyw`.
#[inline(never)]
pub fn is_allowed_name(name: &str) -> bool {
    let name = name.trim_end_matches(['.', ' ']);
    name.rsplit_once('.')
        .is_none_or(|(_, extension)| is_allowed_extension(extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_scripts_and_everything_else_unlisted_are_refused_in_any_case() {
        for name in [
            "report.bat",
            "Report.Cmd",
            "a.ps1",
            "a.pyw",
            "a.PYW",
            "a.py",
            "a.sh",
            "a.rdp",
            "a.theme",
            "a.wsb",
            "a.msu",
            "a.msix",
            "a.appx",
            "a.cer",
            "a.svg",
            "a.html",
            "a.htm",
            "a.hlp",
            "a.diagpkg",
            "a.ini",
            "a.xml",
            "a.exe",
            "a.lnk",
            "a.url",
            "a.settingcontent-ms",
            ".bat",
            "a.txt.bat",
            "a.pyw.",
            "a.pyw ",
            "a.pyw . ",
            "a.txt\u{200b}",
            "a.t xt",
        ] {
            assert!(!is_allowed_name(name), "{name} was allowed");
        }
    }

    #[test]
    fn data_and_picture_kinds_and_names_without_an_extension_are_allowed() {
        for name in [
            "report.txt",
            "REPORT.TXT",
            "a.csv",
            "a.tsv",
            "a.json",
            "a.md",
            "a.gpl",
            "a.hex",
            "a.pal",
            "a.act",
            "a.ase",
            "a.png",
            "a.gif",
            "a.bmp",
            "a.piks",
            "a.aseprite",
            "a.bat.txt",
            "README",
        ] {
            assert!(is_allowed_name(name), "{name} was refused");
        }
        assert!(is_allowed_extension("PNG"));
        assert!(!is_allowed_extension(" png"));
        assert!(!is_allowed_extension("exe"));
    }

    #[test]
    fn every_listed_kind_is_plain_lower_case() {
        for kind in ALLOWED_EXTENSIONS {
            assert!(kind.bytes().all(|b| b.is_ascii_lowercase()), "{kind}");
        }
    }
}
