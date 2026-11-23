//! Where an export may write. Every target is checked before anything is written, by the
//! command line and the app alike.

use pikslar_format::atomic::RUN_TEMP_EXTRA;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Why a file may not be written where it was asked.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// A file is there, and replacing it was not asked for.
    Exists(PathBuf),
    /// Anything else, said in full.
    Other(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::Exists(path) => write!(f, "{} already exists", path.display()),
            Refused::Other(message) => f.write_str(message),
        }
    }
}

/// Names Windows keeps for devices, whatever extension follows them: the superscript forms of
/// COM and LPT among them, and COM0 and LPT0, which Microsoft lists as reserved too.
const RESERVED: [&str; 32] = [
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "COM0",
    "COM1",
    "COM2",
    "COM3",
    "COM4",
    "COM5",
    "COM6",
    "COM7",
    "COM8",
    "COM9",
    "COM\u{b9}",
    "COM\u{b2}",
    "COM\u{b3}",
    "LPT0",
    "LPT1",
    "LPT2",
    "LPT3",
    "LPT4",
    "LPT5",
    "LPT6",
    "LPT7",
    "LPT8",
    "LPT9",
    "LPT\u{b9}",
    "LPT\u{b2}",
    "LPT\u{b3}",
    "CONIN$",
    "CONOUT$",
];

fn shown(path: &Path) -> String {
    path.display().to_string()
}

/// Characters Windows refuses in a file name. A colon would name a stream inside another file.
const NOT_IN_NAMES: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// A name Windows would refuse, change or treat as a device, so the file would not be what was
/// asked.
#[inline(never)]
pub fn check_name(path: &Path) -> Result<(), Refused> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return Err(Refused::Other(format!("{} names no file", shown(path))));
    };
    if name
        .chars()
        .any(|c| c.is_control() || NOT_IN_NAMES.contains(&c))
    {
        return Err(Refused::Other(format!(
            "\"{}\" holds a character Windows does not allow in a file name",
            name.escape_debug()
        )));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(Refused::Other(format!(
            "\"{name}\" ends in a dot or a space, which Windows removes"
        )));
    }
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return Err(Refused::Other(format!(
            "\"{name}\" is a name Windows keeps for a device"
        )));
    }
    if name.encode_utf16().count() > LONGEST_NAME {
        return Err(Refused::Other(format!(
            "\"{name}\" is too long. A name can have at most {LONGEST_NAME} characters, so that \
             its temporary file fits beside it"
        )));
    }
    Ok(())
}

/// Windows allows 255 characters in a name, and an export run's temporary file adds its tag and
/// suffix.
const LONGEST_NAME: usize = 255 - RUN_TEMP_EXTRA;

/// The folder must exist, and none of its names may end in a dot or a space, which Windows
/// removes, so the file would land somewhere other than where it was asked.
fn check_folder(path: &Path) -> Result<(), Refused> {
    let folder = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    for part in folder.components() {
        let Component::Normal(name) = part else {
            continue;
        };
        let name = name.to_string_lossy();
        if name.ends_with('.') || name.ends_with(' ') {
            return Err(Refused::Other(format!(
                "The folder name \"{name}\" ends in a dot or a space, which Windows removes"
            )));
        }
    }
    if !folder.is_dir() {
        return Err(Refused::Other(format!(
            "The folder {} does not exist",
            shown(&folder)
        )));
    }
    Ok(())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Refuses a target that cannot or should not be written: a bad name, a missing folder, a
/// folder, a link, the project itself, an existing file without `overwrite`, or a read-only file.
/// Temporary files need no check: each export run names its own and only ever makes them new.
#[inline(never)]
pub fn check_target(path: &Path, project: Option<&Path>, overwrite: bool) -> Result<(), Refused> {
    check_name(path)?;
    check_folder(path)?;
    if path.is_dir() {
        return Err(Refused::Other(format!("{} is a folder", shown(path))));
    }
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(Refused::Other(format!(
            "{} is a link. Export replaces files, not what links point to",
            shown(path)
        )));
    }
    if project.is_some_and(|project| same_file(path, project)) {
        return Err(Refused::Other(format!(
            "{} is the project being exported",
            shown(path)
        )));
    }
    if path.exists() && !overwrite {
        return Err(Refused::Exists(path.to_path_buf()));
    }
    let read_only = fs::metadata(path).is_ok_and(|m| m.permissions().readonly());
    if read_only {
        return Err(Refused::Other(format!("{} is read-only", shown(path))));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_and_trailing_dots_are_refused_in_any_case() {
        for name in [
            "CON.png",
            "nul.png",
            "Com1.png",
            "lpt9.x.png",
            "aux .png",
            "a.png.",
            "a.png ",
            "ab:c.png",
            "x?.png",
            "a*.png",
            "a<b.png",
            "a|b.png",
            "a\"b.png",
            "a\u{7}.png",
            "COM0.png",
            "lpt0.piks",
            "COM\u{b9}.piks",
            "com\u{b2}.png",
            "LPT\u{b3}.png",
            "conin$.png",
            "CONOUT$.piks",
        ] {
            assert!(check_name(Path::new(name)).is_err(), "{name}");
        }
        for name in ["console.png", "con1.png", "a.png", "COM10.png"] {
            check_name(Path::new(name)).expect(name);
        }
    }

    #[test]
    fn a_file_there_is_refused_only_without_overwrite_and_names_itself() {
        let dir = std::env::temp_dir().join(format!("pikslar-targets-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        let there = dir.join("a.png");
        fs::write(&there, "x").expect("write");
        assert_eq!(
            check_target(&there, None, false),
            Err(Refused::Exists(there.clone()))
        );
        assert_eq!(
            Refused::Exists(there.clone()).to_string(),
            format!("{} already exists", there.display())
        );
        assert_eq!(check_target(&there, None, true), Ok(()));
        let as_project = check_target(&there, Some(&there), true).expect_err("the project");
        assert!(as_project
            .to_string()
            .contains("is the project being exported"));
        assert_eq!(
            check_target(&dir.join("b.png"), Some(&there), false),
            Ok(())
        );
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
